// Shared standard-field catalogue. Keys MUST match the property names on the
// backend's `StandardFieldsConfig`. Kept in sync (by hand) with
// `apps/admin/src/lib/forms/standardFields.ts`.

export interface StandardFieldDef {
  key: string;
  default_label: string;
  /** HTML input type when rendered as a normal text-style field. */
  input_type: "text" | "email" | "tel" | "url" | "textarea";
  autocomplete?: string;
}

export const STANDARD_FIELDS: readonly StandardFieldDef[] = [
  { key: "first_name", default_label: "First name", input_type: "text", autocomplete: "given-name" },
  { key: "last_name", default_label: "Last name", input_type: "text", autocomplete: "family-name" },
  { key: "email", default_label: "Email", input_type: "email", autocomplete: "email" },
  { key: "phone", default_label: "Phone", input_type: "tel", autocomplete: "tel" },
  { key: "company", default_label: "Company", input_type: "text", autocomplete: "organization" },
  { key: "job_title", default_label: "Job title", input_type: "text", autocomplete: "organization-title" },
  { key: "website", default_label: "Website", input_type: "url", autocomplete: "url" },
  { key: "message", default_label: "Message", input_type: "textarea" },
  { key: "address_line_1", default_label: "Address line 1", input_type: "text", autocomplete: "address-line1" },
  { key: "address_line_2", default_label: "Address line 2", input_type: "text", autocomplete: "address-line2" },
  { key: "city", default_label: "City", input_type: "text", autocomplete: "address-level2" },
  { key: "state", default_label: "State / region", input_type: "text", autocomplete: "address-level1" },
  { key: "postal_code", default_label: "Postal code", input_type: "text", autocomplete: "postal-code" },
  { key: "country", default_label: "Country", input_type: "text", autocomplete: "country-name" },
] as const;

export type StandardFieldKey = (typeof STANDARD_FIELDS)[number]["key"];
