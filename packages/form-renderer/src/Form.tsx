import { useEffect, useMemo, useState } from "react";
import { STANDARD_FIELDS } from "./standardFields";
import type { CustomField, PublicFormDto } from "./schema";

export interface FormProps {
  formId: string;
  apiUrl: string;
}

type Status = "loading" | "ready" | "error" | "submitted";

export function Form({ formId, apiUrl }: FormProps) {
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
      <div data-open-relay-form={formId} className="or-form or-form--loading">
        Loading form…
      </div>
    );
  }
  if (status === "error" || !schema) {
    return (
      <div data-open-relay-form={formId} className="or-form or-form--error">
        Couldn't load this form{error ? ` (${error})` : ""}.
      </div>
    );
  }
  if (status === "submitted") {
    return (
      <div data-open-relay-form={formId} className="or-form or-form--submitted">
        Thanks — we've received your submission.
      </div>
    );
  }

  const set = (key: string, val: string | boolean) =>
    setValues((v) => ({ ...v, [key]: val }));

  return (
    <form
      data-open-relay-form={formId}
      className="or-form"
      onSubmit={(e) => {
        e.preventDefault();
        // Submission endpoint lands with the submission resource — until
        // then, log so the host page can confirm wiring without seeing a
        // silent failure.
        // eslint-disable-next-line no-console
        console.log("[open-relay] submit (no-op until backend lands):", values);
        setStatus("submitted");
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
      </div>
      <button type="submit" className="or-form__submit">
        Submit
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
