import { isHexColor } from "@open-relay/form-renderer";
import type { ThemeColors, ThemeSettings } from "../../../lib/formThemes/useThemes";

export type ColorKey = keyof ThemeColors;
export type FontSize = NonNullable<ThemeSettings["font_size"]>;
export type Density = NonNullable<ThemeSettings["density"]>;

/** The renderer's built-in corner radius (`0.5rem`), in pixels. */
export const DEFAULT_RADIUS = 8;

/**
 * Field order of `open_relay_core::themes::ThemeColors`. `canonicalSettings`
 * rebuilds objects in this order so a `JSON.stringify` comparison against what
 * the server sent back converges — key order is otherwise whatever order the
 * admin happened to edit things in.
 */
const COLOR_KEYS: readonly ColorKey[] = [
  "background",
  "text",
  "muted",
  "border",
  "input_background",
  "input_border",
  "accent",
  "accent_text",
  "error",
  "link",
  "rating",
];

/**
 * The editor's colour layout. `builtin` is the light-mode value from the
 * renderer's `styles.css`, shown as the placeholder and the picker's starting
 * point for an unset colour.
 */
export const COLOR_GROUPS: {
  title: string;
  colors: { key: ColorKey; label: string; builtin: string }[];
}[] = [
  {
    title: "Surface",
    colors: [
      { key: "background", label: "Background", builtin: "transparent" },
      { key: "text", label: "Text", builtin: "#111111" },
      { key: "muted", label: "Secondary text", builtin: "#6b7280" },
      { key: "border", label: "Border", builtin: "#e5e7eb" },
    ],
  },
  {
    title: "Inputs",
    colors: [
      { key: "input_background", label: "Input background", builtin: "#ffffff" },
      { key: "input_border", label: "Input border", builtin: "#d1d5db" },
    ],
  },
  {
    title: "Accent",
    colors: [
      { key: "accent", label: "Buttons & accent", builtin: "#111827" },
      { key: "accent_text", label: "Button text", builtin: "#ffffff" },
      { key: "link", label: "Links", builtin: "#1d4ed8" },
      { key: "rating", label: "Star rating", builtin: "#f0a030" },
    ],
  },
  {
    title: "Feedback",
    colors: [{ key: "error", label: "Errors", builtin: "#b91c1c" }],
  },
];

export const FONT_PRESETS: { label: string; value: string }[] = [
  { label: "Built-in", value: "" },
  { label: "Sans-serif", value: 'system-ui, "Segoe UI", Roboto, Helvetica, Arial, sans-serif' },
  { label: "Serif", value: 'Georgia, "Times New Roman", serif' },
  { label: "Monospace", value: "ui-monospace, Menlo, Consolas, monospace" },
];

export const FONT_SIZES: { value: FontSize; title: string; description: string }[] = [
  { value: "small", title: "Small", description: "90% of the built-in type scale." },
  { value: "medium", title: "Medium", description: "The built-in type scale." },
  { value: "large", title: "Large", description: "112.5% of the built-in type scale." },
];

export const DENSITIES: { value: Density; title: string; description: string }[] = [
  { value: "compact", title: "Compact", description: "Tighter gaps and padding, for long forms." },
  { value: "comfortable", title: "Comfortable", description: "The built-in spacing." },
  { value: "spacious", title: "Spacious", description: "More room between and inside fields." },
];

/**
 * The settings exactly as the server will store and return them: unset and
 * default members dropped (never written as `null`), hex lower-cased, the font
 * trimmed, and keys in server order. Used both for the save payload and for
 * the dirty check, so the two can't disagree.
 */
export function canonicalSettings(s: ThemeSettings): ThemeSettings {
  const out: ThemeSettings = {};
  const colors: ThemeColors = {};
  for (const key of COLOR_KEYS) {
    const value = s.colors?.[key]?.trim().toLowerCase();
    if (value) colors[key] = value;
  }
  if (Object.keys(colors).length > 0) out.colors = colors;
  if (s.radius != null) out.radius = s.radius;
  const font = s.font_family?.trim();
  if (font) out.font_family = font;
  if (s.font_size && s.font_size !== "medium") out.font_size = s.font_size;
  if (s.density && s.density !== "comfortable") out.density = s.density;
  return out;
}

/** Set or (with `undefined`) delete one colour, dropping an emptied `colors`. */
export function withColor(
  s: ThemeSettings,
  key: ColorKey,
  value: string | undefined,
): ThemeSettings {
  const colors: ThemeColors = { ...(s.colors ?? {}) };
  if (value === undefined) delete colors[key];
  else colors[key] = value;
  const next: ThemeSettings = { ...s, colors };
  if (Object.keys(colors).length === 0) delete next.colors;
  return next;
}

/** Set a top-level member, deleting it for `undefined` or an empty string. */
export function withSetting<K extends keyof ThemeSettings>(
  s: ThemeSettings,
  key: K,
  value: ThemeSettings[K] | undefined,
): ThemeSettings {
  const next: ThemeSettings = { ...s };
  if (value === undefined || value === null || value === "") delete next[key];
  else next[key] = value;
  return next;
}

/**
 * `<input type="color">` only accepts `#rrggbb`: expand short forms, drop an
 * alpha channel, and start an unset or transparent colour from white.
 */
export function toPickerValue(value: string): string {
  if (!isHexColor(value)) return "#ffffff";
  const hex = value.slice(1).toLowerCase();
  if (hex.length === 3 || hex.length === 4) {
    return `#${hex[0]}${hex[0]}${hex[1]}${hex[1]}${hex[2]}${hex[2]}`;
  }
  return `#${hex.slice(0, 6)}`;
}
