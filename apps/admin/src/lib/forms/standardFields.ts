// The standard-field catalogue lives in `@open-relay/form-renderer`, which the
// admin already depends on for the preview. Re-exported here so existing admin
// imports keep working and there is only one TS copy to keep in step with the
// Rust `declare_standard_fields!` list.
export {
  STANDARD_FIELDS,
  COUNTRIES,
  type CountryOption,
  type StandardFieldDef,
  type StandardFieldKey,
} from "@open-relay/form-renderer";
