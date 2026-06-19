use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde_json::json;
use syn::visit::Visit;

use super::builder::GraphBuilder;

pub(super) fn add_import_edges(
    repo_root: &Path,
    rel: &Path,
    builder: &mut GraphBuilder,
    module_id: &str,
) -> Result<()> {
    let text = fs::read_to_string(repo_root.join(rel)).unwrap_or_default();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("use ") {
            let key = rest
                .trim_end_matches(';')
                .split_whitespace()
                .next()
                .unwrap_or(rest);
            let import_id = builder.node("module", key, key);
            builder.edge(module_id, &import_id, "imports");
        }
        if let Some(rest) = trimmed.strip_prefix("mod ") {
            let key = rest.trim_end_matches(';').trim();
            let import_id = builder.node("module", key, key);
            builder.edge(module_id, &import_id, "imports");
        }
    }
    Ok(())
}

pub(super) fn add_rust_symbol_edges(
    repo_root: &Path,
    rel: &Path,
    key: &str,
    builder: &mut GraphBuilder,
    module_id: &str,
) -> Result<()> {
    let text = fs::read_to_string(repo_root.join(rel)).unwrap_or_default();
    let Ok(file) = syn::parse_file(&text) else {
        return Ok(());
    };
    for item in file.items {
        match item {
            syn::Item::Fn(item_fn) => add_function(builder, key, module_id, item_fn),
            syn::Item::Struct(item_struct) => {
                let name = item_struct.ident.to_string();
                let struct_id = builder.node_with_payload(
                    "struct",
                    &format!("{key}::{name}"),
                    &name,
                    json!({"file": key, "visibility": visibility_label(&item_struct.vis)}),
                );
                builder.edge(module_id, &struct_id, "contains");
            }
            syn::Item::Enum(item_enum) => {
                let name = item_enum.ident.to_string();
                let enum_id = builder.node_with_payload(
                    "enum",
                    &format!("{key}::{name}"),
                    &name,
                    json!({"file": key, "visibility": visibility_label(&item_enum.vis)}),
                );
                builder.edge(module_id, &enum_id, "contains");
            }
            syn::Item::Impl(item_impl) => add_impl(builder, key, module_id, item_impl),
            _ => {}
        }
    }
    Ok(())
}

fn add_function(builder: &mut GraphBuilder, key: &str, module_id: &str, item_fn: syn::ItemFn) {
    let name = item_fn.sig.ident.to_string();
    let fn_id = builder.node_with_payload(
        "function",
        &format!("{key}::{name}"),
        &name,
        json!({"file": key, "visibility": visibility_label(&item_fn.vis)}),
    );
    builder.edge(module_id, &fn_id, "contains");
    add_call_edges(builder, &fn_id, &item_fn.block);
}

fn add_impl(builder: &mut GraphBuilder, key: &str, module_id: &str, item_impl: syn::ItemImpl) {
    let impl_name = impl_label(&item_impl.self_ty);
    let impl_key = format!("{key}::impl::{impl_name}");
    let impl_id = builder.node_with_payload("impl", &impl_key, &impl_name, json!({"file": key}));
    builder.edge(module_id, &impl_id, "contains");
    for item in item_impl.items {
        if let syn::ImplItem::Fn(method) = item {
            let name = method.sig.ident.to_string();
            let method_id = builder.node_with_payload(
                "method",
                &format!("{impl_key}::{name}"),
                &name,
                json!({"file": key, "impl": impl_name}),
            );
            builder.edge(&impl_id, &method_id, "contains");
            add_call_edges(builder, &method_id, &method.block);
        }
    }
}

fn add_call_edges(builder: &mut GraphBuilder, caller_id: &str, block: &syn::Block) {
    let mut visitor = CallVisitor::default();
    visitor.visit_block(block);
    for call in visitor.calls {
        let callee_id = builder.node_with_payload(
            "function",
            &format!("symbol::{call}"),
            &call,
            json!({"approximate": true}),
        );
        builder.edge_with_payload(caller_id, &callee_id, "calls", json!({"approximate": true}));
    }
}

#[derive(Default)]
struct CallVisitor {
    calls: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for CallVisitor {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = node.func.as_ref() {
            if let Some(segment) = path.path.segments.last() {
                self.calls.insert(segment.ident.to_string());
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.calls.insert(node.method.to_string());
        syn::visit::visit_expr_method_call(self, node);
    }
}

fn visibility_label(vis: &syn::Visibility) -> &'static str {
    match vis {
        syn::Visibility::Public(_) => "public",
        _ => "private",
    }
}

fn impl_label(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        _ => "unknown".to_string(),
    }
}
