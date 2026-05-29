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
}

export interface PublicFormDto {
  id: number;
  name: string;
  slug: string;
  standard_fields: StandardFieldsConfig;
  custom_fields: CustomField[];
}
