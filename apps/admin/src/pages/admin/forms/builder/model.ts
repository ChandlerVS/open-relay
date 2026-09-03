import type { components } from "@open-relay/api-client";
import { STANDARD_FIELDS } from "@open-relay/form-renderer";

export type FormElement = components["schemas"]["FormElement"];
export type CustomField = components["schemas"]["CustomField"];
export type StandardElement = components["schemas"]["StandardElement"];
export type CustomFieldType = components["schemas"]["CustomFieldType"];
export type VisibilityRule = components["schemas"]["VisibilityRule"];
export type Condition = components["schemas"]["Condition"];
export type ConditionOp = components["schemas"]["ConditionOp"];

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
  { type: "radio", label: "Radio group" },
  { type: "checkbox", label: "Checkbox" },
] as const;

export type CustomTypeName = (typeof CUSTOM_FIELD_TYPES)[number]["type"];

/** The two types that offer a fixed set of choices. */
export function hasOptions(type: CustomTypeName): boolean {
  return type === "select" || type === "radio";
}

/**
 * Rebuild a custom field around a new type, dropping options on any type that
 * can't carry them so the discriminated union stays clean.
 *
 * The destructure is a whitelist, so anything added to `CustomField` has to be
 * listed here or it silently vanishes the moment someone changes a field's
 * type. Options survive a move between the two choice types, since they mean
 * the same thing to both.
 */
export function retypeCustomField(field: CustomField, type: CustomTypeName): CustomField {
  const { key, label, required, placeholder, help_text, position, width, default_value, visible_when } =
    field;
  const base = {
    key,
    label,
    required,
    placeholder,
    help_text,
    position,
    width,
    default_value,
    visible_when,
  };
  return hasOptions(type)
    ? { ...base, type, options: "options" in field ? (field.options ?? []) : [] }
    : { ...base, type };
}

/** A field element's rule, or `null` for a kind that can't carry one. */
export function elementRule(el: FormElement): VisibilityRule | null {
  switch (el.element) {
    case "standard":
    case "custom":
    case "heading":
    case "paragraph":
      return el.config.visible_when ?? null;
    default:
      return null;
  }
}

/**
 * Whether this kind of element can carry a visibility rule.
 *
 * `divider` cannot: it is a serde *unit* variant on the server, so giving it a
 * body would reject every layout already stored. `page_break` deliberately
 * cannot either — a conditional break would mean a conditional step count.
 */
export function canBeConditional(el: FormElement): boolean {
  return (
    el.element === "standard" ||
    el.element === "custom" ||
    el.element === "heading" ||
    el.element === "paragraph"
  );
}

/**
 * Set (or clear) an element's rule without disturbing the rest of its config.
 *
 * Clearing *deletes* the key rather than writing `null`: the builder's dirty
 * check is a `JSON.stringify` comparison against what the server sent, and the
 * server omits the field entirely when it has no rule, so an explicit `null`
 * would make a pristine form look edited.
 */
export function withRule(el: FormElement, rule: VisibilityRule | null): FormElement {
  if (
    el.element !== "standard" &&
    el.element !== "custom" &&
    el.element !== "heading" &&
    el.element !== "paragraph"
  ) {
    return el;
  }
  const { visible_when: _drop, ...rest } = el.config;
  const config = rule ? { ...rest, visible_when: rule } : rest;
  return { ...el, config } as FormElement;
}

/** A field that could control `index`: any field element strictly before it. */
export interface ControllerCandidate {
  key: string;
  label: string;
  /** `undefined` for a standard field, which is never a checkbox or a choice. */
  type?: CustomTypeName;
  options?: string[];
}

/**
 * Candidate controllers for the element at `index`.
 *
 * "Strictly earlier" is the server's rule (`service::validate_layout`) and the
 * reason both evaluators can resolve a form in a single forward pass, so the
 * builder must not offer anything else. Fields whose key is still blank — a
 * freshly added one — are skipped rather than offered as `""`.
 */
export function controllerCandidates(
  items: BuilderElement[],
  index: number,
): ControllerCandidate[] {
  const out: ControllerCandidate[] = [];
  for (const item of items.slice(0, Math.max(index, 0))) {
    const el = item.element;
    if (el.element === "standard") {
      out.push({ key: el.config.key, label: elementTitle(el) });
    } else if (el.element === "custom" && el.config.key.trim()) {
      out.push({
        key: el.config.key,
        label: elementTitle(el),
        type: el.config.type,
        options: "options" in el.config ? el.config.options : undefined,
      });
    }
  }
  return out;
}

/**
 * Repoint every rule that referenced `from` at `to`, so renaming a controller's
 * key doesn't dangle the rules that depend on it. (Renaming still orphans
 * already-collected submission values — that caveat is unchanged.)
 */
export function renameRuleReferences(
  items: BuilderElement[],
  from: string,
  to: string,
): BuilderElement[] {
  if (!from || from === to) return items;
  return items.map((item) => {
    const rule = elementRule(item.element);
    if (!rule?.conditions.some((c) => c.field === from)) return item;
    const next = {
      ...rule,
      conditions: rule.conditions.map((c) => (c.field === from ? { ...c, field: to } : c)),
    };
    return { ...item, element: withRule(item.element, next) };
  });
}

/**
 * Drop conditions pointing at `key`, and the rule outright once it has none
 * left. Used when the controlling element is deleted, so removing a field can't
 * leave the form in a state the server refuses to save.
 */
export function stripRuleReferences(items: BuilderElement[], key: string): BuilderElement[] {
  if (!key) return items;
  return items.map((item) => {
    const rule = elementRule(item.element);
    if (!rule?.conditions.some((c) => c.field === key)) return item;
    const conditions = rule.conditions.filter((c) => c.field !== key);
    return {
      ...item,
      element: withRule(item.element, conditions.length ? { ...rule, conditions } : null),
    };
  });
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
    config: hasOptions(type) ? { ...base, type, options: [] } : { ...base, type },
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
