import type { FormTheme } from "@open-relay/form-renderer";
import { mount } from "./mount";

// Host pages embed via:
//   <script src=".../open-relay.js"
//           data-form-id="abc123"
//           data-api-url="https://api.openrelay.io/api/v1"
//           data-theme="dark"></script>
//
// We read attributes off the currently-executing <script>, then insert a
// sibling <div> right after it and mount the form inside a Shadow DOM so the
// host's CSS can't bleed in.
//
// data-theme is optional and defaults to "light" (a static light theme — we do
// NOT follow the visitor's OS preference unless asked). Pass "dark" to force a
// dark palette, or "auto" to opt into `prefers-color-scheme` (tracks OS changes
// live).
//
// To make the form match the host site, set the public `--or-*` custom
// properties (colors, `--or-font`, `--or-radius`) on the host page — they
// inherit across the Shadow DOM boundary. See packages/form-renderer/styles.css.
//
// The host page's URL query string is forwarded with the submission as
// `source` — this is how a sales-rep QR code (`?rep=jane&event=mjbiz-2026`)
// attributes the lead. The server keeps only the params it recognises.

function readTheme(raw: string | null): FormTheme {
  return raw === "dark" || raw === "auto" ? raw : "light";
}

// Snapshot the current page's query params. Captured once at load (the embed is
// rendered immediately), so a later client-side route change won't affect an
// already-mounted form.
function readSource(): Record<string, string> {
  const out: Record<string, string> = {};
  try {
    new URLSearchParams(window.location.search).forEach((value, key) => {
      out[key] = value;
    });
  } catch {
    // Malformed query string — fall through with whatever we collected.
  }
  return out;
}

const script = document.currentScript as HTMLScriptElement | null;
if (script) {
  const formId = script.getAttribute("data-form-id");
  // The renderer appends `/public/forms/…` to this base. The public API lives
  // under `/api/v1`, so the same-origin fallback (used when the snippet omits
  // data-api-url) must include that prefix to resolve correctly.
  const apiUrl = script.getAttribute("data-api-url") ?? `${window.location.origin}/api/v1`;
  const theme = readTheme(script.getAttribute("data-theme"));
  if (formId) {
    const host = document.createElement("div");
    host.setAttribute("data-open-relay-host", formId);
    script.parentNode?.insertBefore(host, script.nextSibling);
    mount(host, { formId, apiUrl, theme, source: readSource() });
  } else {
    console.warn("[open-relay] missing data-form-id on <script> tag");
  }
}
