import { STANDARD_FIELDS } from "@open-relay/form-renderer";
import type { BuilderElement } from "./model";

/** Errors keyed by BuilderElement.id, so they survive a reorder. */
export type LayoutErrors = Record<string, string>;

const MAX_KEY_LEN = 64;
const MAX_LABEL_LEN = 200;
const STANDARD_KEYS = new Set(STANDARD_FIELDS.map((f) => f.key));

/**
 * Mirrors `open_relay_core::forms::service::validate_layout` so the builder can
 * show problems inline instead of surfacing one opaque 400 on save. The server
 * remains the authority — this is a convenience, not a substitute.
 */
export function validateLayout(items: BuilderElement[]): LayoutErrors {
  const errors: LayoutErrors = {};
  const seenKeys = new Map<string, string>();
  const seenStandard = new Set<string>();

  items.forEach((item, index) => {
    const el = item.element;
    if (el.element === "standard") {
      if (seenStandard.has(el.config.key)) {
        errors[item.id] = "This standard field is already on the form.";
      }
      seenStandard.add(el.config.key);
      return;
    }

    if (el.element === "custom") {
      const key = el.config.key.trim();
      const label = el.config.label.trim();
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
      } else if (el.config.type === "select" && (el.config.options ?? []).length === 0) {
        errors[item.id] = "A dropdown needs at least one option.";
      }
      if (key && !seenKeys.has(key)) seenKeys.set(key, item.id);
      return;
    }

    if (el.element === "heading" && !el.config.text.trim()) {
      errors[item.id] = "Heading text is required.";
    }
    if (el.element === "paragraph" && !el.config.text.trim()) {
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
