export {
  Form,
  DEFAULT_THANKS,
  DEFAULT_RESUBMIT_LABEL,
  type FormProps,
  type FormTheme,
} from "./Form";
export { ShadowForm } from "./ShadowForm";
export {
  STANDARD_FIELDS,
  COUNTRIES,
  type CountryOption,
  type StandardFieldDef,
  type StandardFieldKey,
} from "./standardFields";
export {
  resolveLayout,
  layoutFromLegacy,
  splitIntoPages,
  pageFieldKeys,
  type FormPage,
} from "./layout";
export type {
  PublicFormDto,
  StandardFieldConfig,
  StandardFieldsConfig,
  CustomField,
  FormElement,
  StandardElement,
  HeadingElement,
  ParagraphElement,
  PageBreakElement,
  FieldWidth,
  StandardInputVariant,
  PostSubmissionAction,
  MessageAction,
  RedirectAction,
} from "./schema";
