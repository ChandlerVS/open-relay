import { useEffect, useMemo, useState } from "react";
import { COUNTRIES, STANDARD_FIELDS } from "./standardFields";
import { resolveLayout, splitIntoPages } from "./layout";
import { computeVisibility, visibleElements } from "./visibility";
import type {
  CustomField,
  FormElement,
  PostSubmissionAction,
  ProgressStyle,
  PublicFormDto,
  StandardElement,
} from "./schema";

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
   * Honor a `redirect` post-submission action by rendering a description of
   * where it would go, instead of navigating. `previewMode` already implies
   * this; the separate flag is for the admin preview page, which submits for
   * real but must not navigate the admin out of its own SPA.
   */
  suppressRedirect?: boolean;
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
  suppressRedirect = false,
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

  const layout = useMemo(() => (schema ? resolveLayout(schema) : []), [schema]);

  // Pages are deliberately *not* value-dependent: a page break is unconditional,
  // so the step list is fixed and can't churn on every keystroke. Conditional
  // elements are filtered inside a page, and a page left with nothing visible is
  // skipped during navigation (see `nextVisiblePage`).
  const pages = useMemo(() => splitIntoPages(layout), [layout]);

  // Each field's default_value, keyed by field key. Shared by the initial
  // prefill and by the "submit another" reset — a reset that just cleared
  // `values` would silently drop every configured default, because the prefill
  // effect below only runs when the schema changes.
  const defaultValues = useMemo(() => {
    const defaults: Record<string, string> = {};
    if (!schema) return defaults;
    for (const el of resolveLayout(schema)) {
      if (el.element !== "standard" && el.element !== "custom") continue;
      const dv = el.config.default_value;
      if (dv) defaults[el.config.key] = dv;
    }
    return defaults;
  }, [schema]);

  // Prefill without clobbering anything the visitor has already typed.
  useEffect(() => {
    if (Object.keys(defaultValues).length === 0) return;
    setValues((v) => ({ ...defaultValues, ...v }));
  }, [defaultValues]);

  // Resolved against defaults *merged in*, not raw state: the prefill above is
  // an effect, so on the first committed frame `values` is still empty and a
  // controller with a `default_value` would read as unanswered — flashing its
  // dependents into view one frame later.
  const { visible, hiddenKeys } = useMemo(
    () => computeVisibility(layout, { ...defaultValues, ...values }),
    [layout, defaultValues, values],
  );

  // What to do once a submission lands. A server too old to know the field, or
  // a form that never configured one, gets the built-in message.
  // Memoized because it is an effect dependency and the fallback is a fresh
  // object literal on every render.
  const action: PostSubmissionAction = useMemo(
    () => schema?.post_submission_action ?? { action: "message", config: {} },
    [schema],
  );

  // How this form shows progress. An absent value — an older server, or a
  // form never configured — means the default bar, matching the server's
  // `ProgressIndicator::default()`.
  const progressStyle: ProgressStyle = schema?.progress_indicator?.style ?? "bar";
  const showPercent = schema?.progress_indicator?.show_percent ?? true;

  // Redirect as an effect, not inside `submit`, so React has committed the
  // "submitted" state before the page goes away.
  useEffect(() => {
    if (status !== "submitted") return;
    if (previewMode || suppressRedirect) return;
    if (action.action !== "redirect") return;
    // The server validates this on write, but the response is still untrusted
    // input to a third-party host page, so re-check the scheme before handing
    // it to the browser.
    if (!isHttpUrl(action.config.url)) return;
    window.location.assign(action.config.url);
  }, [status, action, previewMode, suppressRedirect]);

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
        <SubmittedPanel
          action={action}
          navigates={!previewMode && !suppressRedirect}
          onSubmitAnother={() => {
            setValues(defaultValues);
            setPageIndex(0);
            setError(null);
            setStatus("ready");
          }}
        />
      </div>
    );
  }

  const set = (key: string, val: string | boolean) =>
    setValues((v) => ({ ...v, [key]: val }));

  const page = pages[safePageIndex];
  // Steps that currently have something on them. A page whose every element is
  // conditional can empty out entirely, and stepping onto it would show a lone
  // Next button — or, on the last page, no Submit at all.
  const liveSteps = pages
    .map((p, i) => (visibleElements(p, visible).length > 0 ? i : -1))
    .filter((i) => i >= 0);
  const nextLive = liveSteps.find((i) => i > safePageIndex);
  const prevLive = [...liveSteps].reverse().find((i) => i < safePageIndex);
  const isLastPage = nextLive === undefined;

  // Both the bar and the "Step N of M" wording count only live steps, so the
  // percentage can't disagree with what the visitor is being told. Completed
  // steps, not the current one: step 1 of 4 is 0%, step 4 is 75%.
  const stepCount = Math.max(liveSteps.length, 1);
  const stepIndex = Math.max(
    liveSteps.findIndex((i) => i >= safePageIndex),
    0,
  );
  const percent = Math.round((stepIndex / stepCount) * 100);

  const submit = async () => {
    setStatus("submitting");
    setError(null);
    const base = apiUrl.endsWith("/") ? apiUrl.slice(0, -1) : apiUrl;
    // Subtractive, never a whitelist: `values` also carries the honeypot `_hp`,
    // which the server reads to reject bots. Rebuilding from the visible layout
    // keys would drop it silently and every bot would sail through.
    const answers: Record<string, string | boolean> = { ...values };
    for (const key of hiddenKeys) delete answers[key];
    const body =
      source && Object.keys(source).length > 0
        ? { ...answers, _source: source }
        : answers;
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
        if (nextLive !== undefined) {
          setPageIndex(nextLive);
          return;
        }
        if (previewMode) return;
        void submit();
      }}
    >
      <h2 className="or-form__title">{schema.name}</h2>
      {stepCount > 1 && (progressStyle === "steps" || page?.title) && (
        <div className="or-form__steps">
          {/*
            The count is style-dependent, but a page break's title names the
            step and is the only place it can appear — so it renders under
            every style, including "none".
          */}
          {progressStyle === "steps" && (
            <span className="or-form__step-count">
              Step {stepIndex + 1} of {stepCount}
            </span>
          )}
          {page?.title && <span className="or-form__step-title">{page.title}</span>}
        </div>
      )}
      <div className="or-form__fields">
        {/*
          Hidden elements are *unmounted*, never CSS-hidden. Per-step gating
          leans on native constraint validation seeing only what's in the DOM
          (see the onSubmit comment), so unmounting exempts a hidden required
          field for free — while a `display:none` required input would make the
          browser block submit on a control it refuses to focus.

          Keys come from the element's index in the whole layout, not its
          position in this filtered list, so a heading doesn't remount whenever
          a sibling appears or disappears.
        */}
        {page &&
          visibleElements(page, visible).map(({ el, index }) => (
            <LayoutElement
              key={elementKey(el, index)}
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
        {prevLive !== undefined && (
          <button
            type="button"
            className="or-form__back"
            onClick={() => setPageIndex(prevLive)}
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
      {stepCount > 1 && progressStyle === "bar" && (
        <div className="or-form__progress">
          {/*
            `role="progressbar"` sits on the track, not this wrapper, so the
            fill and the label aren't swallowed as its children. The percentage
            counts *completed* steps, so step 1 of 4 reads 0% — aria-valuetext
            carries the step wording that the bare number loses.
          */}
          <div
            className="or-form__progress-track"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            aria-valuetext={`Step ${stepIndex + 1} of ${stepCount}`}
          >
            {/* Dynamic, so it can't live in the stylesheet. */}
            <div className="or-form__progress-fill" style={{ width: `${percent}%` }} />
          </div>
          {showPercent && <span className="or-form__progress-label">{percent}%</span>}
        </div>
      )}
    </form>
  );
}

/** Stable-ish React key: field elements key by their submission key, decoration by index. */
/**
 * Copy used when a form's post-submission action leaves the field blank.
 * Exported because the admin builder shows these as input placeholders — the
 * renderer is the source of truth for what a blank actually renders as, and
 * two copies would drift.
 */
export const DEFAULT_THANKS = "Thanks — we've received your submission.";
export const DEFAULT_RESUBMIT_LABEL = "Submit another response";

/**
 * Absolute http(s) only. Mirrors `service::validate_redirect_url` on the
 * server; duplicated here because this value reaches `window.location` on a
 * page we don't own.
 */
function isHttpUrl(url: string): boolean {
  const trimmed = url.trim();
  if (trimmed !== url || url === "") return false;
  if (/[\s\u0000-\u001f\u007f]/.test(url)) return false;
  const lower = url.toLowerCase();
  const rest = lower.startsWith("https://")
    ? lower.slice(8)
    : lower.startsWith("http://")
      ? lower.slice(7)
      : null;
  if (rest === null || rest === "") return false;
  return !/^[/?#]/.test(rest);
}

/**
 * The terminal state of a form. Message copy is rendered as a text node with
 * `white-space: pre-line` — never as markup, because this draws inside
 * third-party host pages.
 */
function SubmittedPanel({
  action,
  navigates,
  onSubmitAnother,
}: {
  action: PostSubmissionAction;
  navigates: boolean;
  onSubmitAnother: () => void;
}) {
  if (action.action === "redirect") {
    // `navigates` false means a preview: say where it would have gone rather
    // than pulling the admin off their own page.
    return navigates && isHttpUrl(action.config.url) ? (
      <p className="or-form__thanks-text">Redirecting…</p>
    ) : (
      <div className="or-form__thanks">
        <p className="or-form__thanks-text">Submitted.</p>
        <p className="or-form__thanks-note">
          Would redirect to {action.config.url}
        </p>
      </div>
    );
  }

  const { message, allow_resubmit, resubmit_label } = action.config;
  return (
    <div className="or-form__thanks">
      <p className="or-form__thanks-text">{message ?? DEFAULT_THANKS}</p>
      {allow_resubmit && (
        <button type="button" className="or-form__again" onClick={onSubmitAnother}>
          {resubmit_label ?? DEFAULT_RESUBMIT_LABEL}
        </button>
      )}
    </div>
  );
}

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

  if (field.type === "radio") {
    // `name` groups the buttons, so it has to be unique to this form instance —
    // elsewhere in this file it is decorative (values are React state, never
    // FormData). `required` on every member is satisfied by any one of them
    // being checked, which is the per-group behaviour we want.
    const group = `or-${scope}-${field.key}`;
    return (
      <div className={`${fieldClass(field.width)} or-field--radio`}>
        <fieldset className="or-radio-group">
          <legend className="or-field__label">
            {field.label}
            {required && <span className="or-field__required"> *</span>}
          </legend>
          {field.options.map((opt) => (
            <label key={opt} className="or-radio-option" htmlFor={`${group}-${opt}`}>
              <input
                id={`${group}-${opt}`}
                name={group}
                type="radio"
                required={required}
                value={opt}
                checked={value === opt}
                onChange={() => onChange(opt)}
              />{" "}
              {opt}
            </label>
          ))}
        </fieldset>
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
