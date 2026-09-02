import { useEffect, useMemo, useState } from "react";
import { COUNTRIES, STANDARD_FIELDS } from "./standardFields";
import { resolveLayout, splitIntoPages } from "./layout";
import type { CustomField, FormElement, PublicFormDto, StandardElement } from "./schema";

export type FormTheme = "light" | "dark" | "auto";

export interface FormProps {
  formId: string;
  apiUrl: string;
  /**
   * Render this schema instead of fetching one. Lets the admin builder preview
   * unsaved edits through the same component the embed uses, so what you see
   * is what a host page gets.
   */
  schema?: PublicFormDto;
  /**
   * Render read-only: submission is disabled. The builder preview sets this so
   * experimenting with a form can't create real submissions and fire real
   * backend deliveries.
   */
  previewMode?: boolean;
  /**
   * Color theme. "light" (the default) is a static light palette. Pass "dark"
   * to force dark, or "auto" to opt into the host's `prefers-color-scheme` and
   * track OS changes live.
   */
  theme?: FormTheme;
  /**
   * Source context captured from the host page's URL query string (e.g. a QR
   * code's `?rep=jane&event=mjbiz-2026`). Forwarded with the submission under
   * the reserved `_source` key; the server keeps only the params it recognises
   * (the rep + the form's configured source params) and drops the rest.
   */
  source?: Record<string, string>;
  /** Fired after a submission is accepted, with the new submission id. */
  onSubmitted?: (result: { id: number }) => void;
  /** Fired when submission fails, with a human-readable message. */
  onError?: (message: string) => void;
}

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

// Resolves "auto" against the OS preference, tracking live changes. An explicit
// "light"/"dark" wins and skips the media listener.
function useResolvedTheme(theme: FormTheme): "light" | "dark" {
  const [systemDark, setSystemDark] = useState(prefersDark);
  useEffect(() => {
    if (theme !== "auto" || typeof window.matchMedia !== "function") return;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => setSystemDark(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [theme]);
  if (theme === "auto") return systemDark ? "dark" : "light";
  return theme;
}

type Status =
  | "loading"
  | "ready"
  | "submitting"
  | "submit_error"
  | "error"
  | "submitted";

export function Form({
  formId,
  apiUrl,
  schema: schemaProp,
  previewMode = false,
  theme = "light",
  source,
  onSubmitted,
  onError,
}: FormProps) {
  const resolvedTheme = useResolvedTheme(theme);
  const [fetched, setFetched] = useState<PublicFormDto | null>(null);
  const [status, setStatus] = useState<Status>(schemaProp ? "ready" : "loading");
  const [error, setError] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, string | boolean>>({});
  const [pageIndex, setPageIndex] = useState(0);

  const schema = schemaProp ?? fetched;

  useEffect(() => {
    // A caller-supplied schema short-circuits the fetch entirely. Flip out of
    // the initial "loading" state in case the prop arrived after mount.
    if (schemaProp) {
      setStatus((prev) => (prev === "loading" ? "ready" : prev));
      return;
    }
    let cancelled = false;
    setStatus("loading");
    setError(null);
    const base = apiUrl.endsWith("/") ? apiUrl.slice(0, -1) : apiUrl;
    fetch(`${base}/public/forms/${encodeURIComponent(formId)}`)
      .then(async (r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return (await r.json()) as PublicFormDto;
      })
      .then((data) => {
        if (cancelled) return;
        setFetched(data);
        setStatus("ready");
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        setError(err instanceof Error ? err.message : String(err));
        setStatus("error");
      });
    return () => {
      cancelled = true;
    };
  }, [formId, apiUrl, schemaProp]);

  const pages = useMemo(
    () => (schema ? splitIntoPages(resolveLayout(schema)) : []),
    [schema],
  );

  // Prefill from each field's default_value, without clobbering anything the
  // visitor has already typed.
  useEffect(() => {
    if (!schema) return;
    const defaults: Record<string, string> = {};
    for (const el of resolveLayout(schema)) {
      if (el.element !== "standard" && el.element !== "custom") continue;
      const dv = el.config.default_value;
      if (dv) defaults[el.config.key] = dv;
    }
    if (Object.keys(defaults).length === 0) return;
    setValues((v) => ({ ...defaults, ...v }));
  }, [schema]);

  // A layout can shrink between renders (the builder preview edits live), so
  // never leave the step cursor past the end.
  const safePageIndex = Math.min(pageIndex, Math.max(pages.length - 1, 0));

  if (status === "loading") {
    return (
      <div
        data-open-relay-form={formId}
        data-theme={resolvedTheme}
        className="or-form or-form--loading"
      >
        Loading form…
      </div>
    );
  }
  if (status === "error" || !schema) {
    return (
      <div
        data-open-relay-form={formId}
        data-theme={resolvedTheme}
        className="or-form or-form--error"
      >
        Couldn't load this form{error ? ` (${error})` : ""}.
      </div>
    );
  }
  if (status === "submitted") {
    return (
      <div
        data-open-relay-form={formId}
        data-theme={resolvedTheme}
        className="or-form or-form--submitted"
      >
        Thanks — we've received your submission.
      </div>
    );
  }

  const set = (key: string, val: string | boolean) =>
    setValues((v) => ({ ...v, [key]: val }));

  const page = pages[safePageIndex];
  const isLastPage = safePageIndex >= pages.length - 1;

  const submit = async () => {
    setStatus("submitting");
    setError(null);
    const base = apiUrl.endsWith("/") ? apiUrl.slice(0, -1) : apiUrl;
    const body =
      source && Object.keys(source).length > 0
        ? { ...values, _source: source }
        : values;
    try {
      const res = await fetch(
        `${base}/public/forms/${encodeURIComponent(formId)}/submissions`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        },
      );
      if (!res.ok) {
        let message = `HTTP ${res.status}`;
        try {
          const body = (await res.json()) as { error?: string };
          if (body.error) message = body.error;
        } catch {
          // ignore: server returned non-JSON
        }
        throw new Error(message);
      }
      let accepted: { id: number } | null = null;
      try {
        accepted = (await res.json()) as { id: number };
      } catch {
        // ignore: success without a parseable body
      }
      setStatus("submitted");
      if (accepted) onSubmitted?.(accepted);
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      setError(message);
      setStatus("submit_error");
      onError?.(message);
    }
  };

  return (
    <form
      data-open-relay-form={formId}
      data-theme={resolvedTheme}
      className="or-form"
      onSubmit={(e) => {
        // Browser-native validation has already passed for the fields
        // currently in the DOM, i.e. this step only — which is exactly the
        // per-step gate we want. Fields on later pages aren't mounted yet.
        e.preventDefault();
        if (!isLastPage) {
          setPageIndex(safePageIndex + 1);
          return;
        }
        if (previewMode) return;
        void submit();
      }}
    >
      <h2 className="or-form__title">{schema.name}</h2>
      {pages.length > 1 && (
        <div className="or-form__steps">
          <span className="or-form__step-count">
            Step {safePageIndex + 1} of {pages.length}
          </span>
          {page?.title && <span className="or-form__step-title">{page.title}</span>}
        </div>
      )}
      <div className="or-form__fields">
        {page?.elements.map((el, i) => (
          <LayoutElement
            key={elementKey(el, i)}
            element={el}
            scope={schema.id}
            values={values}
            onChange={set}
          />
        ))}
        {/*
          Honeypot: a hidden field a real user never sees or tabs to, but many
          bots auto-fill. The server rejects a submission whose `_hp` is set.
          Hidden inline (not via CSS class) so it works even if the host page
          strips our stylesheet.
        */}
        <div
          aria-hidden="true"
          style={{
            position: "absolute",
            left: "-9999px",
            width: "1px",
            height: "1px",
            overflow: "hidden",
          }}
        >
          <label htmlFor={`or-${schema.id}-_hp`}>Leave this field empty</label>
          <input
            id={`or-${schema.id}-_hp`}
            name="_hp"
            type="text"
            tabIndex={-1}
            autoComplete="off"
            value={String(values["_hp"] ?? "")}
            onChange={(e) => set("_hp", e.target.value)}
          />
        </div>
      </div>
      {status === "submit_error" && error && (
        <div className="or-form__error" role="alert">
          {error}
        </div>
      )}
      <div className="or-form__actions">
        {safePageIndex > 0 && (
          <button
            type="button"
            className="or-form__back"
            onClick={() => setPageIndex(safePageIndex - 1)}
          >
            Back
          </button>
        )}
        {isLastPage ? (
          <button
            type="submit"
            className="or-form__submit"
            disabled={status === "submitting" || previewMode}
            title={previewMode ? "Disabled in preview" : undefined}
          >
            {status === "submitting" ? "Submitting…" : "Submit"}
          </button>
        ) : (
          <button type="submit" className="or-form__submit">
            Next
          </button>
        )}
      </div>
    </form>
  );
}

/** Stable-ish React key: field elements key by their submission key, decoration by index. */
function elementKey(el: FormElement, index: number): string {
  if (el.element === "standard" || el.element === "custom") return el.config.key;
  return `${el.element}-${index}`;
}

function LayoutElement({
  element,
  scope,
  values,
  onChange,
}: {
  element: FormElement;
  scope: number;
  values: Record<string, string | boolean>;
  onChange: (key: string, value: string | boolean) => void;
}) {
  switch (element.element) {
    case "standard":
      return (
        <StandardFieldInput
          field={element.config}
          value={values[element.config.key]}
          onChange={(v) => onChange(element.config.key, v)}
          scope={scope}
        />
      );
    case "custom":
      return (
        <CustomFieldInput
          field={element.config}
          value={values[element.config.key]}
          onChange={(v) => onChange(element.config.key, v)}
          scope={scope}
        />
      );
    case "heading": {
      const Tag = `h${Math.min(Math.max(element.config.level, 1), 6)}` as "h2";
      return <Tag className="or-heading">{element.config.text}</Tag>;
    }
    case "paragraph":
      return <p className="or-paragraph">{element.config.text}</p>;
    case "divider":
      return <hr className="or-divider" />;
    case "page_break":
      // Consumed by splitIntoPages; never reaches a page's element list.
      return null;
  }
}

function StandardFieldInput({
  field,
  value,
  onChange,
  scope,
}: {
  field: StandardElement;
  value: string | boolean | undefined;
  onChange: (next: string) => void;
  scope: number;
}) {
  const def = STANDARD_FIELDS.find((d) => d.key === field.key);
  if (!def) return null;

  const id = `or-${scope}-${field.key}`;
  const required = field.required ?? false;
  const label = (field.label && field.label.trim()) || def.default_label;
  const asSelect = field.key === "country" && field.input_override === "select";

  const common = {
    id,
    name: field.key,
    required,
    autoComplete: def.autocomplete,
    placeholder: field.placeholder ?? undefined,
    value: String(value ?? ""),
    onChange: (
      e: React.ChangeEvent<
        HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
      >,
    ) => onChange(e.target.value),
  };

  return (
    <div className={fieldClass(field.width)}>
      <label htmlFor={id} className="or-field__label">
        {label}
        {required && <span className="or-field__required"> *</span>}
      </label>
      {asSelect ? (
        // Submits the ISO alpha-2 code, which the GoHighLevel backend's
        // normalize_country already passes through unchanged.
        <select {...common}>
          <option value="" disabled>
            Choose…
          </option>
          {COUNTRIES.map((c) => (
            <option key={c.code} value={c.code}>
              {c.name}
            </option>
          ))}
        </select>
      ) : def.input_type === "textarea" ? (
        <textarea {...common} rows={4} />
      ) : (
        <input type={def.input_type} {...common} />
      )}
      {field.help_text && <p className="or-field__help">{field.help_text}</p>}
    </div>
  );
}

function fieldClass(width: "full" | "half" | undefined): string {
  return width === "half" ? "or-field or-field--half" : "or-field";
}

function CustomFieldInput({
  field,
  value,
  onChange,
  scope,
}: {
  field: CustomField;
  value: string | boolean | undefined;
  onChange: (next: string | boolean) => void;
  scope: number;
}) {
  const id = `or-${scope}-${field.key}`;
  const required = field.required ?? false;

  if (field.type === "checkbox") {
    return (
      <div className="or-field or-field--checkbox">
        <label htmlFor={id} className="or-field__label">
          <input
            id={id}
            name={field.key}
            type="checkbox"
            required={required}
            checked={value === true}
            onChange={(e) => onChange(e.target.checked)}
          />{" "}
          {field.label}
          {required && <span className="or-field__required"> *</span>}
        </label>
        {field.help_text && <p className="or-field__help">{field.help_text}</p>}
      </div>
    );
  }

  const inputProps = {
    id,
    name: field.key,
    required,
    placeholder: field.placeholder ?? undefined,
    value: String(value ?? ""),
    onChange: (
      e: React.ChangeEvent<
        HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement
      >,
    ) => onChange(e.target.value),
  };

  return (
    <div className={fieldClass(field.width)}>
      <label htmlFor={id} className="or-field__label">
        {field.label}
        {required && <span className="or-field__required"> *</span>}
      </label>
      {field.type === "textarea" ? (
        <textarea {...inputProps} rows={4} />
      ) : field.type === "select" ? (
        <select {...inputProps}>
          <option value="" disabled>
            Choose…
          </option>
          {field.options.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      ) : (
        <input type={field.type} {...inputProps} />
      )}
      {field.help_text && <p className="or-field__help">{field.help_text}</p>}
    </div>
  );
}
