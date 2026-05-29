// Shared standard-field catalogue. The keys MUST match the property names on
// the backend's `StandardFieldsConfig`; the labels are the renderer/admin
// default UI copy.

export interface StandardFieldDef {
  key: string;
  default_label: string;
}

export const STANDARD_FIELDS: readonly StandardFieldDef[] = [
  { key: "first_name", default_label: "First name" },
  { key: "last_name", default_label: "Last name" },
  { key: "email", default_label: "Email" },
  { key: "phone", default_label: "Phone" },
  { key: "company", default_label: "Company" },
  { key: "job_title", default_label: "Job title" },
  { key: "website", default_label: "Website" },
  { key: "message", default_label: "Message" },
  { key: "address_line_1", default_label: "Address line 1" },
  { key: "address_line_2", default_label: "Address line 2" },
  { key: "city", default_label: "City" },
  { key: "state", default_label: "State / region" },
  { key: "postal_code", default_label: "Postal code" },
  { key: "country", default_label: "Country" },
] as const;

export type StandardFieldKey = (typeof STANDARD_FIELDS)[number]["key"];
