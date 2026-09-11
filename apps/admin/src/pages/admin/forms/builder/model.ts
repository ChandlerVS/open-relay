import type { components } from "@open-relay/api-client";
import { COUNTRIES, STANDARD_FIELDS } from "@open-relay/form-renderer";

export type FormElement = components["schemas"]["FormElement"];
export type CustomField = components["schemas"]["CustomField"];
export type StandardElement = components["schemas"]["StandardElement"];
export type CustomFieldType = components["schemas"]["CustomFieldType"];
export type VisibilityRule = components["schemas"]["VisibilityRule"];
export type Condition = components["schemas"]["Condition"];
export type ConditionOp = components["schemas"]["ConditionOp"];
export type FieldWidth = components["schemas"]["FieldWidth"];

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
    case "rich_text":
      return markdownTitle(el.config.markdown) || "Rich text";
    case "divider":
      return "Divider";
    case "page_break":
      return el.config.title?.trim() || "Page break";
    case "row_start":
      return el.config.label?.trim() || "Row";
    case "row_end":
      return "End of row";
  }
}

export type RichTextTone = components["schemas"]["RichTextTone"];

export const RICH_TEXT_TONES: { value: RichTextTone; label: string }[] = [
  { value: "normal", label: "Normal" },
  { value: "muted", label: "Muted — fine print" },
  { value: "info", label: "Info callout" },
  { value: "warning", label: "Warning callout" },
  { value: "danger", label: "Danger callout" },
];

/**
 * Set a block's tone without disturbing the rest of its config.
 *
 * `normal` *deletes* the key rather than writing it, because the server omits a
 * default tone entirely (`RichTextTone::is_default`) and the dirty check below
 * is a `JSON.stringify` comparison — the same reason `withRule` deletes.
 */
export function withTone(el: FormElement, tone: RichTextTone): FormElement {
  if (el.element !== "rich_text") return el;
  const { tone: _drop, ...rest } = el.config;
  return {
    ...el,
    config: tone === "normal" ? rest : { ...rest, tone },
  } as FormElement;
}

/** First meaningful line of markdown, with its markers stripped, for a summary row. */
function markdownTitle(md: string): string {
  const line = md.split("\n").find((l) => l.trim() && !/^\s*[-*_]{3,}\s*$/.test(l)) ?? "";
  return line
    .replace(/^\s*(?:#{1,6} +|[-*+] +|\d{1,9}[.)] +|> ?)/, "")
    .replace(/[*_`]/g, "")
    .trim()
    .slice(0, 60);
}

/**
 * Default per-file cap, matching `storage::DEFAULT_MAX_FILE_MB`. The hard
 * ceiling is enforced server-side (`MAX_UPLOAD_MB`); this is just the value a
 * fresh field starts at.
 */
export const DEFAULT_MAX_FILE_MB = 10;
/** Ceiling the server clamps to, mirrored so the editor can bound its input. */
export const MAX_UPLOAD_MB = 100;

/**
 * Whole-form ceilings, mirroring `service.rs`.
 *
 * Nothing checked these before: clicking the palette one element at a time made
 * them unreachable in practice, so the only mirror that existed was of the
 * per-field limits above. Pasting a block puts all three within one keystroke,
 * and the server's refusal names no element, so a paste that would breach one is
 * turned away here instead.
 */
export const MAX_LAYOUT_ELEMENTS = 300;
export const MAX_CUSTOM_FIELDS = 100;
export const MAX_PAGES = 20;
export const MAX_KEY_LEN = 64;

/**
 * The reserved standard keys, as a set.
 *
 * `validate.ts` built this privately and the paste transform needs the same
 * thing, so it lives here rather than becoming a third copy of the catalogue.
 */
export const STANDARD_KEYS: ReadonlySet<string> = new Set(STANDARD_FIELDS.map((f) => f.key));

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
  { type: "country", label: "Country" },
  { type: "state", label: "State / province" },
  { type: "file", label: "File upload" },
] as const;

export type CustomTypeName = (typeof CUSTOM_FIELD_TYPES)[number]["type"];

/**
 * The two types that offer an **author-declared** set of choices.
 *
 * `country` and `state` render dropdowns too, but their choices come from the
 * ISO catalogue, so they have no options editor and no "needs at least one
 * option" rule. Mirrors `CustomFieldType::options` on the server.
 */
export function hasOptions(type: CustomTypeName): boolean {
  return type === "select" || type === "radio";
}

/**
 * The narrowing form of [`hasOptions`], so the options editor can reach
 * `field.options` without restating the type test inline.
 */
export function fieldHasOptions(
  field: CustomField,
): field is CustomField & { options?: string[] } {
  return hasOptions(field.type);
}

/** Whether a field's answer is an ISO country code, and so can drive a state picker. */
export function isCountryField(el: FormElement): boolean {
  if (el.element === "custom") return el.config.type === "country";
  // A plain-text standard country holds a *name*, not a code.
  return (
    el.element === "standard" &&
    el.config.key === "country" &&
    el.config.input_override === "select"
  );
}

/** The country field a state picker is bound to, or `null`. */
export function countryFieldRef(el: FormElement): string | null {
  if (el.element !== "custom" || el.config.type !== "state") return null;
  return el.config.country_field ?? null;
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
  if (hasOptions(type)) {
    return { ...base, type, options: "options" in field ? (field.options ?? []) : [] };
  }
  if (type === "state") {
    // Survives a move away and back, the same way options do between the two
    // choice types.
    return {
      ...base,
      type,
      country_field: "country_field" in field ? field.country_field : undefined,
    };
  }
  if (type === "file") {
    return {
      ...base,
      type,
      accept: "accept" in field ? (field.accept ?? []) : [],
      max_size_mb: "max_size_mb" in field ? field.max_size_mb : DEFAULT_MAX_FILE_MB,
    };
  }
  return { ...base, type };
}

/** A field element's rule, or `null` for a kind that can't carry one. */
export function elementRule(el: FormElement): VisibilityRule | null {
  switch (el.element) {
    case "standard":
    case "custom":
    case "heading":
    case "paragraph":
    case "rich_text":
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
 * cannot either — a conditional break would mean a conditional step count. Nor
 * can a row marker: the fields in a row carry their own rules, and a row whose
 * fields have all hidden is collapsed by the renderer.
 */
export function canBeConditional(el: FormElement): boolean {
  return (
    el.element === "standard" ||
    el.element === "custom" ||
    el.element === "heading" ||
    el.element === "paragraph" ||
    el.element === "rich_text"
  );
}

/** Is this element one half of a row marker pair? */
export function isRowMarker(el: FormElement): boolean {
  return el.element === "row_start" || el.element === "row_end";
}

/** May this element sit inside a row? Mirrors `FormElement::allowed_in_row`. */
export function allowedInRow(el: FormElement): boolean {
  return el.element === "standard" || el.element === "custom";
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
    el.element !== "paragraph" &&
    el.element !== "rich_text"
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
  /**
   * Display names for `options` whose stored value isn't human-readable — a
   * country picker's operands are ISO codes. Absent means the option string is
   * its own label, which is true of every author-declared option.
   */
  optionLabels?: Record<string, string>;
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
        // A country picker's answers are a known set too, so offer them rather
        // than inviting a typo — but as names over codes. A *state* picker is
        // deliberately left to free text: its answer set depends on an answer
        // nobody has given yet, and the union of all of them is 3,590 entries.
        ...(el.config.type === "country"
          ? {
              options: COUNTRIES.map((c) => c.code),
              optionLabels: Object.fromEntries(COUNTRIES.map((c) => [c.code, c.name])),
            }
          : { options: "options" in el.config ? el.config.options : undefined }),
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

/**
 * Country fields that could drive the state picker at `index`.
 *
 * Same "strictly earlier" rule as `controllerCandidates`, and for the same
 * reason: it is what `service::validate_layout` enforces and what lets the
 * renderer resolve every picker in one forward pass.
 */
export function countryFieldCandidates(
  items: BuilderElement[],
  index: number,
): ControllerCandidate[] {
  const out: ControllerCandidate[] = [];
  for (const item of items.slice(0, Math.max(index, 0))) {
    const el = item.element;
    if (!isCountryField(el)) continue;
    const key = el.element === "custom" ? el.config.key : "country";
    if (!key.trim()) continue;
    out.push({ key, label: elementTitle(el) });
  }
  return out;
}

/** Rewrite a state picker's `country_field` when its country is renamed. */
export function renameCountryReferences(
  items: BuilderElement[],
  from: string,
  to: string,
): BuilderElement[] {
  if (!from || from === to) return items;
  return items.map((item) =>
    countryFieldRef(item.element) === from
      ? { ...item, element: withCountryField(item.element, to) }
      : item,
  );
}

/**
 * Unbind any state picker driven by `key`.
 *
 * Deleting or retyping the country strands the reference, and an unbound state
 * picker renders free text — the same repair `service::strip_dangling_country_refs`
 * applies server-side, so the builder never shows a form the server would
 * reject.
 */
export function stripCountryReferences(
  items: BuilderElement[],
  key: string,
): BuilderElement[] {
  if (!key) return items;
  return items.map((item) =>
    countryFieldRef(item.element) === key
      ? { ...item, element: withCountryField(item.element, undefined) }
      : item,
  );
}

/**
 * Bind (or unbind) a state picker's country.
 *
 * Exported because the paste transform has to unbind a reference whose country
 * was left behind in the source form, and the delete-don't-null nuance below is
 * exactly the kind of thing a second implementation gets wrong.
 */
export function withCountryField(el: FormElement, key: string | undefined): FormElement {
  if (el.element !== "custom" || el.config.type !== "state") return el;
  // Delete rather than write `null`, like `withRule` — the dirty check is a
  // `JSON.stringify` comparison, so an explicit null would read as an edit.
  const { country_field: _drop, ...rest } = el.config;
  return {
    ...el,
    config: key ? { ...rest, country_field: key } : rest,
  } as FormElement;
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

export function newCustomElement(
  type: CustomTypeName,
  index: number,
  /**
   * Existing elements, so a new state picker can bind itself to the last
   * country field ahead of it. Without this a freshly dropped state field is
   * unbound and renders free text until someone notices the setting.
   */
  items: BuilderElement[] = [],
): FormElement {
  const base = {
    key: "",
    label: "",
    required: false,
    position: index,
  };
  if (hasOptions(type)) return { element: "custom", config: { ...base, type, options: [] } };
  if (type === "state") {
    const candidates = countryFieldCandidates(items, index);
    const nearest = candidates[candidates.length - 1];
    return {
      element: "custom",
      config: nearest ? { ...base, type, country_field: nearest.key } : { ...base, type },
    };
  }
  return { element: "custom", config: { ...base, type } };
}

/**
 * A new, empty row: the marker pair, ready for fields to be dragged between.
 *
 * Two elements rather than one because a row is a flat marker pair on the wire
 * (see `RowStartElement` on the server). That is what lets the canvas stay a
 * single flat sortable list — dragging a field into a row is an ordinary
 * reorder, not a cross-container drop.
 */
export function newRowElements(): FormElement[] {
  return [{ element: "row_start", config: {} }, { element: "row_end" }];
}

/**
 * Repair row markers after a reorder.
 *
 * A flat list is what makes drag-and-drop cheap, but it also lets a drag put
 * the markers in an order that means nothing: a `row_end` above its
 * `row_start`, a marker dragged inside another row, a row left holding a
 * heading. Rather than blocking those drags — dnd-kit would have to know about
 * rows, which is the nesting complexity this design avoids — we let the drop
 * happen and tidy up after, the way the server's `normalize_layout` tidies an
 * empty row instead of rejecting it.
 *
 * Four repairs, in one pass:
 *  - a `row_end` with no open row is dropped;
 *  - a `row_start` inside an open row closes the previous one rather than
 *    nesting;
 *  - anything that can't live in a row (a heading, a divider, a page break)
 *    closes the row and lands after it;
 *  - a row left with no fields, or never closed, is dropped entirely.
 *
 * The result always satisfies `service::validate_layout`'s row rules, so a
 * reorder can never put the form into a state that won't save.
 */
export function normalizeRows(items: BuilderElement[]): BuilderElement[] {
  const out: BuilderElement[] = [];
  // Index in `out` of the open row's marker, and how many fields it holds.
  let open: { at: number; fields: number } | null = null;

  const closeRow = (closer: BuilderElement | null) => {
    if (!open) return;
    if (open.fields === 0) out.splice(open.at, 1);
    else if (closer) out.push(closer);
    else out.push({ id: newId(), element: { element: "row_end" } });
    open = null;
  };

  for (const item of items) {
    const el = item.element;

    if (el.element === "row_end") {
      // A closer with nothing open is debris from a drag; drop it.
      if (open) closeRow(item);
      continue;
    }

    if (el.element === "row_start") {
      closeRow(null); // never nest — the previous row ends here
      open = { at: out.length, fields: 0 };
      out.push(item);
      continue;
    }

    if (open && !allowedInRow(el)) {
      closeRow(null);
      out.push(item);
      continue;
    }

    if (open) open.fields += 1;
    out.push(item);
  }

  closeRow(null); // an unclosed row at the end
  return out;
}

/**
 * The other half of a row marker pair, or `null` for anything else.
 *
 * Half a pair is a layout the server rejects, so deleting either marker has to
 * mean "un-row these fields" — the only reading a user could intend. The fields
 * stay exactly where they are and go back to being full width. `removeMany`
 * ([`./blockOps`]) is the one caller; it pairs the markers up before deleting so
 * one code path serves the trash button and a bulk delete alike.
 */
export function rowPartner(items: BuilderElement[], id: string): string | null {
  const at = items.findIndex((i) => i.id === id);
  if (at === -1) return null;
  const kind = items[at]!.element.element;
  if (kind === "row_start") {
    for (let j = at + 1; j < items.length; j += 1) {
      if (items[j]!.element.element === "row_end") return items[j]!.id;
    }
  }
  if (kind === "row_end") {
    for (let j = at - 1; j >= 0; j -= 1) {
      if (items[j]!.element.element === "row_start") return items[j]!.id;
    }
  }
  return null;
}

/**
 * Renumber every custom field's `position` to its ordinal among the customs,
 * mirroring `normalize_layout` (`service.rs`).
 *
 * The server does this on every write, and the builder's dirty check compares
 * against what the server sent back — so a layout whose positions disagree with
 * layout order reads as permanently edited after a successful save. Pasting a
 * block carries the *source* form's positions, which is exactly that case.
 */
export function renumberPositions(items: BuilderElement[]): BuilderElement[] {
  let idx = 0;
  return items.map((item) => {
    if (item.element.element !== "custom") return item;
    const position = idx;
    idx += 1;
    if (item.element.config.position === position) return item;
    return {
      ...item,
      element: { ...item.element, config: { ...item.element.config, position } },
    } as BuilderElement;
  });
}

/**
 * Is the element at `index` between a `row_start` and its `row_end`?
 *
 * Derived from the flat list rather than stored, exactly as the canvas derives
 * its indentation and its step numbers.
 */
export function isInsideRow(items: BuilderElement[], index: number): boolean {
  // A marker is not "in" its own row: the opener precedes the run and the
  // closer ends it. This has to agree with the canvas, which indents a row's
  // fields but draws the two markers flush — the same derivation twice.
  const self = items[index]?.element.element;
  if (self === "row_start" || self === "row_end") return false;

  let open = false;
  for (const item of items.slice(0, Math.max(index, 0))) {
    if (item.element.element === "row_start") open = true;
    if (item.element.element === "row_end") open = false;
  }
  return open;
}

export function newDecorationElement(
  kind: "heading" | "paragraph" | "rich_text" | "divider" | "page_break",
): FormElement {
  switch (kind) {
    case "heading":
      return { element: "heading", config: { text: "Section", level: 2 } };
    case "paragraph":
      return { element: "paragraph", config: { text: "" } };
    case "rich_text":
      // No `tone` key: the default is absent on the wire, so writing it here
      // would make a freshly added block read as dirty against a saved form.
      return { element: "rich_text", config: { markdown: "" } };
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
