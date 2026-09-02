import type { components } from "@open-relay/api-client";
import { STANDARD_FIELDS } from "@open-relay/form-renderer";

export type FormElement = components["schemas"]["FormElement"];
export type CustomField = components["schemas"]["CustomField"];
export type StandardElement = components["schemas"]["StandardElement"];
export type CustomFieldType = components["schemas"]["CustomFieldType"];

/**
 * A layout element plus a client-side identity.
 *
 * Sortable lists must key by something stable across reorders. Array index
 * won't do — that was the bug in the old CustomFieldsEditor, whose
 * index-keyed error map went stale whenever a field moved. A field's `key` is
 * no good either, because it is editable and starts out blank on a new field.
 */
export interface BuilderElement {
  id: string;
  element: FormElement;
}

let counter = 0;
export function newId(): string {
  counter += 1;
  return `el-${counter}-${Math.random().toString(36).slice(2, 8)}`;
}

export function withIds(layout: FormElement[]): BuilderElement[] {
  return layout.map((element) => ({ id: newId(), element }));
}

export function stripIds(items: BuilderElement[]): FormElement[] {
  return items.map((i) => i.element);
}

/** Submission key for a field element; `null` for decoration. */
export function elementKey(el: FormElement): string | null {
  return el.element === "standard" || el.element === "custom" ? el.config.key : null;
}

export function elementTitle(el: FormElement): string {
  switch (el.element) {
    case "standard": {
      const def = STANDARD_FIELDS.find((d) => d.key === el.config.key);
      return el.config.label?.trim() || def?.default_label || el.config.key;
    }
    case "custom":
      return el.config.label?.trim() || el.config.key || "Untitled field";
    case "heading":
      return el.config.text || "Heading";
    case "paragraph":
      return el.config.text || "Paragraph";
    case "divider":
      return "Divider";
    case "page_break":
      return el.config.title?.trim() || "Page break";
  }
}

export const CUSTOM_FIELD_TYPES = [
  { type: "text", label: "Text" },
  { type: "email", label: "Email" },
  { type: "number", label: "Number" },
  { type: "tel", label: "Phone" },
  { type: "url", label: "URL" },
  { type: "textarea", label: "Long text" },
  { type: "select", label: "Dropdown" },
  { type: "checkbox", label: "Checkbox" },
] as const;

export type CustomTypeName = (typeof CUSTOM_FIELD_TYPES)[number]["type"];

/**
 * Rebuild a custom field around a new type, dropping options on any type that
 * can't carry them so the discriminated union stays clean.
 */
export function retypeCustomField(field: CustomField, type: CustomTypeName): CustomField {
  const { key, label, required, placeholder, help_text, position, width, default_value } = field;
  const base = { key, label, required, placeholder, help_text, position, width, default_value };
  return type === "select"
    ? { ...base, type, options: "options" in field ? (field.options ?? []) : [] }
    : { ...base, type };
}

export function newStandardElement(key: string): FormElement {
  return {
    element: "standard",
    config: {
      key,
      required: false,
      // Country is the one standard field with a select variant, and a
      // dropdown is the better default for anything created from now on.
      // Existing forms keep free text until an admin opts in.
      ...(key === "country" ? { input_override: "select" as const } : {}),
    },
  };
}

export function newCustomElement(type: CustomTypeName, index: number): FormElement {
  const base = {
    key: "",
    label: "",
    required: false,
    position: index,
  };
  return {
    element: "custom",
    config: type === "select" ? { ...base, type, options: [] } : { ...base, type },
  };
}

export function newDecorationElement(
  kind: "heading" | "paragraph" | "divider" | "page_break",
): FormElement {
  switch (kind) {
    case "heading":
      return { element: "heading", config: { text: "Section", level: 2 } };
    case "paragraph":
      return { element: "paragraph", config: { text: "" } };
    case "divider":
      return { element: "divider" };
    case "page_break":
      return { element: "page_break", config: {} };
  }
}

/** Standard keys already placed, so the palette can grey them out. */
export function usedStandardKeys(items: BuilderElement[]): Set<string> {
  const out = new Set<string>();
  for (const i of items) {
    if (i.element.element === "standard") out.add(i.element.config.key);
  }
  return out;
}
