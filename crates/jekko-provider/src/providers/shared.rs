//! Shared HTTP plumbing for provider adapters.
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use crate::adapter::{ProviderRequest, ProviderStream};
use crate::error::{ProviderError, ProviderResult};
use crate::stream::{ProviderCapabilities, ProviderEvent};

mod sse;

pub use sse::{
    parse_data_as_json, post_json_sse_stream, preparse_sse_frame, sse_decode_all,
    sse_into_provider_stream, McpReceiverStream, SsePreparse,
};

/// Default request timeout for non-streaming setup (headers / connect).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Construct a [`reqwest::Client`] tuned for streaming SSE.
///
/// The builder options used here (no TLS config, no proxy override) cannot
/// fail at runtime, so we surface any unexpected error via `expect` rather
/// than silently degrading to a default client that would lose the SSE
/// pool/nodelay tuning.
pub fn make_client() -> reqwest::Client {
    reqwest::Client::builder()
        .pool_max_idle_per_host(0)
        .tcp_nodelay(true)
        .build()
        .expect("reqwest client builder with default TLS must not fail")
}

/// Build a [`HeaderMap`] from a [`ProviderRequest::headers`] string map,
/// silently skipping headers with invalid names/values (e.g. accidentally
/// inserted control bytes from user config).
pub fn headers_from(req: &ProviderRequest) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (k, v) in &req.headers {
        if let (Ok(name), Ok(value)) = (HeaderName::try_from(k.as_str()), HeaderValue::try_from(v))
        {
            map.insert(name, value);
        }
    }
    map
}

/// Build the OpenAI-compatible JSON body used by OpenAI and Jekko-style
/// adapters.
pub fn build_openai_style_body(req: &ProviderRequest) -> Value {
    let mut body = Map::new();
    body.insert("model".into(), Value::String(req.api_model_id.clone()));
    body.insert("stream".into(), Value::Bool(true));
    body.insert(
        "max_completion_tokens".into(),
        Value::Number(serde_json::Number::from(req.max_output_tokens)),
    );

    if let Some(t) = req.temperature {
        body.insert(
            "temperature".into(),
            serde_json::Number::from_f64(t)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }
    if let Some(p) = req.top_p {
        body.insert(
            "top_p".into(),
            serde_json::Number::from_f64(p)
                .map(Value::Number)
                .unwrap_or(Value::Null),
        );
    }

    let mut messages: Vec<Value> = Vec::new();
    for seg in &req.system {
        messages.push(json!({ "role": "system", "content": seg }));
    }
    messages.extend(req.messages.clone());
    body.insert("messages".into(), Value::Array(messages));

    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema.clone(),
                    },
                })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        if let Some(tc) = &req.tool_choice {
            let v = match tc.as_str() {
                "auto" => json!("auto"),
                "required" => json!("required"),
                "none" => json!("none"),
                other => json!(other),
            };
            body.insert("tool_choice".into(), v);
        }
    }

    if let Some(opts) = req.options.get("openai").and_then(Value::as_object) {
        for (k, v) in opts {
            body.insert(k.clone(), v.clone());
        }
    }

    if let Some(store) = req.options.get("store") {
        body.insert("store".into(), store.clone());
    }
    // ZYAL `quality_band` request hint — flows through as a top-level
    // field so fusion's RequestProfile::from_request can lift it into
    // its `extra` map. Set by `jekko run` from JEKKO_RUN_QUALITY_BAND,
    // which jekko-runner sets per stage from the manifest's
    // `model_policy.<role>.quality_band` declaration. See
    // docs/ZYAL/MODEL_QUALITY_BAND.md.
    if let Some(band) = req.options.get("quality_band") {
        body.insert("quality_band".into(), band.clone());
    }
    if let Some(key) = req.options.get("promptCacheKey") {
        body.insert("prompt_cache_key".into(), key.clone());
    }

    Value::Object(body)
}

/// Build the OpenAI-style authorization headers shared by OpenAI and
/// Jekko-style adapters.
pub fn build_openai_style_headers(
    req: &ProviderRequest,
    provider_id: &str,
) -> ProviderResult<HeaderMap> {
    let mut headers = headers_from(req);
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    let cred = crate::adapter::require_credential(req, provider_id)?;
    let bearer = match cred {
        crate::adapter::ProviderCredential::ApiKey { key } => key.clone(),
        crate::adapter::ProviderCredential::Bearer { token } => token.clone(),
        crate::adapter::ProviderCredential::OAuth { access_token } => access_token.clone(),
    };
    headers.insert(
        reqwest::header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {bearer}"))
            .map_err(|_| ProviderError::MissingCredential(provider_id.into()))?,
    );
    Ok(headers)
}

/// Internal spec for OpenAI-compatible provider adapters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpenAiCompatSpec {
    /// Provider identifier used for credential lookup and errors.
    pub(crate) provider_id: &'static str,
    /// Default base URL when the request does not override it.
    pub(crate) default_base_url: &'static str,
    /// Adapter-specific extra headers appended after the OpenAI-style set.
    pub(crate) extra_headers: fn(&ProviderRequest, &mut HeaderMap) -> ProviderResult<()>,
}

/// Internal helper that owns the shared request and streaming scaffold for
/// OpenAI-compatible providers.
#[derive(Debug, Clone)]
pub(crate) struct OpenAiCompatAdapter {
    client: reqwest::Client,
    spec: OpenAiCompatSpec,
}

impl OpenAiCompatAdapter {
    /// Construct a new helper for a specific provider shape.
    pub(crate) fn new(spec: OpenAiCompatSpec) -> Self {
        Self {
            client: make_client(),
            spec,
        }
    }

    /// Build the JSON body for the underlying chat-completions call.
    pub(crate) fn build_body(&self, req: &ProviderRequest) -> Value {
        build_openai_style_body(req)
    }

    /// Build the request headers, including provider-specific metadata.
    pub(crate) fn build_headers(&self, req: &ProviderRequest) -> ProviderResult<HeaderMap> {
        let mut headers = build_openai_style_headers(req, self.spec.provider_id)?;
        (self.spec.extra_headers)(req, &mut headers)?;
        Ok(headers)
    }

    /// Stream the response from the provider's chat-completions endpoint.
    pub(crate) async fn stream(
        &self,
        req: ProviderRequest,
        abort: CancellationToken,
    ) -> ProviderResult<McpReceiverStream<ProviderResult<ProviderEvent>>> {
        let base = req
            .base_url
            .as_deref()
            .unwrap_or(self.spec.default_base_url);
        let url = format!("{base}/v1/chat/completions");
        let body = self.build_body(&req);
        let headers = self.build_headers(&req)?;

        let mut state = super::openai::OpenAiStreamState::new();
        let stream =
            post_json_sse_stream(&self.client, &url, headers, &body, abort, move |frame| {
                super::openai::map_openai_frame_stateful(frame, &mut state)
            })
            .await?;
        Ok(stream)
    }

    /// Stream the response as a fully boxed [`ProviderStream`], shared by
    /// every adapter that simply delegates to the OpenAI-compatible pipeline.
    pub(crate) async fn boxed_stream(
        &self,
        req: ProviderRequest,
        abort: CancellationToken,
    ) -> ProviderResult<ProviderStream> {
        let stream = self.stream(req, abort).await?;
        Ok(Box::pin(stream))
    }
}

/// Capability set shared by every OpenAI-compatible delegating adapter (Jekko,
/// JNOccio). Hoisted so the adapter wrappers stay symbolic 1-liners.
pub(crate) fn openai_compat_capabilities() -> ProviderCapabilities {
    ProviderCapabilities {
        streaming: true,
        cache_control: false,
        tool_streaming: true,
    }
}

/// Generate a public `*Adapter` newtype that delegates every method to an
/// internal [`OpenAiCompatAdapter`]. Keeps the surface area identical to a
/// hand-written wrapper but eliminates the per-provider boilerplate.
///
/// Each invocation produces:
/// - a `pub struct $name { inner: OpenAiCompatAdapter }`
/// - inherent constructors and `build_body` / `build_headers` / `map_frame`
/// - `Default` and `ProviderAdapter` impls
///
/// The spec fields are baked into a `const ${NAME}_SPEC: OpenAiCompatSpec`
/// hidden in the macro expansion, so there is no per-provider spec literal
/// duplicated in product code. `$doc` becomes the struct docstring.
#[macro_export]
macro_rules! define_openai_compat_adapter {
    (
        $(#[$meta:meta])*
        $name:ident,
        provider_id = $provider_id:expr,
        default_base_url = $base_url:expr,
        extra_headers = $extra:expr,
        doc = $doc:literal
    ) => {
        #[doc = $doc]
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name {
            inner: $crate::providers::shared::OpenAiCompatAdapter,
        }

        impl $name {
            #[doc = concat!("Construct a new ", stringify!($name), ".")]
            pub fn new() -> Self {
                const SPEC: $crate::providers::shared::OpenAiCompatSpec =
                    $crate::providers::shared::OpenAiCompatSpec {
                        provider_id: $provider_id,
                        default_base_url: $base_url,
                        extra_headers: $extra,
                    };
                Self {
                    inner: $crate::providers::shared::OpenAiCompatAdapter::new(SPEC),
                }
            }

            /// Build the JSON request body.
            pub fn build_body(
                &self,
                req: &$crate::adapter::ProviderRequest,
            ) -> ::serde_json::Value {
                self.inner.build_body(req)
            }

            /// Build the request headers.
            pub fn build_headers(
                &self,
                req: &$crate::adapter::ProviderRequest,
            ) -> $crate::error::ProviderResult<::reqwest::header::HeaderMap> {
                self.inner.build_headers(req)
            }

            /// Map a single SSE frame using the OpenAI parser.
            pub fn map_frame(
                frame: &$crate::stream::SseFrame,
            ) -> $crate::error::ProviderResult<Vec<$crate::stream::ProviderEvent>> {
                $crate::providers::openai::OpenAiAdapter::map_frame(frame)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        #[::async_trait::async_trait]
        impl $crate::adapter::ProviderAdapter for $name {
            async fn stream(
                &self,
                req: $crate::adapter::ProviderRequest,
                abort: ::tokio_util::sync::CancellationToken,
            ) -> $crate::error::ProviderResult<$crate::adapter::ProviderStream> {
                self.inner.boxed_stream(req, abort).await
            }

            fn capabilities(&self) -> $crate::stream::ProviderCapabilities {
                $crate::providers::shared::openai_compat_capabilities()
            }
        }
    };
}

/// No-op extra-header hook for OpenAI-compatible providers without metadata.
pub(crate) fn no_extra_headers(
    _req: &ProviderRequest,
    _headers: &mut HeaderMap,
) -> ProviderResult<()> {
    Ok(())
}

/// Test-only fixture builder shared by every provider adapter's unit suite.
/// Lets each adapter override the differentiating fields (model id, api id,
/// credential, base url) without duplicating the 18-line `ProviderRequest`
/// boilerplate in every test module.
#[cfg(test)]
pub(crate) fn test_request_with(
    model: &str,
    api_model_id: &str,
    credential: crate::adapter::ProviderCredential,
    base_url: Option<String>,
    temperature: Option<f64>,
) -> crate::adapter::ProviderRequest {
    crate::adapter::ProviderRequest {
        model: model.into(),
        api_model_id: api_model_id.into(),
        session_id: "sess-1".into(),
        system: vec!["sys".into()],
        messages: vec![json!({ "role": "user", "content": "hi" })],
        tools: vec![],
        tool_choice: None,
        options: serde_json::Map::new(),
        headers: Default::default(),
        max_output_tokens: 4096,
        temperature,
        top_p: None,
        top_k: None,
        credential: Some(credential),
        base_url,
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
