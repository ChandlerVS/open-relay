import type { PublicFormDto } from "@open-relay/form-renderer";

/**
 * A static form for the theme editor's preview, chosen to put every themed
 * surface on screen at once: headings and body text, a row, text inputs, a
 * select, radios, a checkbox, a textarea, a star rating, an `info` rich-text
 * block (link colour), and a page break so the progress bar and Next button
 * draw. It never touches the API — `previewMode` disables submit.
 */
export const SAMPLE_SCHEMA: PublicFormDto = {
  id: 0,
  name: "Book a demo",
  slug: "theme-preview",
  // Only here to satisfy the type; the renderer prefers `layout`.
  standard_fields: {},
  custom_fields: [],
  layout: [
    {
      element: "rich_text",
      config: {
        markdown: "Tell us a little about your team. See our [privacy policy](https://example.com/privacy).",
        tone: "info",
      },
    },
    { element: "row_start", config: {} },
    { element: "standard", config: { key: "first_name", required: true } },
    { element: "standard", config: { key: "last_name", required: true } },
    { element: "row_end" },
    {
      element: "standard",
      config: { key: "email", required: true, help_text: "We'll never share it." },
    },
    {
      element: "custom",
      config: {
        key: "team_size",
        label: "Team size",
        type: "select",
        options: ["1–10", "11–50", "51–200", "200+"],
        position: 0,
      },
    },
    {
      element: "custom",
      config: {
        key: "contact_method",
        label: "Preferred contact",
        type: "radio",
        options: ["Email", "Phone"],
        position: 1,
      },
    },
    {
      element: "custom",
      config: {
        key: "experience",
        label: "How was your last demo?",
        type: "rating",
        position: 2,
      },
    },
    { element: "standard", config: { key: "message", placeholder: "Anything else?" } },
    {
      element: "custom",
      config: {
        key: "newsletter",
        label: "Send me product updates",
        type: "checkbox",
        position: 3,
      },
    },
    { element: "page_break", config: { title: "Scheduling" } },
    { element: "heading", config: { text: "Pick a time", level: 3 } },
    {
      element: "custom",
      config: { key: "preferred_date", label: "Preferred date", type: "text", position: 4 },
    },
  ],
  progress_indicator: { style: "bar", show_percent: true },
};
