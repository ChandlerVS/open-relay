import { useEffect, useMemo, useState } from "react";
import { STANDARD_FIELDS } from "./standardFields";
import type { CustomField, PublicFormDto } from "./schema";

export type FormTheme = "light" | "dark" | "auto";

export interface FormProps {
  formId: string;
  apiUrl: string;
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
  theme = "light",
  source,
  onSubmitted,
  onError,
}: FormProps) {
  const resolvedTheme = useResolvedTheme(theme);
  const [schema, setSchema] = useState<PublicFormDto | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [error, setError] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, string | boolean>>({});

  useEffect(() => {
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
        setSchema(data);
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
  }, [formId, apiUrl]);

  const enabledStandard = useMemo(() => {
    if (!schema) return [];
    return STANDARD_FIELDS.filter(
      (def) => schema.standard_fields[def.key]?.enabled,
    );
  }, [schema]);

  const orderedCustom = useMemo(() => {
    if (!schema) return [];
    return [...schema.custom_fields].sort((a, b) => a.position - b.position);
  }, [schema]);

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
        e.preventDefault();
        void submit();
      }}
    >
      <h2 className="or-form__title">{schema.name}</h2>
      <div className="or-form__fields">
        {enabledStandard.map((def) => {
          const cfg = schema.standard_fields[def.key]!;
          const label = (cfg.label && cfg.label.trim()) || def.default_label;
          const id = `or-${schema.id}-${def.key}`;
          const common = {
            id,
            name: def.key,
            required: cfg.required,
            autoComplete: def.autocomplete,
            value: String(values[def.key] ?? ""),
            onChange: (
              e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>,
            ) => set(def.key, e.target.value),
          };
          return (
            <div key={def.key} className="or-field">
              <label htmlFor={id} className="or-field__label">
                {label}
                {cfg.required && <span className="or-field__required"> *</span>}
              </label>
              {def.input_type === "textarea" ? (
                <textarea {...common} rows={4} />
              ) : (
                <input type={def.input_type} {...common} />
              )}
            </div>
          );
        })}
        {orderedCustom.map((field) => (
          <CustomFieldInput
            key={field.key}
            field={field}
            value={values[field.key]}
            onChange={(v) => set(field.key, v)}
            scope={schema.id}
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
      <button
        type="submit"
        className="or-form__submit"
        disabled={status === "submitting"}
      >
        {status === "submitting" ? "Submitting…" : "Submit"}
      </button>
    </form>
  );
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
    <div className="or-field">
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
