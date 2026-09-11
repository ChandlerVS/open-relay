import { CUSTOM_FIELD_TYPES, type FormElement } from "./model";

/**
 * Transport for the builder's clipboard.
 *
 * The payload is a JSON envelope carried as **`text/plain`**, not a custom MIME
 * type. Custom types are unreliable in Safari, whereas plain text crosses every
 * tab, window and profile boundary the system clipboard does — which is the
 * whole point, since copying a block out of one form and pasting it into
 * another happens across two page loads. It also means a block can be pasted
 * into a scratch file and mailed to someone, which is a free export path.
 *
 * `localStorage` mirrors the same string because reading the clipboard is the
 * half browsers guard: `navigator.clipboard.readText()` is not exposed to page
 * content in Firefox and needs a gesture plus a native prompt in Safari, and
 * neither works at all in a non-secure context. The mirror is what makes the
 * toolbar's Paste button work there.
 */

const ENVELOPE = "openRelayFormElements";
const VERSION = 1;
/** Matches the `open-relay:<name>:v1` convention in `lib/auth/storage.ts`. */
const LOCAL_KEY = "open-relay:builder-clipboard:v1";

/** Every `FormElement` discriminant, so a foreign payload is refused up front. */
const KINDS: ReadonlySet<string> = new Set([
  "standard",
  "custom",
  "heading",
  "paragraph",
  "rich_text",
  "divider",
  "page_break",
  "row_start",
  "row_end",
]);

const CUSTOM_TYPES: ReadonlySet<string> = new Set(CUSTOM_FIELD_TYPES.map((t) => t.type));

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function isStr(v: unknown): boolean {
  return typeof v === "string";
}

/**
 * Shallow per-variant check on the members the builder dereferences *without*
 * guarding — `validate.ts` does `el.config.key.trim()` and
 * `el.config.label.trim()` straight off the union, so a truncated or
 * hand-edited payload would white-screen the canvas on the next render.
 *
 * Deliberately not a full schema. The generated OpenAPI types already are one,
 * and restating them here (in zod or by hand) would be a second copy to keep in
 * step with the server. Anything past these required scalars is the inline
 * validation's job, exactly as it is for an element the palette created.
 */
function wellFormed(el: unknown): el is FormElement {
  if (!isRecord(el)) return false;
  const kind = el["element"];
  if (!isStr(kind) || !KINDS.has(kind as string)) return false;
  // The two unit variants carry no `config` at all.
  if (kind === "divider" || kind === "row_end") return true;
  const cfg = el["config"];
  if (!isRecord(cfg)) return false;
  switch (kind) {
    case "standard":
      return isStr(cfg["key"]);
    case "custom":
      // The type has to be one we can actually draw: `CustomFieldInput` ends in
      // a `never` exhaustiveness guard, so an unknown variant is a crash.
      return isStr(cfg["key"]) && isStr(cfg["label"]) && CUSTOM_TYPES.has(cfg["type"] as string);
    case "heading":
    case "paragraph":
      return isStr(cfg["text"]);
    case "rich_text":
      return isStr(cfg["markdown"]);
    default:
      // `page_break` and `row_start` carry only optional members.
      return true;
  }
}

export function serializeElements(elements: FormElement[]): string {
  return JSON.stringify({ [ENVELOPE]: VERSION, elements });
}

/** `null` for anything that isn't one of our payloads — an ordinary text paste. */
export function parseElements(text: string): FormElement[] | null {
  if (!text.includes(ENVELOPE)) return null;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return null;
  }
  if (!isRecord(raw) || raw[ENVELOPE] !== VERSION) return null;
  const elements = raw["elements"];
  if (!Array.isArray(elements) || elements.length === 0) return null;
  if (!elements.every(wellFormed)) return null;
  return elements as FormElement[];
}

export function writeLocalClipboard(text: string): void {
  try {
    window.localStorage.setItem(LOCAL_KEY, text);
  } catch {
    // Quota, or Safari private browsing, which throws on write. The system
    // clipboard still has the payload; only the fallback is lost.
  }
}

/**
 * Read the mirror. Deliberately *not* cached in state at mount: a `storage`
 * event never fires in the tab that wrote it, so a cached copy would leave the
 * second tab — the one doing the pasting — looking at nothing.
 */
export function readLocalClipboard(): FormElement[] | null {
  try {
    const raw = window.localStorage.getItem(LOCAL_KEY);
    return raw ? parseElements(raw) : null;
  } catch {
    return null;
  }
}
