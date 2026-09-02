// Mirrors `open_relay_core::forms::PublicFormDto`. We don't import from
// `@open-relay/api-client` here so the bundle stays free of openapi-fetch
// machinery — the embed SDK ships into third-party host pages and needs to
// stay small.

export interface StandardFieldConfig {
  enabled: boolean;
  required: boolean;
  label?: string | null;
}

export type StandardFieldsConfig = Record<string, StandardFieldConfig>;

/** Fraction of a row a field occupies. Absent means full width. */
export type FieldWidth = "full" | "half";

/**
 * Overrides how a standard field renders. Only `country` has a select variant
 * today; it submits an ISO 3166-1 alpha-2 code.
 */
export type StandardInputVariant = "text" | "select";

export type CustomField =
  | (CustomFieldBase & { type: "text" })
  | (CustomFieldBase & { type: "email" })
  | (CustomFieldBase & { type: "number" })
  | (CustomFieldBase & { type: "tel" })
  | (CustomFieldBase & { type: "url" })
  | (CustomFieldBase & { type: "textarea" })
  | (CustomFieldBase & { type: "select"; options: string[] })
  | (CustomFieldBase & { type: "checkbox" });

interface CustomFieldBase {
  key: string;
  label: string;
  required?: boolean;
  placeholder?: string | null;
  help_text?: string | null;
  position: number;
  width?: FieldWidth;
  default_value?: string | null;
}

export interface StandardElement {
  key: string;
  required?: boolean;
  label?: string | null;
  placeholder?: string | null;
  help_text?: string | null;
  width?: FieldWidth;
  default_value?: string | null;
  input_override?: StandardInputVariant | null;
}

export interface HeadingElement {
  text: string;
  level: number;
}

export interface ParagraphElement {
  text: string;
}

export interface PageBreakElement {
  title?: string | null;
}

/**
 * One entry in a form's ordered layout. Adjacently tagged to match
 * `open_relay_core::forms::FormElement` — `element` discriminates, `config`
 * carries the body (absent for `divider`).
 */
export type FormElement =
  | { element: "standard"; config: StandardElement }
  | { element: "custom"; config: CustomField }
  | { element: "heading"; config: HeadingElement }
  | { element: "paragraph"; config: ParagraphElement }
  | { element: "divider" }
  | { element: "page_break"; config: PageBreakElement };

/**
 * Body of a `message` post-submission action. Every field is optional; an
 * absent one falls back to the renderer's built-in default.
 */
export interface MessageAction {
  /** Plain text. Newlines are preserved; nothing is parsed as markup. */
  message?: string | null;
  /** Show a button that resets the form in place for another submission. */
  allow_resubmit?: boolean;
  resubmit_label?: string | null;
}

/** Body of a `redirect` post-submission action. */
export interface RedirectAction {
  /** Absolute http(s) URL. Re-checked here before navigating — see `Form.tsx`. */
  url: string;
}

/**
 * What the renderer does once a submission is accepted. Adjacently tagged to
 * match `open_relay_core::forms::PostSubmissionAction` — `action`
 * discriminates, `config` carries the body.
 */
export type PostSubmissionAction =
  | { action: "message"; config: MessageAction }
  | { action: "redirect"; config: RedirectAction };

export interface PublicFormDto {
  id: number;
  name: string;
  slug: string;
  /**
   * Legacy field config, kept so bundles cached before `layout` existed keep
   * working. Prefer `layout`; this is only the fallback input to
   * `resolveLayout`.
   */
  standard_fields: StandardFieldsConfig;
  /** See `standard_fields`. */
  custom_fields: CustomField[];
  /**
   * Ordered layout. Optional so a bundle newer than its server still renders —
   * `resolveLayout` derives one from the legacy pair when it's missing.
   */
  layout?: FormElement[];
  /**
   * What happens after a successful submission. Optional so a bundle newer than
   * its server still renders — an absent value means the default message.
   */
  post_submission_action?: PostSubmissionAction;
}
