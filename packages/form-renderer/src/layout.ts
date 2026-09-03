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
