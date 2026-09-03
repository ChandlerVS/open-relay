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
 * Overrides how a standard field renders. Only `country` and `state` have a
 * select variant: country submits an ISO 3166-1 alpha-2 code, state submits a
 * bare ISO 3166-2 subdivision code (`CA`, not `US-CA`) drawn from whatever the
 * standard `country` field holds — which the server requires to be a dropdown
 * too, since a free-text country holds a name rather than a code.
 */
export type StandardInputVariant = "text" | "select";

/** Whether every condition must hold, or only one. Absent means `"all"`. */
export type MatchMode = "all" | "any";

/** Mirrors `open_relay_core::forms::ConditionOp`. */
export type ConditionOp =
  | "equals"
  | "not_equals"
  | "contains"
  | "is_empty"
  | "is_not_empty"
  | "is_checked"
  | "is_not_checked";

/** One comparison against a controlling field's answer. */
export interface Condition {
  /** Submission key of a field appearing strictly earlier in the layout. */
  field: string;
  op: ConditionOp;
  /** Present for `equals`/`not_equals`/`contains`, absent for the rest. */
  value?: string | null;
}

/**
 * Show an element only when earlier answers match. Mirrors
 * `open_relay_core::forms::VisibilityRule`; evaluated by `visibility.ts`, which
 * must stay in lockstep with `crates/core/src/forms/visibility.rs`.
 */
export interface VisibilityRule {
  match?: MatchMode;
  conditions: Condition[];
}

export type CustomField =
  | (CustomFieldBase & { type: "text" })
  | (CustomFieldBase & { type: "email" })
  | (CustomFieldBase & { type: "number" })
  | (CustomFieldBase & { type: "tel" })
  | (CustomFieldBase & { type: "url" })
  | (CustomFieldBase & { type: "textarea" })
  | (CustomFieldBase & { type: "select"; options: string[] })
  | (CustomFieldBase & { type: "radio"; options: string[] })
  | (CustomFieldBase & { type: "checkbox" })
  /** ISO 3166-1 country picker. Submits the alpha-2 code. */
  | (CustomFieldBase & { type: "country" })
  /**
   * ISO 3166-2 subdivision picker. Submits the bare subdivision code (`CA`,
   * not `US-CA`).
   *
   * `country_field` names a country-valued field appearing **strictly
   * earlier** in the layout — the same backwards-only rule `visible_when`
   * follows, which is what lets the renderer resolve every picker in one
   * forward pass. Absent means unbound, which renders free text.
   */
  | (CustomFieldBase & { type: "state"; country_field?: string | null });

interface CustomFieldBase {
  key: string;
  label: string;
  required?: boolean;
  placeholder?: string | null;
  help_text?: string | null;
  position: number;
  width?: FieldWidth;
  default_value?: string | null;
  /** Show this field only when earlier answers match. Absent is unconditional. */
  visible_when?: VisibilityRule | null;
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
  /** Show this field only when earlier answers match. Absent is unconditional. */
  visible_when?: VisibilityRule | null;
}

export interface HeadingElement {
  text: string;
  level: number;
  /** Show this field only when earlier answers match. Absent is unconditional. */
  visible_when?: VisibilityRule | null;
}

export interface ParagraphElement {
  text: string;
  /** Show this field only when earlier answers match. Absent is unconditional. */
  visible_when?: VisibilityRule | null;
}

export interface PageBreakElement {
  title?: string | null;
}

/**
 * One entry in a form's ordered layout. Adjacently tagged to match
 * `open_relay_core::forms::FormElement` — `element` discriminates, `config`
 * carries the body (absent for `divider`).
 *
 * `divider` alone cannot carry a `visible_when` — it is a serde *unit* variant
 * on the server, and giving it a body would reject every stored
 * `{"element":"divider"}`. The renderer collapses dividers left stranded by
 * their hidden neighbours instead; see `visibility.ts`.
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

/**
 * How a multi-step form shows the visitor their progress. Mirrors
 * `open_relay_core::forms::ProgressStyle`. Ignored by single-page forms.
 */
export type ProgressStyle = "bar" | "steps" | "none";

/** Mirrors `open_relay_core::forms::ProgressIndicator`. */
export interface ProgressIndicator {
  style?: ProgressStyle;
  /** Draw the numeric percentage next to the bar. Only read for `"bar"`. */
  show_percent?: boolean;
}

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
  /**
   * How to show progress through a multi-step form. Optional so a bundle newer
   * than its server still renders — an absent value means the default bar.
   */
  progress_indicator?: ProgressIndicator;
  /**
   * Packed ISO 3166-2 subdivisions, sent only when this form has a field that
   * needs them (see `regions.ts` for the format).
   *
   * The table is ~40 KB gzipped and most forms have no state field, so it is
   * deliberately not in the bundle. Absent is the normal case, and a state
   * picker with no table falls back to a text input — the same shape the
   * server accepts for a country with no subdivisions.
   */
  regions?: string | null;
}
