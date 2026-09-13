//! JSON Schema for the form wire types, for callers that have to *construct* a
//! layout rather than just deserialize one.
//!
//! # Why this exists rather than a second set of derives
//!
//! The MCP tool surface has to publish an input schema for every tool, and an
//! agent building a `layout` needs the real shape of [`super::FormElement`] —
//! nine adjacently-tagged variants, each with its own body, several of them
//! carrying a `VisibilityRule`. Describing that by hand, or deriving it a second
//! time from a different schema crate, would create two descriptions of one type
//! that drift apart silently. Everything else in this codebase that needs one
//! shape in two places generates both from a single source (`gen-regions.mjs`
//! being the clearest case), and this follows that stance: the `utoipa::ToSchema`
//! derives already on these types are the source, and both the OpenAPI document
//! and the MCP tool schemas are rendered from them.
//!
//! [`components`] returns the schemas *and every type they reference*, so the
//! result is self-contained — `ToSchema::schemas` walks transitively, which is
//! what lets a consumer inline the whole set under `$defs` and resolve every
//! `$ref` locally.

use utoipa::{PartialSchema, ToSchema};
use utoipa::openapi::{Components, ComponentsBuilder, RefOr, Schema};

use crate::forms::{
    CustomField, FormElement, NewForm, PostSubmissionAction, ProgressIndicator,
    StandardFieldsConfig, UpdateForm,
};

/// The schema names a consumer is most likely to want to point a property at.
/// Exposed so a caller naming one gets a compile error if it is ever renamed,
/// rather than a `$ref` that silently resolves to nothing.
pub const FORM_ELEMENT: &str = "FormElement";
pub const CUSTOM_FIELD: &str = "CustomField";
pub const STANDARD_FIELDS_CONFIG: &str = "StandardFieldsConfig";
pub const POST_SUBMISSION_ACTION: &str = "PostSubmissionAction";
pub const PROGRESS_INDICATOR: &str = "ProgressIndicator";
pub const NEW_FORM: &str = "NewForm";
pub const UPDATE_FORM: &str = "UpdateForm";

/// Every form wire type, transitively closed over its references.
///
/// The `$ref`s inside point at `#/components/schemas/...`, the OpenAPI
/// convention. A consumer targeting plain JSON Schema is expected to rehome them
/// — see `open_relay_mcp::schema`.
pub fn components() -> Components {
    let mut schemas: Vec<(String, RefOr<Schema>)> = Vec::new();

    // `schemas` pulls in dependencies recursively, so naming the roots is
    // enough; the overlap between them is deduped by the builder.
    FormElement::schemas(&mut schemas);
    CustomField::schemas(&mut schemas);
    StandardFieldsConfig::schemas(&mut schemas);
    PostSubmissionAction::schemas(&mut schemas);
    ProgressIndicator::schemas(&mut schemas);
    NewForm::schemas(&mut schemas);
    UpdateForm::schemas(&mut schemas);

    // A root type is only added to `schemas` when something *references* it, so
    // the roots themselves have to be named explicitly or a `$ref` to
    // `FormElement` would dangle.
    schemas.push((FORM_ELEMENT.into(), FormElement::schema()));
    schemas.push((CUSTOM_FIELD.into(), CustomField::schema()));
    schemas.push((STANDARD_FIELDS_CONFIG.into(), StandardFieldsConfig::schema()));
    schemas.push((POST_SUBMISSION_ACTION.into(), PostSubmissionAction::schema()));
    schemas.push((PROGRESS_INDICATOR.into(), ProgressIndicator::schema()));
    schemas.push((NEW_FORM.into(), NewForm::schema()));
    schemas.push((UPDATE_FORM.into(), UpdateForm::schema()));

    ComponentsBuilder::new().schemas_from_iter(schemas).build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_json() -> serde_json::Value {
        serde_json::to_value(components()).expect("serialize components")
    }

    /// Collect every `$ref` target name appearing anywhere in the document.
    fn refs(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    if k == "$ref" {
                        if let Some(s) = v.as_str() {
                            out.push(s.to_string());
                        }
                    }
                    refs(v, out);
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| refs(v, out)),
            _ => {}
        }
    }

    #[test]
    fn the_roots_are_all_present() {
        let doc = as_json();
        let schemas = doc.get("schemas").expect("schemas key");
        for name in [
            FORM_ELEMENT,
            CUSTOM_FIELD,
            STANDARD_FIELDS_CONFIG,
            POST_SUBMISSION_ACTION,
            PROGRESS_INDICATOR,
            NEW_FORM,
            UPDATE_FORM,
        ] {
            assert!(schemas.get(name).is_some(), "missing root schema {name}");
        }
    }

    /// The property the MCP crate depends on: the set is self-contained, so a
    /// consumer can inline it wholesale and every `$ref` still resolves. If a
    /// new nested type is ever added to a form element and `schemas()` stops
    /// reaching it, this fails here rather than as an unresolvable ref in an
    /// agent's tool schema.
    #[test]
    fn every_ref_resolves_within_the_set() {
        let doc = as_json();
        let schemas = doc.get("schemas").expect("schemas key");
        let mut found = Vec::new();
        refs(&doc, &mut found);
        assert!(!found.is_empty(), "expected the set to contain $refs");
        for r in found {
            let name = r
                .rsplit('/')
                .next()
                .expect("a $ref always has a trailing segment");
            assert!(
                schemas.get(name).is_some(),
                "dangling $ref {r:?}: nothing named {name} in the component set"
            );
        }
    }

    /// The nine `FormElement` variants are the discovery surface an agent reads
    /// to learn what it may build, so a new variant must not be able to slip in
    /// without being described.
    #[test]
    fn form_element_describes_every_variant() {
        let doc = as_json();
        let element = doc
            .pointer("/schemas/FormElement/oneOf")
            .and_then(|v| v.as_array())
            .expect("FormElement is a tagged union");
        let tags: Vec<&str> = element
            .iter()
            .filter_map(|v| v.pointer("/properties/element/enum/0"))
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            tags,
            [
                "standard",
                "custom",
                "heading",
                "paragraph",
                "rich_text",
                "divider",
                "page_break",
                "row_start",
                "row_end",
            ],
            "FormElement variants changed; update the MCP instructions too"
        );
    }
}
