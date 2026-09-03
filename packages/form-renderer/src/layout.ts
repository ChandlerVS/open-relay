import type { FormElement, PublicFormDto, StandardFieldsConfig, CustomField } from "./schema";
import { STANDARD_FIELDS } from "./standardFields";

/**
 * Derive a layout from the legacy `standard_fields` + `custom_fields` pair.
 *
 * Mirrors `open_relay_core::forms::service::layout_from_legacy`. The order is
 * the compat guarantee: enabled standard fields in catalogue order, then
 * custom fields by `position` — exactly what this renderer drew before
 * layouts existed.
 */
export function layoutFromLegacy(
  standardFields: StandardFieldsConfig,
  customFields: CustomField[],
): FormElement[] {
  const out: FormElement[] = [];
  for (const def of STANDARD_FIELDS) {
    const cfg = standardFields[def.key];
    if (!cfg?.enabled) continue;
    out.push({
      element: "standard",
      config: { key: def.key, required: cfg.required, label: cfg.label },
    });
  }
  for (const f of [...customFields].sort((a, b) => a.position - b.position)) {
    out.push({ element: "custom", config: f });
  }
  return out;
}

/**
 * The layout to render. Prefers the server's `layout`; falls back to deriving
 * one, which only happens against a server older than this bundle.
 */
export function resolveLayout(schema: PublicFormDto): FormElement[] {
  return schema.layout ?? layoutFromLegacy(schema.standard_fields, schema.custom_fields);
}

/** One step of a multi-page form. */
export interface FormPage {
  title: string | null;
  elements: FormElement[];
  /**
   * Index of this page's first element in the flat layout. Conditional
   * visibility is resolved over the whole layout in one pass, so a page needs
   * this to look its own elements up in that result — and it gives decoration
   * elements a React key that doesn't shift when a sibling hides.
   */
  offset: number;
}

/**
 * Split a layout into pages on `page_break`. A break's title names the page it
 * opens, so the first page's title is always null. Always returns at least one
 * page, so single-page forms need no special casing at the call site.
 */
export function splitIntoPages(layout: FormElement[]): FormPage[] {
  const pages: FormPage[] = [{ title: null, elements: [], offset: 0 }];
  layout.forEach((el, i) => {
    if (el.element === "page_break") {
      pages.push({ title: el.config.title ?? null, elements: [], offset: i + 1 });
    } else {
      pages[pages.length - 1]!.elements.push(el);
    }
  });
  return pages;
}

/**
 * Submission keys on a page.
 *
 * Layout-shaped, not submission-shaped: this includes fields a visibility rule
 * currently hides. Use `computeVisibility` from `visibility.ts` if you need the
 * keys that will actually be submitted.
 */
export function pageFieldKeys(page: FormPage): string[] {
  return page.elements
    .map((el) =>
      el.element === "standard" || el.element === "custom" ? el.config.key : null,
    )
    .filter((k): k is string => k !== null);
}

/**
 * How each subdivision picker is bound to a country field, both ways round.
 *
 * One function so the two uses can't disagree. `byState` answers "which
 * country's list do I draw?"; `byCountry` answers "which answers do I clear
 * when this country changes?" — because a subdivision code only means anything
 * under its own country, and a visitor who picks `US` / `CA` then switches to
 * `FR` would otherwise submit California under France. The server rejects that
 * pairing, so without the clearing they'd find out at submit time, on a field
 * that looks answered.
 *
 * Mirrors what `service::validate_layout` will accept, which is why both ends
 * are checked:
 *
 * - a custom `state` names its country explicitly, and unbound is legal (it
 *   renders free text);
 * - the standard pair is a singleton each, so its binding is implicit — but
 *   only when *both* are dropdowns, since a free-text country holds a name
 *   rather than a code.
 *
 * A binding this function omits is one the server would refuse to save, so the
 * builder preview can't show a working dropdown for a form that won't save.
 */
export function stateBindings(layout: FormElement[]): {
  byCountry: Map<string, string[]>;
  byState: Map<string, string>;
} {
  const byCountry = new Map<string, string[]>();
  const byState = new Map<string, string>();
  const standardCountryIsSelect = layout.some(
    (el) =>
      el.element === "standard" &&
      el.config.key === "country" &&
      el.config.input_override === "select",
  );
  const bind = (country: string, state: string) => {
    byState.set(state, country);
    const list = byCountry.get(country);
    if (list) list.push(state);
    else byCountry.set(country, [state]);
  };
  for (const el of layout) {
    if (el.element === "custom" && el.config.type === "state") {
      if (el.config.country_field) bind(el.config.country_field, el.config.key);
    } else if (
      el.element === "standard" &&
      el.config.key === "state" &&
      el.config.input_override === "select" &&
      standardCountryIsSelect
    ) {
      bind("country", "state");
    }
  }
  return { byCountry, byState };
}
