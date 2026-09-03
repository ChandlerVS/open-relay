import { STANDARD_FIELDS } from "@open-relay/form-renderer";
import {
  elementRule,
  isCountryField,
  type BuilderElement,
  type VisibilityRule,
} from "./model";

/** Errors keyed by BuilderElement.id, so they survive a reorder. */
export type LayoutErrors = Record<string, string>;

const MAX_KEY_LEN = 64;
const MAX_LABEL_LEN = 200;
const MAX_CONDITIONS = 10;
const VALUE_OPS = new Set(["equals", "not_equals", "contains"]);
const CHECKBOX_OPS = new Set(["is_checked", "is_not_checked"]);
const STANDARD_KEYS = new Set(STANDARD_FIELDS.map((f) => f.key));

/**
 * Mirrors `open_relay_core::forms::service::validate_layout` so the builder can
 * show problems inline instead of surfacing one opaque 400 on save. The server
 * remains the authority — this is a convenience, not a substitute.
 */
/**
 * Mirrors `service::validate_visible_when`. `seenFields` maps every field key
 * *already passed* to whether it is a checkbox — a condition may only name one
 * of those, which is what rules out unknown keys, self-references and cycles in
 * a single check.
 *
 * Returns `null` when the rule is fine. A field whose key is still blank (a
 * freshly added one) is skipped rather than reported, so the builder doesn't
 * shout at every new field before it has been named.
 */
function visibilityError(
  rule: VisibilityRule,
  seenFields: Map<string, boolean>,
): string | null {
  if (rule.conditions.length === 0) return "Add a condition, or turn the rule off.";
  if (rule.conditions.length > MAX_CONDITIONS) {
    return `At most ${MAX_CONDITIONS} conditions.`;
  }
  for (const c of rule.conditions) {
    if (!c.field.trim()) return "Pick the field this depends on.";
    if (!seenFields.has(c.field)) {
      return `"${c.field}" has to appear earlier in the form for this to depend on it.`;
    }
    if (VALUE_OPS.has(c.op) && !(c.value ?? "").trim()) {
      return `The condition on "${c.field}" needs a value.`;
    }
    if (!VALUE_OPS.has(c.op) && c.value != null && c.value !== "") {
      return `The condition on "${c.field}" takes no value.`;
    }
    if ((c.value ?? "").length > MAX_LABEL_LEN) {
      return `A condition value must be at most ${MAX_LABEL_LEN} characters.`;
    }
    if (CHECKBOX_OPS.has(c.op) && !seenFields.get(c.field)) {
      return `"${c.field}" is not a checkbox.`;
    }
  }
  return null;
}

export function validateLayout(items: BuilderElement[]): LayoutErrors {
  const errors: LayoutErrors = {};
  const seenKeys = new Map<string, string>();
  const seenStandard = new Set<string>();
  // Field keys passed so far → is it a checkbox. Standard fields feed this too,
  // even though they return early below, because a rule may depend on one.
  const seenFields = new Map<string, boolean>();
  // Country-valued keys passed so far — the only legal parents for a state
  // picker. Mirrors the `is_country` half of the server's `SeenField`.
  const seenCountries = new Set<string>();

  const record = (item: BuilderElement) => {
    const el = item.element;
    if (el.element === "standard") seenFields.set(el.config.key, false);
    if (el.element === "custom" && el.config.key.trim()) {
      seenFields.set(el.config.key, el.config.type === "checkbox");
    }
    if (isCountryField(el)) {
      seenCountries.add(el.element === "custom" ? el.config.key : "country");
    }
  };

  items.forEach((item, index) => {
    const el = item.element;

    // Checked before this element records its own key, so a self-reference
    // reads as "does not appear earlier" — same order as the server.
    const rule = elementRule(el);
    if (rule) {
      const problem = visibilityError(rule, seenFields);
      if (problem) errors[item.id] = problem;
    }

    if (el.element === "standard") {
      if (seenStandard.has(el.config.key)) {
        errors[item.id] = "This standard field is already on the form.";
      } else if (
        el.config.key === "state" &&
        el.config.input_override === "select" &&
        !seenCountries.has("country")
      ) {
        errors[item.id] =
          "The state dropdown needs the Country field earlier in the form, also set to a dropdown.";
      }
      seenStandard.add(el.config.key);
      record(item);
      return;
    }

    if (el.element === "custom") {
      const key = el.config.key.trim();
      const label = el.config.label.trim();
      // One error per element: a rule problem already reported above wins, so
      // the chain below only fills an empty slot.
      if (errors[item.id]) {
        if (key && !seenKeys.has(key)) seenKeys.set(key, item.id);
        record(item);
        return;
      }
      if (!label) {
        errors[item.id] = "Label is required.";
      } else if (label.length > MAX_LABEL_LEN) {
        errors[item.id] = `Label must be at most ${MAX_LABEL_LEN} characters.`;
      } else if (!key) {
        errors[item.id] = "Key is required.";
      } else if (key.length > MAX_KEY_LEN) {
        errors[item.id] = `Key must be at most ${MAX_KEY_LEN} characters.`;
      } else if (/\s/.test(key)) {
        errors[item.id] = "Key can't contain spaces.";
      } else if (STANDARD_KEYS.has(key)) {
        errors[item.id] = `"${key}" collides with a standard field.`;
      } else if (seenKeys.has(key)) {
        errors[item.id] = `Duplicate key "${key}".`;
      } else if (
        (el.config.type === "select" || el.config.type === "radio") &&
        (el.config.options ?? []).length === 0
      ) {
        errors[item.id] =
          el.config.type === "radio"
            ? "A radio group needs at least one option."
            : "A dropdown needs at least one option.";
      } else if (el.config.type === "state" && el.config.country_field) {
        // Unbound is fine — that is the free-text fallback. A reference that
        // names nothing earlier, or names something that isn't a country, is
        // what the server rejects.
        const parent = el.config.country_field;
        if (!seenFields.has(parent)) {
          errors[item.id] = `"${parent}" has to appear earlier in the form to drive this.`;
        } else if (!seenCountries.has(parent)) {
          errors[item.id] = `"${parent}" is not a country field.`;
        }
      }
      if (key && !seenKeys.has(key)) seenKeys.set(key, item.id);
      record(item);
      return;
    }

    if (el.element === "heading" && !el.config.text.trim() && !errors[item.id]) {
      errors[item.id] = "Heading text is required.";
    }
    if (el.element === "paragraph" && !el.config.text.trim() && !errors[item.id]) {
      errors[item.id] = "Paragraph text is required.";
    }
    if (el.element === "page_break") {
      const prev = items[index - 1];
      if (index === 0) {
        errors[item.id] = "A form can't start with a page break.";
      } else if (index === items.length - 1) {
        errors[item.id] = "A form can't end with a page break.";
      } else if (prev?.element.element === "page_break") {
        errors[item.id] = "Two page breaks in a row leave an empty step.";
      }
    }
  });

  return errors;
}
