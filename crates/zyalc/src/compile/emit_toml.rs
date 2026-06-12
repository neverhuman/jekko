use anyhow::{anyhow, Context, Result};

pub(crate) fn render(raw: &str) -> Result<String> {
    let body = super::toml_source_body(raw);
    let parsed: serde_yaml::Value =
        serde_yaml::from_str(&body).context("parse declarative YAML body")?;
    let toml_value = yaml_to_toml(parsed)?;
    let rendered = toml::to_string_pretty(&toml_value).context("render TOML")?;
    Ok(rendered)
}

/// Convert a YAML mapping into a TOML value. The declarative schema uses
/// `lanes: [...]` at the top level; TOML's natural array-of-tables form is
/// `[[lane]]`, so we rename the key during translation.
fn yaml_to_toml(value: serde_yaml::Value) -> Result<toml::Value> {
    use serde_yaml::Value as Y;
    let map = match value {
        Y::Mapping(m) => m,
        _ => return Err(anyhow!("declarative body must be a YAML mapping")),
    };
    let sandbox = match map.get(Y::String("sandbox".to_string())) {
        Some(Y::Mapping(sandbox)) => Some(sandbox.clone()),
        Some(_) => return Err(anyhow!("sandbox must be a mapping")),
        None => None,
    };
    let job = match map.get(Y::String("job".to_string())) {
        Some(Y::Mapping(job)) => Some(job.clone()),
        Some(_) => return Err(anyhow!("job must be a mapping")),
        None => None,
    };
    let dispatch = match map.get(Y::String("dispatch".to_string())) {
        Some(Y::Mapping(dispatch)) => Some(dispatch.clone()),
        Some(_) => return Err(anyhow!("dispatch must be a mapping")),
        None => None,
    };
    let sandbox_workers = match sandbox
        .as_ref()
        .and_then(|nested| nested.get(Y::String("workers".to_string())))
    {
        Some(Y::Mapping(workers)) => Some(workers.clone()),
        Some(_) => return Err(anyhow!("sandbox.workers must be a mapping")),
        None => None,
    };
    let has_lane_contract = map.contains_key(Y::String("lanes".to_string()))
        || map.contains_key(Y::String("schema_version".to_string()))
        || map.contains_key(Y::String("sandbox_root".to_string()))
        || job
            .as_ref()
            .is_some_and(|nested| nested.contains_key(Y::String("lanes".to_string())))
        || dispatch
            .as_ref()
            .is_some_and(|nested| nested.contains_key(Y::String("lanes".to_string())))
        || sandbox
            .as_ref()
            .is_some_and(|nested| nested.contains_key(Y::String("lanes".to_string())));
    if has_lane_contract {
        let mut tbl = toml::value::Table::new();
        let schema_version = match map.get(Y::String("schema_version".to_string())) {
            Some(value) => yaml_value_to_toml(value.clone())?,
            None => match sandbox
                .as_ref()
                .and_then(|nested| nested.get(Y::String("schema_version".to_string())))
            {
                Some(value) => yaml_value_to_toml(value.clone())?,
                None => return Err(anyhow!("schema_version is required")),
            },
        };
        tbl.insert("schema_version".into(), schema_version);
        if let Some(sandbox_root) = map.get(Y::String("sandbox_root".to_string())) {
            tbl.insert(
                "sandbox_root".into(),
                yaml_value_to_toml(sandbox_root.clone())?,
            );
        } else if let Some(sandbox_root) = sandbox
            .as_ref()
            .and_then(|nested| nested.get(Y::String("sandbox_root".to_string())))
        {
            tbl.insert(
                "sandbox_root".into(),
                yaml_value_to_toml(sandbox_root.clone())?,
            );
        } else if let Some(sandbox_root) = sandbox_workers
            .as_ref()
            .and_then(|nested| nested.get(Y::String("root".to_string())))
        {
            tbl.insert(
                "sandbox_root".into(),
                yaml_value_to_toml(sandbox_root.clone())?,
            );
        }
        let lanes = if let Some(value) = map.get(Y::String("lanes".to_string())) {
            match value {
                Y::Sequence(arr) => arr.clone(),
                _ => return Err(anyhow!("lanes must be a sequence")),
            }
        } else if let Some(value) = job
            .as_ref()
            .and_then(|nested| nested.get(Y::String("lanes".to_string())))
        {
            match value {
                Y::Sequence(arr) => arr.clone(),
                _ => return Err(anyhow!("lanes must be a sequence")),
            }
        } else if let Some(value) = dispatch
            .as_ref()
            .and_then(|nested| nested.get(Y::String("lanes".to_string())))
        {
            match value {
                Y::Sequence(arr) => arr.clone(),
                _ => return Err(anyhow!("lanes must be a sequence")),
            }
        } else if let Some(value) = sandbox
            .as_ref()
            .and_then(|nested| nested.get(Y::String("lanes".to_string())))
        {
            match value {
                Y::Sequence(arr) => arr.clone(),
                _ => return Err(anyhow!("lanes must be a sequence")),
            }
        } else {
            return Err(anyhow!("lanes are required"));
        };
        let mut arr = Vec::with_capacity(lanes.len());
        for item in lanes {
            arr.push(yaml_value_to_toml(item)?);
        }
        tbl.insert("lane".into(), toml::Value::Array(arr));
        return Ok(toml::Value::Table(tbl));
    }
    let map = match sandbox {
        Some(sandbox) => sandbox,
        None => map,
    };
    let mut tbl = toml::value::Table::new();
    for (k, v) in map {
        let key = match k.as_str() {
            Some(s) => s.to_string(),
            None => return Err(anyhow!("non-string key")),
        };
        if key == "lanes" {
            let array = match v.as_sequence() {
                Some(arr) => arr.clone(),
                None => return Err(anyhow!("lanes must be a sequence")),
            };
            let mut arr = Vec::with_capacity(array.len());
            for item in array {
                arr.push(yaml_value_to_toml(item)?);
            }
            tbl.insert("lane".into(), toml::Value::Array(arr));
        } else {
            tbl.insert(key, yaml_value_to_toml(v)?);
        }
    }
    Ok(toml::Value::Table(tbl))
}

fn yaml_value_to_toml(v: serde_yaml::Value) -> Result<toml::Value> {
    use serde_yaml::Value as Y;
    Ok(match v {
        Y::Null => toml::Value::String(String::new()),
        Y::Bool(b) => toml::Value::Boolean(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(u) = n.as_u64() {
                toml::Value::Integer(u as i64)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        Y::String(s) => toml::Value::String(s),
        Y::Sequence(seq) => {
            let mut arr = Vec::with_capacity(seq.len());
            for item in seq {
                arr.push(yaml_value_to_toml(item)?);
            }
            toml::Value::Array(arr)
        }
        Y::Mapping(m) => {
            let mut tbl = toml::value::Table::new();
            for (k, v) in m {
                let key = match k.as_str() {
                    Some(s) => s.to_string(),
                    None => return Err(anyhow!("non-string key in mapping")),
                };
                tbl.insert(key, yaml_value_to_toml(v)?);
            }
            toml::Value::Table(tbl)
        }
        Y::Tagged(t) => yaml_value_to_toml(t.value)?,
    })
}
