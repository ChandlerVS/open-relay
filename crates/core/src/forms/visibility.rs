//! Evaluating a layout's conditional-visibility rules.
//!
//! This is the reference implementation of a spec with two copies — the other
//! is `packages/form-renderer/src/visibility.ts`, which the embed SDK runs on
//! third-party host pages. **They must agree exactly**, or a visitor sees one
//! form and the server validates a different one. Any change here is a change
//! there.
//!
//! # The spec
//!
//! A value is canonicalised to a string before any comparison:
//!
//! | JSON            | canonical            |
//! |-----------------|----------------------|
//! | `String(s)`     | `s.trim()`           |
//! | `Bool(b)`       | `"true"` / `"false"` |
//! | `Number(n)`     | `n.to_string()`      |
//! | `Null`/absent   | `""`                 |
//!
//! `equals`/`not_equals`/`contains` compare ASCII-case-insensitively: a
//! select's options are exact, but free-text controllers aren't, and MySQL's
//! own collation is case-insensitive too, so this is the least surprising rule.
//! `is_checked` is "the canonical string is truthy", reusing the same vocabulary
//! [`crate::submissions::service::coerce_custom`] accepts (`true`/`on`/`yes`/`1`)
//! so a non-renderer client posting `"on"` can't be read as unchecked here and
//! as `true` there. `is_not_checked` is its negation — deliberately *not*
//! "equals false", because an unanswered checkbox is absent, never `false`.
//!
//! Evaluation is a **single forward pass**. `service::validate_layout`
//! guarantees a condition only ever names a field appearing strictly earlier,
//! so by the time an element is reached every controller it mentions already
//! has a verdict.
//!
//! 1. An element with no rule is visible.
//! 2. A condition naming a controller that is *itself hidden* is **false**.
//!    Transitive hiding falls out of this — under `all` one hidden controller
//!    hides the dependent, under `any` it merely contributes nothing. This is
//!    also what stops a hidden field's `default_value` (which the renderer
//!    prefills for every element, visible or not) from steering a later one.
//! 3. `all` needs every condition true; `any` needs at least one.
//! 4. `Divider` and `PageBreak` are always visible.

use std::collections::{HashMap, HashSet};

use serde_json::Value as JsonValue;

use super::{Condition, ConditionOp, FormElement, MatchMode, VisibilityRule};

/// The payload shape submissions arrive in — matches
/// `crate::submissions::NewSubmissionPayload`'s inner map.
pub type ValueMap = HashMap<String, JsonValue>;

/// Canonical string for a submitted value. See the module docs.
pub fn canonical(value: Option<&JsonValue>) -> String {
    match value {
        Some(JsonValue::String(s)) => s.trim().to_string(),
        Some(JsonValue::Bool(b)) => b.to_string(),
        Some(JsonValue::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Whether a canonical string reads as a ticked checkbox. Matches the
/// vocabulary `submissions::service::coerce_custom` coerces to `true`.
fn is_truthy(canonical: &str) -> bool {
    matches!(
        canonical.to_ascii_lowercase().as_str(),
        "true" | "on" | "yes" | "1"
    )
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Evaluate one condition. `visible` holds the verdicts for every field key
/// already passed; a controller missing from it, or present and hidden, makes
/// the condition false.
fn eval_condition(cond: &Condition, values: &ValueMap, visible: &HashMap<String, bool>) -> bool {
    if !visible.get(&cond.field).copied().unwrap_or(false) {
        return false;
    }
    let actual = canonical(values.get(&cond.field));
    let operand = cond.value.as_deref().unwrap_or_default();
    match cond.op {
        ConditionOp::Equals => eq_ignore_case(&actual, operand),
        ConditionOp::NotEquals => !eq_ignore_case(&actual, operand),
        ConditionOp::Contains => actual
            .to_ascii_lowercase()
            .contains(&operand.to_ascii_lowercase()),
        ConditionOp::IsEmpty => actual.is_empty(),
        ConditionOp::IsNotEmpty => !actual.is_empty(),
        ConditionOp::IsChecked => is_truthy(&actual),
        ConditionOp::IsNotChecked => !is_truthy(&actual),
    }
}

fn eval_rule(rule: &VisibilityRule, values: &ValueMap, visible: &HashMap<String, bool>) -> bool {
    match rule.match_mode {
        MatchMode::All => rule.conditions.iter().all(|c| eval_condition(c, values, visible)),
        MatchMode::Any => rule.conditions.iter().any(|c| eval_condition(c, values, visible)),
    }
}

/// Per-element visibility, parallel to `layout`.
pub fn element_visibility(layout: &[FormElement], values: &ValueMap) -> Vec<bool> {
    let mut visible_keys: HashMap<String, bool> = HashMap::new();
    let mut out = Vec::with_capacity(layout.len());
    for el in layout {
        let visible = match el.visible_when() {
            Some(rule) => eval_rule(rule, values, &visible_keys),
            None => true,
        };
        if let Some(key) = el.field_key() {
            visible_keys.insert(key.to_string(), visible);
        }
        out.push(visible);
    }
    out
}

/// Submission keys the rules say should not have been answered.
///
/// The server drops these from a payload rather than merely exempting them from
/// `required`: a visitor who fills a block and then flips the controlling answer
/// would otherwise deliver contradictory data to a backend, and an embed bundle
/// cached before rules existed submits every field unconditionally.
pub fn hidden_field_keys(layout: &[FormElement], values: &ValueMap) -> HashSet<String> {
    let visibility = element_visibility(layout, values);
    layout
        .iter()
        .zip(visibility)
        .filter(|(_, visible)| !visible)
        .filter_map(|(el, _)| el.field_key().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::{CustomField, CustomFieldType, FieldWidth, StandardElement};

    fn values(pairs: &[(&str, JsonValue)]) -> ValueMap {
        pairs.iter().map(|(k, v)| ((*k).to_string(), v.clone())).collect()
    }

    fn rule(mode: MatchMode, conds: &[(&str, ConditionOp, Option<&str>)]) -> VisibilityRule {
        VisibilityRule {
            match_mode: mode,
            conditions: conds
                .iter()
                .map(|(f, op, v)| Condition {
                    field: (*f).to_string(),
                    op: *op,
                    value: v.map(str::to_string),
                })
                .collect(),
        }
    }

    fn custom(key: &str, kind: CustomFieldType, when: Option<VisibilityRule>) -> FormElement {
        FormElement::Custom(CustomField {
            key: key.into(),
            label: key.into(),
            kind,
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
            width: FieldWidth::Full,
            default_value: None,
            visible_when: when,
        })
    }

    fn standard(key: &str, when: Option<VisibilityRule>) -> FormElement {
        FormElement::Standard(StandardElement {
            visible_when: when,
            ..StandardElement::from_legacy(key, &crate::forms::StandardFieldConfig::default_enabled())
        })
    }

    #[test]
    fn an_unruled_element_is_always_visible() {
        let layout = vec![custom("a", CustomFieldType::Text, None)];
        assert_eq!(element_visibility(&layout, &values(&[])), vec![true]);
    }

    #[test]
    fn equals_is_case_insensitive_and_trims() {
        let layout = vec![
            custom("same", CustomFieldType::Text, None),
            standard("city", Some(rule(MatchMode::All, &[("same", ConditionOp::Equals, Some("No"))]))),
        ];
        let v = values(&[("same", JsonValue::String("  no ".into()))]);
        assert_eq!(element_visibility(&layout, &v), vec![true, true]);
    }

    #[test]
    fn all_needs_every_condition_any_needs_one() {
        let base = |mode| {
            vec![
                custom("a", CustomFieldType::Text, None),
                custom("b", CustomFieldType::Text, None),
                standard(
                    "city",
                    Some(rule(
                        mode,
                        &[
                            ("a", ConditionOp::Equals, Some("x")),
                            ("b", ConditionOp::Equals, Some("y")),
                        ],
                    )),
                ),
            ]
        };
        let v = values(&[("a", JsonValue::String("x".into())), ("b", JsonValue::String("nope".into()))]);
        assert!(!element_visibility(&base(MatchMode::All), &v)[2]);
        assert!(element_visibility(&base(MatchMode::Any), &v)[2]);
    }

    #[test]
    fn hiding_a_controller_hides_what_depends_on_it() {
        // c depends on b, b depends on a. `a` says no, so b hides — and c must
        // hide with it even though c's own test would otherwise pass on b's
        // stale value.
        let layout = vec![
            custom("a", CustomFieldType::Text, None),
            custom(
                "b",
                CustomFieldType::Text,
                Some(rule(MatchMode::All, &[("a", ConditionOp::Equals, Some("yes"))])),
            ),
            custom(
                "c",
                CustomFieldType::Text,
                Some(rule(MatchMode::All, &[("b", ConditionOp::IsEmpty, None)])),
            ),
        ];
        let v = values(&[
            ("a", JsonValue::String("no".into())),
            ("b", JsonValue::String("".into())),
        ]);
        assert_eq!(element_visibility(&layout, &v), vec![true, false, false]);
    }

    #[test]
    fn a_hidden_fields_default_cannot_steer_a_later_one() {
        // `b` is hidden but prefilled from its default_value; `c` keys off it.
        let layout = vec![
            custom("a", CustomFieldType::Text, None),
            custom(
                "b",
                CustomFieldType::Text,
                Some(rule(MatchMode::All, &[("a", ConditionOp::Equals, Some("yes"))])),
            ),
            custom(
                "c",
                CustomFieldType::Text,
                Some(rule(MatchMode::All, &[("b", ConditionOp::Equals, Some("seed"))])),
            ),
        ];
        let v = values(&[
            ("a", JsonValue::String("no".into())),
            ("b", JsonValue::String("seed".into())),
        ]);
        assert!(!element_visibility(&layout, &v)[2]);
    }

    #[test]
    fn checkbox_truthiness_matches_the_submission_coercion_vocabulary() {
        let layout = |op| {
            vec![
                custom("agree", CustomFieldType::Checkbox, None),
                standard("city", Some(rule(MatchMode::All, &[("agree", op, None)]))),
            ]
        };
        for raw in [JsonValue::Bool(true), JsonValue::String("on".into()), JsonValue::String("1".into())] {
            let v = values(&[("agree", raw.clone())]);
            assert!(element_visibility(&layout(ConditionOp::IsChecked), &v)[1], "{raw:?}");
            assert!(!element_visibility(&layout(ConditionOp::IsNotChecked), &v)[1], "{raw:?}");
        }
        // An unanswered checkbox is absent, never `false`.
        let empty = values(&[]);
        assert!(!element_visibility(&layout(ConditionOp::IsChecked), &empty)[1]);
        assert!(element_visibility(&layout(ConditionOp::IsNotChecked), &empty)[1]);
    }

    #[test]
    fn hidden_field_keys_reports_only_field_elements() {
        let layout = vec![
            custom("same", CustomFieldType::Text, None),
            FormElement::Heading(crate::forms::HeadingElement {
                text: "Billing".into(),
                level: 2,
                visible_when: Some(rule(MatchMode::All, &[("same", ConditionOp::Equals, Some("no"))])),
            }),
            standard("city", Some(rule(MatchMode::All, &[("same", ConditionOp::Equals, Some("no"))]))),
        ];
        let v = values(&[("same", JsonValue::String("yes".into()))]);
        let hidden = hidden_field_keys(&layout, &v);
        assert_eq!(hidden, HashSet::from(["city".to_string()]));
    }

    #[test]
    fn numbers_canonicalise_without_quotes() {
        assert_eq!(canonical(Some(&JsonValue::from(42))), "42");
        assert_eq!(canonical(None), "");
    }
}
