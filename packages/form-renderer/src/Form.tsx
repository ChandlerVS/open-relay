import { useEffect, useMemo, useRef, useState } from "react";
import { STANDARD_FIELDS } from "./standardFields";
import { COUNTRIES, subdivisionsFor, type RegionOption } from "./regions";
import { UploadError, localRejection, uploadFile } from "./uploads";
import { groupRows, resolveLayout, splitIntoPages, stateBindings } from "./layout";
import type { LayoutEntry } from "./layout";
import { computeVisibility, visibleElements, type FieldValue } from "./visibility";
import { Markdown, richTextClass } from "./RichText";
import { isHttpUrl } from "./url";
import { themeStyle } from "./theme";
import type {
  CustomField,
  FieldWidth,
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
  /** API root with any trailing slash removed — shared by all three fetches. */
  const base = useMemo(
    () => (apiUrl.endsWith("/") ? apiUrl.slice(0, -1) : apiUrl),
    [apiUrl],
  );
  const [fetched, setFetched] = useState<PublicFormDto | null>(null);
  const [status, setStatus] = useState<Status>(schemaProp ? "ready" : "loading");
  const [error, setError] = useState<string | null>(null);
  const [values, setValues] = useState<Record<string, FieldValue>>({});
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
  }, [formId, base, schemaProp]);

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
      // A group's answer is an array; a stale string default (left by a
      // retype, say) must never land in its state.
      if (el.element === "custom" && el.config.type === "checkboxes") continue;
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

  // Defaults merged in, not raw state: the prefill above is an effect, so on
  // the first committed frame `values` is still empty and a controller with a
  // `default_value` would read as unanswered — flashing its dependents into
  // view one frame later. Every read that asks "what does that other field
  // say?" goes through this, which is both rules and the state pickers'
  // country lookup: a country with a default would otherwise draw its state as
  // a text box for one frame before swapping to a dropdown.
  const resolved = useMemo(
    () => ({ ...defaultValues, ...values }),
    [defaultValues, values],
  );

  const { visible, hiddenKeys } = useMemo(
    () => computeVisibility(layout, resolved),
    [layout, resolved],
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

  // Layout-derived, so neither lookup costs anything per keystroke.
  //
  // This and `uploads` below sit above the early returns because they are
  // hooks: React counts them per render, and a hook that only runs once
  // `status` leaves "loading" changes that count mid-lifecycle.
  const bindings = useMemo(() => stateBindings(layout), [layout]);


  // Upload progress lives *beside* `values`, not in it. The value of a file
  // field is the receipt string the server sealed, so `values` holds nothing
  // but answers and every consumer of it — visibility rules, `hiddenKeys`, the
  // subtractive payload build — never has to skip over progress state.
  const [uploads, setUploads] = useState<Record<string, UploadState>>({});

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
        style={themeStyle(schema?.theme)}
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
        style={themeStyle(schema?.theme)}
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

  const uploading = Object.values(uploads).some((u) => u.status === "uploading");

  const onPickFile = async (field: CustomField, file: File | null, input: HTMLInputElement) => {
    if (!file) {
      setUploads((u) => {
        const next = { ...u };
        delete next[field.key];
        return next;
      });
      set(field.key, "");
      return;
    }
    const accept = field.type === "file" ? field.accept : undefined;
    const maxSizeMb = field.type === "file" ? field.max_size_mb : undefined;
    const rejection = localRejection(file, accept, maxSizeMb);
    if (rejection) {
      // Clear the input too, not just the value: native `required` checks
      // `files.length`, so leaving the rejected file selected would let an
      // incomplete form pass client-side validation and 400 at the server.
      input.value = "";
      set(field.key, "");
      setUploads((u) => ({ ...u, [field.key]: { name: file.name, status: "error", error: rejection } }));
      return;
    }
    setUploads((u) => ({ ...u, [field.key]: { name: file.name, status: "uploading" } }));
    try {
      const token = await uploadFile(base, formId, field.key, file);
      set(field.key, token);
      setUploads((u) => ({ ...u, [field.key]: { name: file.name, status: "done" } }));
    } catch (err) {
      input.value = "";
      set(field.key, "");
      setUploads((u) => ({
        ...u,
        [field.key]: {
          name: file.name,
          status: "error",
          error: err instanceof UploadError ? err.message : "Upload failed. Please try again.",
        },
      }));
    }
  };

  const set = (key: string, val: FieldValue) =>
    setValues((v) => {
      const next = { ...v, [key]: val };
      // See `stateBindings`: a stale subdivision under a new country is a
      // pairing the server rejects, and the field would look answered.
      for (const dependent of bindings.byCountry.get(key) ?? []) next[dependent] = "";
      return next;
    });

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
    // Subtractive, never a whitelist: `values` also carries the honeypot `_hp`,
    // which the server reads to reject bots. Rebuilding from the visible layout
    // keys would drop it silently and every bot would sail through.
    const answers: Record<string, FieldValue> = { ...values };
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
      style={themeStyle(schema.theme)}
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
          groupRows(visibleElements(page, visible)).map((node) => {
            const draw = ({ el, index }: LayoutEntry) => (
              <LayoutElement
                key={elementKey(el, index)}
                element={el}
                scope={schema.id}
                values={values}
                resolved={resolved}
                bindings={bindings}
                hiddenKeys={hiddenKeys}
                regions={schema.regions}
                onChange={set}
                uploads={uploads}
                uploadsEnabled={schema.uploads_enabled ?? false}
                onPickFile={onPickFile}
              />
            );
            if (node.kind === "element") return draw(node.entry);
            return (
              <div
                key={`row-${node.index}`}
                className="or-row"
                role="group"
                aria-label={node.config.label ?? undefined}
              >
                {node.children.map(draw)}
              </div>
            );
          })}
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
        {/* Both buttons gate on `uploading`: a receipt that hasn't arrived
            yet means the field's value is still empty, and Next would let a
            visitor step past a file they think they attached. */}
        {isLastPage ? (
          <button
            type="submit"
            className="or-form__submit"
            disabled={status === "submitting" || previewMode || uploading}
            title={previewMode ? "Disabled in preview" : undefined}
          >
            {status === "submitting"
              ? "Submitting…"
              : uploading
                ? "Uploading…"
                : "Submit"}
          </button>
        ) : (
          <button type="submit" className="or-form__submit" disabled={uploading}>
            {uploading ? "Uploading…" : "Next"}
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

/**
 * The country code a subdivision picker should list against.
 *
 * A hidden controller reads as unset, exactly as it does in `visibility.ts` —
 * and this agrees with the server, where a hidden field's answer is dropped
 * before the subdivision is ever checked. Without that, hiding a country would
 * leave its state picker showing a list for an answer nobody is sending.
 */
function resolveCountry(
  key: string | null | undefined,
  values: Record<string, FieldValue>,
  hiddenKeys: ReadonlySet<string>,
): string | undefined {
  if (!key || hiddenKeys.has(key)) return undefined;
  const raw = values[key];
  return typeof raw === "string" && raw ? raw : undefined;
}

/** Per-field upload progress. Never part of the submitted payload. */
interface UploadState {
  name: string;
  status: "uploading" | "done" | "error";
  error?: string;
}

function LayoutElement({
  element,
  scope,
  values,
  resolved,
  bindings,
  hiddenKeys,
  regions,
  onChange,
  uploads,
  uploadsEnabled,
  onPickFile,
}: {
  element: FormElement;
  scope: number;
  /** Raw state — what each control displays. */
  values: Record<string, FieldValue>;
  /** State with defaults merged in — what one field reads *about another*. */
  resolved: Record<string, FieldValue>;
  bindings: ReturnType<typeof stateBindings>;
  hiddenKeys: ReadonlySet<string>;
  regions: string | null | undefined;
  onChange: (key: string, value: FieldValue) => void;
  /** Upload progress for file fields, keyed by field key. */
  uploads: Record<string, UploadState>;
  uploadsEnabled: boolean;
  onPickFile: (field: CustomField, file: File | null, input: HTMLInputElement) => void;
}) {
  switch (element.element) {
    case "standard":
      return (
        <StandardFieldInput
          field={element.config}
          value={values[element.config.key]}
          onChange={(v) => onChange(element.config.key, v)}
          scope={scope}
          subdivisions={subdivisionsFor(
            regions,
            resolveCountry(bindings.byState.get(element.config.key), resolved, hiddenKeys),
          )}
        />
      );
    case "custom":
      return (
        <CustomFieldInput
          field={element.config}
          value={values[element.config.key]}
          onChange={(v) => onChange(element.config.key, v)}
          scope={scope}
          subdivisions={subdivisionsFor(
            regions,
            resolveCountry(bindings.byState.get(element.config.key), resolved, hiddenKeys),
          )}
          upload={uploads[element.config.key]}
          uploadsEnabled={uploadsEnabled}
          onPickFile={onPickFile}
        />
      );
    case "heading": {
      const Tag = `h${Math.min(Math.max(element.config.level, 1), 6)}` as "h2";
      return <Tag className="or-heading">{element.config.text}</Tag>;
    }
    case "paragraph":
      return <p className="or-paragraph">{element.config.text}</p>;
    case "rich_text":
      return (
        <Markdown
          source={element.config.markdown}
          className={richTextClass(element.config.tone)}
        />
      );
    case "divider":
      return <hr className="or-divider" />;
    case "page_break":
      // Consumed by splitIntoPages; never reaches a page's element list.
      return null;
    case "row_start":
    case "row_end":
      // Consumed by groupRows, which folds the pair and everything between it
      // into a single row node before anything gets here.
      return null;
    default: {
      // A layout from a newer server than this bundle. Drawing nothing is the
      // right failure: the form still renders and still submits, and the
      // server revalidates whatever comes back regardless.
      const unhandled: never = element;
      void unhandled;
      return null;
    }
  }
}

function StandardFieldInput({
  field,
  value,
  onChange,
  scope,
  subdivisions,
}: {
  field: StandardElement;
  value: FieldValue | undefined;
  onChange: (next: string) => void;
  scope: number;
  /** Set for a `state` field whose country is chosen and has subdivisions. */
  subdivisions?: readonly RegionOption[] | undefined;
}) {
  const def = STANDARD_FIELDS.find((d) => d.key === field.key);
  if (!def) return null;

  const id = `or-${scope}-${field.key}`;
  const required = field.required ?? false;
  const label = (field.label && field.label.trim()) || def.default_label;
  const asSelect = field.key === "country" && field.input_override === "select";
  // A state dropdown falls back to text whenever there is no list to draw:
  // country unanswered, country hidden, a country with no ISO subdivisions, or
  // a server too old to send the table. The server accepts free text in every
  // one of those cases, so the fallback can't produce a value it would reject.
  const stateOptions =
    field.key === "state" && field.input_override === "select" ? subdivisions : undefined;

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
      ) : stateOptions ? (
        // Submits the bare ISO 3166-2 subdivision code, which the GoHighLevel
        // backend forwards verbatim — `CA`, the form CRMs expect.
        <select {...common}>
          <option value="" disabled>
            Choose…
          </option>
          {stateOptions.map((r) => (
            <option key={r.code} value={r.code}>
              {r.name}
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

/**
 * The grid/flex class for a field's width.
 *
 * `full` gets no modifier: it is the default in both layout contexts (all six
 * grid tracks outside a row, the full flex weight inside one), so leaving the
 * class off keeps the markup identical to what a form with no widths produces.
 */
function fieldClass(width: FieldWidth | undefined, extra?: string): string {
  const modifier = width && width !== "full" ? ` or-field--${width.replace("_", "-")}` : "";
  return `or-field${modifier}${extra ? ` ${extra}` : ""}`;
}

/** Stars a rating offers when `max` is absent. Mirrors `DEFAULT_RATING_MAX`. */
const DEFAULT_RATING_MAX = 5;

/**
 * A star rating, built as a radio group with the buttons hidden behind the
 * stars. Radios are what make it behave like every other field for free:
 * `required` is a native constraint (so per-step gating sees it), arrow keys
 * move between stars, and each star has an accessible name.
 *
 * The inputs are hidden with opacity, never `display: none` — the browser
 * refuses to focus a hidden required control, and would then block submit
 * with nothing to point at.
 */
function RatingInput({
  field,
  value,
  onChange,
  scope,
}: {
  field: CustomField & { type: "rating" };
  value: FieldValue | undefined;
  onChange: (next: string) => void;
  scope: number;
}) {
  const [hover, setHover] = useState(0);
  const max = field.max ?? DEFAULT_RATING_MAX;
  const required = field.required ?? false;
  const group = `or-${scope}-${field.key}`;
  const selected = typeof value === "string" ? value.trim() : "";
  // Hovering previews a score; leaving the stars falls back to the answer.
  const lit = hover || Number(selected) || 0;
  const stars = Array.from({ length: max }, (_, i) => i + 1);

  return (
    <div className={fieldClass(field.width, "or-field--rating")}>
      <fieldset className="or-radio-group">
        <legend className="or-field__label">
          {field.label}
          {required && <span className="or-field__required"> *</span>}
        </legend>
        <div className="or-rating" onMouseLeave={() => setHover(0)}>
          {stars.map((n) => (
            <label
              key={n}
              htmlFor={`${group}-${n}`}
              className={`or-rating__star${n <= lit ? " or-rating__star--on" : ""}`}
              onMouseEnter={() => setHover(n)}
            >
              <input
                id={`${group}-${n}`}
                className="or-rating__input"
                name={group}
                type="radio"
                required={required}
                value={String(n)}
                checked={selected === String(n)}
                onChange={() => onChange(String(n))}
                aria-label={`${n} of ${max}`}
              />
              <svg className="or-rating__icon" viewBox="0 0 24 24" aria-hidden="true">
                <path d="M12 2.8l2.84 5.76 6.36.92-4.6 4.49 1.08 6.33L12 17.31l-5.68 2.99 1.08-6.33-4.6-4.49 6.36-.92z" />
              </svg>
              <span className="or-rating__num" aria-hidden="true">
                {n}
              </span>
            </label>
          ))}
        </div>
      </fieldset>
      {field.help_text && <p className="or-field__help">{field.help_text}</p>}
    </div>
  );
}

/**
 * A group of checkboxes, any number of which may be ticked. The answer is the
 * ticked options as an array, always in the author's order.
 *
 * `required` can't go on the inputs: on a checkbox it demands *that* box. So
 * "at least one" is a custom validity on the first box instead — still a native
 * constraint, so per-step gating sees it, and an unmounted (hidden) group takes
 * it away with it.
 */
function CheckboxesInput({
  field,
  value,
  onChange,
  scope,
}: {
  field: CustomField & { type: "checkboxes" };
  value: FieldValue | undefined;
  onChange: (next: string[]) => void;
  scope: number;
}) {
  const first = useRef<HTMLInputElement>(null);
  const required = field.required ?? false;
  const group = `or-${scope}-${field.key}`;
  const ticked = Array.isArray(value) ? value : [];
  const missing = required && ticked.length === 0;

  useEffect(() => {
    first.current?.setCustomValidity(missing ? "Please select at least one option." : "");
  }, [missing]);

  const toggle = (opt: string, on: boolean) =>
    onChange(field.options.filter((o) => (o === opt ? on : ticked.includes(o))));

  return (
    <div className={fieldClass(field.width, "or-field--checkboxes")}>
      <fieldset className="or-radio-group">
        <legend className="or-field__label">
          {field.label}
          {required && <span className="or-field__required"> *</span>}
        </legend>
        {field.options.map((opt, i) => (
          <label key={opt} className="or-radio-option" htmlFor={`${group}-${i}`}>
            <input
              ref={i === 0 ? first : undefined}
              id={`${group}-${i}`}
              name={group}
              type="checkbox"
              value={opt}
              checked={ticked.includes(opt)}
              onChange={(e) => toggle(opt, e.target.checked)}
            />{" "}
            {opt}
          </label>
        ))}
      </fieldset>
      {field.help_text && <p className="or-field__help">{field.help_text}</p>}
    </div>
  );
}

function CustomFieldInput({
  field,
  value,
  onChange,
  scope,
  subdivisions,
  upload,
  uploadsEnabled = false,
  onPickFile,
}: {
  field: CustomField;
  value: FieldValue | undefined;
  onChange: (next: FieldValue) => void;
  scope: number;
  /** Set for a `state` field whose country is chosen and has subdivisions. */
  subdivisions?: readonly RegionOption[] | undefined;
  upload?: UploadState | undefined;
  uploadsEnabled?: boolean;
  onPickFile?: (field: CustomField, file: File | null, input: HTMLInputElement) => void;
}) {
  const id = `or-${scope}-${field.key}`;
  const required = field.required ?? false;

  if (field.type === "checkbox") {
    return (
      <div className={fieldClass(field.width, "or-field--checkbox")}>
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

  if (field.type === "file") {
    // The input is deliberately *uncontrolled*: a file input's value can't be
    // set programmatically, and what `values` holds for this field is the
    // receipt, not the filename. `required` still works — the browser checks
    // `files.length` — which is why a failed upload clears the input as well
    // as the value.
    const accepted = field.accept && field.accept.length > 0 ? field.accept.join(",") : undefined;
    return (
      <div className={fieldClass(field.width, "or-field--file")}>
        <label htmlFor={id} className="or-field__label">
          {field.label}
          {required && <span className="or-field__required"> *</span>}
        </label>
        {uploadsEnabled ? (
          <>
            <input
              id={id}
              name={field.key}
              type="file"
              accept={accepted}
              required={required && upload?.status !== "done"}
              disabled={upload?.status === "uploading"}
              onChange={(e) => onPickFile?.(field, e.target.files?.[0] ?? null, e.target)}
            />
            {upload?.status === "uploading" && (
              <p className="or-field__help" role="status">
                Uploading {upload.name}…
              </p>
            )}
            {upload?.status === "done" && (
              <p className="or-field__help or-field__help--ok" role="status">
                Attached {upload.name}
              </p>
            )}
            {upload?.status === "error" && (
              <p className="or-field__error" role="alert">
                {upload.error}
              </p>
            )}
          </>
        ) : (
          // No storage provider is configured, so an upload cannot succeed.
          // Say so instead of letting the visitor pick a file and fail — and
          // drop `required`, which would otherwise make the form
          // uncompletable through no fault of theirs.
          <p className="or-field__error">File uploads aren't available right now.</p>
        )}
        {field.help_text && <p className="or-field__help">{field.help_text}</p>}
      </div>
    );
  }

  if (field.type === "rating") {
    return <RatingInput field={field} value={value} onChange={onChange} scope={scope} />;
  }

  if (field.type === "checkboxes") {
    return <CheckboxesInput field={field} value={value} onChange={onChange} scope={scope} />;
  }

  if (field.type === "radio") {
    // `name` groups the buttons, so it has to be unique to this form instance —
    // elsewhere in this file it is decorative (values are React state, never
    // FormData). `required` on every member is satisfied by any one of them
    // being checked, which is the per-group behaviour we want.
    const group = `or-${scope}-${field.key}`;
    return (
      <div className={fieldClass(field.width, "or-field--radio")}>
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

  const choices = (options: readonly RegionOption[] | readonly string[]) => (
    <select {...inputProps}>
      <option value="" disabled>
        Choose…
      </option>
      {options.map((o) =>
        typeof o === "string" ? (
          <option key={o} value={o}>
            {o}
          </option>
        ) : (
          <option key={o.code} value={o.code}>
            {o.name}
          </option>
        ),
      )}
    </select>
  );

  // A switch rather than a ternary chain so the `never` below is reachable:
  // without it a new variant on the `CustomField` union silently renders
  // `<input type="whatever">`, which is how `country` and `state` would have
  // failed — quietly, and only in a visitor's browser.
  let control: React.ReactNode;
  switch (field.type) {
    case "textarea":
      control = <textarea {...inputProps} rows={4} />;
      break;
    case "select":
      control = choices(field.options);
      break;
    case "country":
      // Submits the ISO alpha-2 code. The list is in the bundle; only the
      // subdivision table has to come from the server.
      control = choices(COUNTRIES);
      break;
    case "state":
      // Falls back to text whenever there is no list to draw: country
      // unanswered, country hidden, a country with no ISO subdivisions, an
      // unbound field, or a server too old to send the table. The server
      // accepts free text in every one of those cases, so this can't produce a
      // value it would reject.
      control = subdivisions ? (
        choices(subdivisions)
      ) : (
        <input type="text" autoComplete="address-level1" {...inputProps} />
      );
      break;
    case "text":
    case "email":
    case "number":
    case "tel":
    case "url":
      // The type is the HTML input type, which is why these need no branch of
      // their own.
      control = <input type={field.type} {...inputProps} />;
      break;
    default: {
      const unhandled: never = field;
      void unhandled;
      control = null;
    }
  }

  return (
    <div className={fieldClass(field.width)}>
      <label htmlFor={id} className="or-field__label">
        {field.label}
        {required && <span className="or-field__required"> *</span>}
      </label>
      {control}
      {field.help_text && <p className="or-field__help">{field.help_text}</p>}
    </div>
  );
}
