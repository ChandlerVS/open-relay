import type { FormTheme } from "@open-relay/form-renderer";
import { mount } from "./mount";

// Host pages embed via:
//   <script src=".../open-relay.js"
//           data-form-id="abc123"
//           data-api-url="https://api.openrelay.io"
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

function readTheme(raw: string | null): FormTheme {
  return raw === "dark" || raw === "auto" ? raw : "light";
}

const script = document.currentScript as HTMLScriptElement | null;
if (script) {
  const formId = script.getAttribute("data-form-id");
  const apiUrl = script.getAttribute("data-api-url") ?? window.location.origin;
  const theme = readTheme(script.getAttribute("data-theme"));
  if (formId) {
    const host = document.createElement("div");
    host.setAttribute("data-open-relay-host", formId);
    script.parentNode?.insertBefore(host, script.nextSibling);
    mount(host, { formId, apiUrl, theme });
  } else {
    console.warn("[open-relay] missing data-form-id on <script> tag");
  }
}
