//! Tool input schemas, built from the same `utoipa` derives the REST API uses.
//!
//! # The problem this solves
//!
//! rmcp derives a tool's input schema from `schemars::JsonSchema`, but the form
//! wire types derive `utoipa::ToSchema` — that is what generates
//! `/openapi.json` and, from it, the TypeScript client. Deriving both would put
//! two descriptions of one type in the codebase, and the one nobody reads would
//! rot. So instead of a second derive, this module *rehomes* the schemas that
//! already exist: [`open_relay_core::forms::schema::components`] hands over the
//! transitively-closed set, and [`defs`] rewrites it from OpenAPI's
//! `#/components/schemas/X` into plain JSON Schema's `#/$defs/X`.
//!
//! The result is self-contained — core's `every_ref_resolves_within_the_set`
//! test is what guarantees that — so a tool schema can inline the whole set and
//! resolve every reference locally, with no `$ref` pointing at a document the
//! agent's client cannot fetch.
//!
//! `#[tool(input_schema = ...)]` accepts exactly this shape: it lowers to
//! `Tool::new_with_raw(name, description, expr)` where the expr is a
//! `serde_json::Map<String, Value>`.

use std::sync::{Arc, OnceLock};

use rmcp::model::JsonObject;
use serde_json::{Map, Value, json};

const OPENAPI_REF_PREFIX: &str = "#/components/schemas/";
const JSON_SCHEMA_REF_PREFIX: &str = "#/$defs/";

/// The form wire types as JSON Schema definitions, ready to drop under `$defs`.
///
/// Built once. The set is a few hundred KB of `serde_json` and every tool that
/// takes a layout embeds it, so paying for it per tool — let alone per call —
/// would be wasteful for a value that cannot change at runtime.
fn definitions() -> &'static Map<String, Value> {
    static DEFS: OnceLock<Map<String, Value>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let components = open_relay_core::forms::schema::components();
        let mut value = serde_json::to_value(&components)
            .expect("utoipa components always serialize to JSON");
        rehome_refs(&mut value);
        value
            .get_mut("schemas")
            .and_then(Value::as_object_mut)
            .map(std::mem::take)
            .unwrap_or_default()
    })
}

/// Rewrite every `$ref` from the OpenAPI document root to the JSON Schema one.
///
/// Deliberately a blind walk rather than a targeted edit: a `$ref` can appear
/// inside `oneOf`, `items`, `additionalProperties`, `properties`, or nested
/// arbitrarily deep in any of them, and `FormElement` alone uses three of those.
/// Missing one would leave a reference that resolves to nothing in the agent's
/// client — silently, since JSON Schema validators generally ignore what they
/// cannot resolve.
fn rehome_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "$ref" {
                    if let Some(target) = v.as_str().and_then(|s| s.strip_prefix(OPENAPI_REF_PREFIX))
                    {
                        *v = Value::String(format!("{JSON_SCHEMA_REF_PREFIX}{target}"));
                        continue;
                    }
                }
                rehome_refs(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rehome_refs),
        _ => {}
    }
}

/// A `$ref` to one of the form schemas by name.
///
/// Take the name from the `open_relay_core::forms::schema` constants rather
/// than spelling it, so a rename is a compile error instead of a dangling ref.
pub fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("{JSON_SCHEMA_REF_PREFIX}{name}") })
}

/// Build a tool input schema.
///
/// `properties` are the tool's own parameters; any that reference a form type
/// via [`schema_ref`] resolve against the `$defs` block this attaches.
pub fn object(properties: Vec<(&str, Value)>, required: &[&str]) -> Arc<JsonObject> {
    let mut props = Map::new();
    for (name, schema) in properties {
        props.insert(name.to_string(), schema);
    }
    let mut root = Map::new();
    root.insert("type".into(), json!("object"));
    root.insert("properties".into(), Value::Object(props));
    root.insert(
        "required".into(),
        Value::Array(required.iter().map(|r| json!(r)).collect()),
    );
    // Agents guess parameter names; an unexpected one should be rejected loudly
    // rather than silently dropped into a form write.
    root.insert("additionalProperties".into(), json!(false));
    // Only attach the definition set when something actually points into it.
    // Half the tools take nothing but scalars, and the set is large enough that
    // handing every one of them a copy would bloat `tools/list` for no benefit.
    if references_a_definition(&root) {
        root.insert("$defs".into(), Value::Object(definitions().clone()));
    }
    Arc::new(root)
}

/// Does any `$ref` in `value` point into `$defs`?
fn references_a_definition(map: &Map<String, Value>) -> bool {
    fn walk(value: &Value) -> bool {
        match value {
            Value::Object(m) => references_a_definition(m),
            Value::Array(items) => items.iter().any(walk),
            _ => false,
        }
    }
    map.iter().any(|(k, v)| {
        (k == "$ref" && v.as_str().is_some_and(|s| s.starts_with(JSON_SCHEMA_REF_PREFIX)))
            || walk(v)
    })
}

/// The whole definition set, for the `describe_form_schema` tool and the
/// matching MCP resource.
pub fn all_definitions() -> Value {
    Value::Object(definitions().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_relay_core::forms::schema as core_schema;

    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (k, v) in map {
                    if k == "$ref" {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                    collect_refs(v, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|v| collect_refs(v, out)),
            _ => {}
        }
    }

    #[test]
    fn no_openapi_refs_survive_the_rehome() {
        let defs = all_definitions();
        let mut refs = Vec::new();
        collect_refs(&defs, &mut refs);
        assert!(!refs.is_empty(), "expected the definition set to use $refs");
        for r in &refs {
            assert!(
                r.starts_with(JSON_SCHEMA_REF_PREFIX),
                "{r:?} still points at the OpenAPI document root"
            );
        }
    }

    /// The property every published tool schema depends on: an agent's client
    /// can resolve every reference without fetching anything.
    #[test]
    fn a_built_schema_resolves_every_ref_locally() {
        let schema = object(
            vec![
                ("form_id", json!({ "type": "integer" })),
                ("element", schema_ref(core_schema::FORM_ELEMENT)),
            ],
            &["form_id", "element"],
        );
        let value = Value::Object((*schema).clone());
        let defs = value.get("$defs").and_then(Value::as_object).expect("$defs");

        let mut refs = Vec::new();
        collect_refs(&value, &mut refs);
        assert!(refs.iter().any(|r| r.ends_with("/FormElement")));
        for r in refs {
            let name = r
                .strip_prefix(JSON_SCHEMA_REF_PREFIX)
                .unwrap_or_else(|| panic!("unexpected ref form {r:?}"));
            assert!(defs.contains_key(name), "dangling $ref {r:?}");
        }
    }

    #[test]
    fn the_root_is_a_closed_object() {
        let schema = object(vec![("id", json!({ "type": "integer" }))], &["id"]);
        assert_eq!(schema.get("type"), Some(&json!("object")));
        assert_eq!(schema.get("additionalProperties"), Some(&json!(false)));
        assert_eq!(schema.get("required"), Some(&json!(["id"])));
    }
}
