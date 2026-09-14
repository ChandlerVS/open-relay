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
        let mut defs = value
            .get_mut("schemas")
            .and_then(Value::as_object_mut)
            .map(std::mem::take)
            .unwrap_or_default();
        annotate(&mut defs);
        defs
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

/// Teach the schema the things a Rust type cannot say for itself.
///
/// `StandardElement::key` is a `String` in Rust, constrained only by
/// `validate_layout` at write time, and its doc comment renders into the schema
/// as the *rustdoc link* "One of [`STANDARD_FIELD_KEYS`]" — which reads like
/// documentation and conveys nothing to a client that cannot see Rust source.
/// An agent reading it would be guessing at the key names.
///
/// So the catalogue is injected as a JSON Schema `enum`, taken from
/// `STANDARD_FIELD_KEYS` itself — the list the `declare_standard_fields!` macro
/// generates, so this is reading the single source of truth rather than copying
/// it. Adding a standard field updates this automatically.
///
/// Putting it here rather than in a `list_available_fields` tool is deliberate:
/// the constraint appears on the parameter the agent is actually filling in,
/// and MCP clients validate arguments against the tool schema, so a wrong key
/// fails locally instead of costing a round trip to find out.
fn annotate(defs: &mut Map<String, Value>) {
    let keys: Vec<Value> = open_relay_core::forms::STANDARD_FIELD_KEYS
        .iter()
        .map(|k| Value::String((*k).to_string()))
        .collect();

    for path in ["StandardElement", "StandardFieldsConfig"] {
        let Some(props) = defs
            .get_mut(path)
            .and_then(|s| s.get_mut("properties"))
            .and_then(Value::as_object_mut)
        else {
            continue;
        };
        if let Some(key) = props.get_mut("key").and_then(Value::as_object_mut) {
            key.insert("enum".into(), Value::Array(keys.clone()));
            key.insert(
                "description".into(),
                Value::String(
                    "Which standard field this is. Each may appear at most once per form."
                        .into(),
                ),
            );
        }
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


#[cfg(test)]
mod catalogue_tests {
    use super::*;

    /// The gap this closes: `StandardElement::key` is a bare `String` in Rust,
    /// so without this the schema told an agent only "type: string" plus a
    /// rustdoc link it cannot follow. Every valid key must be named on the
    /// parameter the agent fills in.
    #[test]
    fn standard_field_keys_are_enumerated_in_the_schema() {
        let defs = all_definitions();
        let key = defs
            .pointer("/StandardElement/properties/key")
            .expect("StandardElement.key");

        let listed: Vec<&str> = key
            .get("enum")
            .and_then(|e| e.as_array())
            .expect("key must carry an enum")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();

        assert_eq!(
            listed,
            open_relay_core::forms::STANDARD_FIELD_KEYS,
            "the schema's key list must be STANDARD_FIELD_KEYS itself, not a copy"
        );

        // The rustdoc link must not survive into the wire schema.
        let description = key.get("description").and_then(|d| d.as_str()).unwrap_or("");
        assert!(
            !description.contains("STANDARD_FIELD_KEYS"),
            "the description still leaks a rustdoc reference: {description}"
        );
    }

    /// Custom field types were already discoverable — the tagged enum renders
    /// each variant with its `type` and its own config. Pinned so a refactor
    /// away from internal tagging cannot quietly erase it.
    #[test]
    fn custom_field_types_remain_discoverable() {
        let defs = all_definitions();
        let variants = defs
            .pointer("/CustomFieldType/oneOf")
            .and_then(|v| v.as_array())
            .expect("CustomFieldType is a tagged union");
        let types: Vec<&str> = variants
            .iter()
            .filter_map(|b| b.pointer("/properties/type/enum/0"))
            .filter_map(|t| t.as_str())
            .collect();
        for expected in [
            "text", "email", "number", "tel", "url", "textarea", "select", "radio", "checkbox",
            "checkboxes", "country", "state", "file", "rating",
        ] {
            assert!(types.contains(&expected), "custom type {expected} is not described");
        }
    }
}
