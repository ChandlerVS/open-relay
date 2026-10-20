//! Plain-text rendering of a submission's answers, shared by the backends that
//! hand a person a readable summary (the notification email, a GoHighLevel
//! contact note).
//!
//! Order: standard fields in catalogue order, then custom fields in layout
//! order under their labels, then anything else the payload carries (a field
//! since removed from the form), sorted by key.

use std::collections::HashSet;

use serde_json::Value;

use crate::forms::STANDARD_FIELD_KEYS;

/// Cap on one rendered answer, so a 10 000-character message field can't
/// turn a summary into a wall of text. The full value is in the admin.
pub const MAX_VALUE_CHARS: usize = 2_000;

/// One `label: value` line per answer, newline-terminated, or `(no answers)`.
/// `labels` maps a custom field key to its label, in layout order; a blank
/// label falls back to the key.
pub fn render_answers(data: &Value, labels: &[(String, String)]) -> String {
    let data = data.as_object();
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines: Vec<(String, String)> = Vec::new();
    let mut push = |label: String, key: &str| {
        let Some(value) = data.and_then(|d| d.get(key)) else {
            return;
        };
        if !seen.insert(key.to_string()) {
            return;
        }
        if let Some(text) = render_value(value) {
            lines.push((label, text));
        }
    };
    for key in STANDARD_FIELD_KEYS {
        push(humanize(key), key);
    }
    for (key, label) in labels {
        let label = match label.trim() {
            "" => key.clone(),
            l => l.to_string(),
        };
        push(label, key);
    }
    if let Some(d) = data {
        let mut rest: Vec<&String> = d.keys().collect();
        rest.sort();
        for key in rest {
            push(key.clone(), key);
        }
    }

    if lines.is_empty() {
        return "(no answers)\n".to_string();
    }
    let mut out = String::new();
    for (label, value) in lines {
        if value.contains('\n') {
            // Indent continuation lines so a multi-line answer can't pass
            // itself off as another field.
            out.push_str(&format!("{label}:\n"));
            for line in value.lines() {
                out.push_str(&format!("    {line}\n"));
            }
        } else {
            out.push_str(&format!("{label}: {value}\n"));
        }
    }
    out
}

/// One answer as text; `None` for an empty one.
fn render_value(value: &Value) -> Option<String> {
    let text = match value {
        Value::Null => return None,
        Value::String(s) => s.clone(),
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        Value::Number(n) => n.to_string(),
        // Checkbox groups: the ticked options.
        Value::Array(items) => items
            .iter()
            .filter_map(render_value)
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(_) => value.to_string(),
    };
    // Control characters other than newline/tab have no business in a
    // plain-text body (a bare CR could overwrite a line in some clients).
    let text: String = text
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    Some(if text.chars().count() > MAX_VALUE_CHARS {
        let mut cut: String = text.chars().take(MAX_VALUE_CHARS).collect();
        cut.push('…');
        cut
    } else {
        text.to_string()
    })
}

/// `address_line_1` → `Address line 1`.
fn humanize(key: &str) -> String {
    let spaced = key.replace('_', " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multi_line_answers_are_indented_and_stripped_of_control_chars() {
        let out = render_answers(&json!({ "message": "line one\r\nline two\u{7}" }), &[]);
        assert_eq!(out, "Message:\n    line one\n    line two\n");
    }

    #[test]
    fn long_answers_are_truncated() {
        let out = render_answers(&json!({ "message": "x".repeat(MAX_VALUE_CHARS + 50) }), &[]);
        assert!(out.contains('…'));
        assert!(!out.contains(&"x".repeat(MAX_VALUE_CHARS + 1)));
    }

    #[test]
    fn empty_submission_says_so() {
        assert_eq!(render_answers(&json!({}), &[]), "(no answers)\n");
    }

    #[test]
    fn humanize_capitalises_first_word_only() {
        assert_eq!(humanize("address_line_1"), "Address line 1");
        assert_eq!(humanize("email"), "Email");
    }
}
