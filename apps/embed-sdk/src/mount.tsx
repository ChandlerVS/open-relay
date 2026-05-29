import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Form, type FormProps } from "@open-relay/form-renderer";
import styles from "./styles.css?inline";

export function mount(host: HTMLElement, props: FormProps) {
  const shadow = host.attachShadow({ mode: "open" });

  // Style the shadow root via a constructable stylesheet (no FOUC, no leakage).
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(styles);
  shadow.adoptedStyleSheets = [sheet];

  const container = document.createElement("div");
  shadow.appendChild(container);

  createRoot(container).render(
    <StrictMode>
      <Form {...props} />
    </StrictMode>,
  );
}
