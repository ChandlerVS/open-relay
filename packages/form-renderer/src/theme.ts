import type { CSSProperties } from "react";
import type { ThemeColors, ThemeSettings } from "./schema";

/**
 * Turn the theme the server resolved for this form into `--or-theme-*`
 * custom properties for the form root's `style`.
 *
 * `styles.css` reads each one *beneath* the matching public token —
 * `var(--or-color-accent, var(--or-theme-accent, #111827))` — so a host page
 * that themes its embed keeps winning. Setting `--or-color-*` inline here
 * instead would silently override those host rules the moment an admin
 * assigned a theme.
 *
 * Every value is re-checked, because the API response is untrusted input on a
 * page we don't own: colours must be hex, a font family is a fixed character
 * set, radius is a bounded integer, and the two scales map to fixed numbers.
 * That rules out `url()` (a request), `var()` and declaration break-out. The
 * rules mirror `open_relay_core::themes::service`; the server's set is never
 * wider than this one, so drift can only drop a style, never inject one.
 */

/** Mirrors `open_relay_core::themes::service::MAX_RADIUS`. */
export const MAX_RADIUS = 32;
const MAX_FONT_FAMILY_LEN = 200;

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{4}|[0-9a-f]{6}|[0-9a-f]{8})$/i;
const FONT_CHARS = /^[A-Za-z0-9 ,'"_-]+$/;

const COLOR_TOKENS: Record<keyof ThemeColors, string> = {
  background: "--or-theme-background",
  text: "--or-theme-text",
  muted: "--or-theme-muted",
  border: "--or-theme-border",
  input_background: "--or-theme-input-background",
  input_border: "--or-theme-input-border",
  accent: "--or-theme-accent",
  accent_text: "--or-theme-accent-text",
  error: "--or-theme-error",
  link: "--or-theme-link",
  rating: "--or-theme-rating",
};

const FONT_SCALE: Record<string, number> = { small: 0.9, large: 1.125 };
const SPACE_SCALE: Record<string, number> = { compact: 0.75, spacious: 1.25 };

export function isHexColor(value: unknown): value is string {
  return typeof value === "string" && HEX.test(value);
}

export function isSafeFontFamily(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length <= MAX_FONT_FAMILY_LEN &&
    FONT_CHARS.test(value) &&
    value.split('"').length % 2 === 1 &&
    value.split("'").length % 2 === 1
  );
}

function scale(table: Record<string, number>, key: unknown): number | undefined {
  return typeof key === "string" && Object.prototype.hasOwnProperty.call(table, key)
    ? table[key]
    : undefined;
}

export function themeStyle(
  theme: ThemeSettings | null | undefined,
): CSSProperties | undefined {
  if (!theme || typeof theme !== "object") return undefined;
  const style: Record<string, string> = {};

  const colors: unknown = theme.colors;
  if (colors && typeof colors === "object") {
    for (const [key, token] of Object.entries(COLOR_TOKENS)) {
      const value = (colors as Record<string, unknown>)[key];
      if (isHexColor(value)) style[token] = value;
    }
  }

  const radius: unknown = theme.radius;
  if (
    typeof radius === "number" &&
    Number.isInteger(radius) &&
    radius >= 0 &&
    radius <= MAX_RADIUS
  ) {
    style["--or-theme-radius"] = `${radius}px`;
  }

  if (isSafeFontFamily(theme.font_family)) {
    style["--or-theme-font"] = theme.font_family;
  }

  const fontScale = scale(FONT_SCALE, theme.font_size);
  if (fontScale !== undefined) style["--or-theme-font-scale"] = String(fontScale);
  const spaceScale = scale(SPACE_SCALE, theme.density);
  if (spaceScale !== undefined) style["--or-theme-space-scale"] = String(spaceScale);

  return Object.keys(style).length > 0 ? (style as CSSProperties) : undefined;
}
