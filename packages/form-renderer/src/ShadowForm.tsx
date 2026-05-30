import { useEffect, useRef } from "react";
import { createRoot, type Root } from "react-dom/client";
import { StrictMode } from "react";
import { Form, type FormProps } from "./Form";
import styles from "./styles.css?inline";

// Renders <Form> inside an open shadow root, styled by the shared stylesheet via
// a constructable stylesheet — byte-for-byte the same isolation the embed SDK
// gives host pages. Usable from any React tree (e.g. the admin preview page) so
// what you see here is exactly what an embedded form looks like in the wild.
export function ShadowForm(props: FormProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const rootRef = useRef<Root | null>(null);

  // Attach the shadow root + React root once, when the host element mounts.
  // Written to be idempotent: React StrictMode runs effects twice in dev, and
  // attachShadow throws if a shadow tree already exists, so reuse what's there.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const shadow = host.shadowRoot ?? host.attachShadow({ mode: "open" });
    if (shadow.adoptedStyleSheets.length === 0) {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(styles);
      shadow.adoptedStyleSheets = [sheet];
    }
    let container = shadow.firstElementChild as HTMLElement | null;
    if (!container) {
      container = document.createElement("div");
      shadow.appendChild(container);
    }
    rootRef.current = createRoot(container);

    return () => {
      rootRef.current?.unmount();
      rootRef.current = null;
    };
  }, []);

  // Re-render the inner root whenever props change (formId, apiUrl, callbacks).
  useEffect(() => {
    rootRef.current?.render(
      <StrictMode>
        <Form {...props} />
      </StrictMode>,
    );
  });

  return <div ref={hostRef} />;
}
