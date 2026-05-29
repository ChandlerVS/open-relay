import { mount } from "./mount";

// Host pages embed via:
//   <script src=".../open-relay.js"
//           data-form-id="abc123"
//           data-api-url="https://api.openrelay.io"></script>
//
// We read attributes off the currently-executing <script>, then insert a
// sibling <div> right after it and mount the form inside a Shadow DOM so the
// host's CSS can't bleed in.

const script = document.currentScript as HTMLScriptElement | null;
if (script) {
  const formId = script.getAttribute("data-form-id");
  const apiUrl = script.getAttribute("data-api-url") ?? window.location.origin;
  if (formId) {
    const host = document.createElement("div");
    host.setAttribute("data-open-relay-host", formId);
    script.parentNode?.insertBefore(host, script.nextSibling);
    mount(host, { formId, apiUrl });
  } else {
    console.warn("[open-relay] missing data-form-id on <script> tag");
  }
}
