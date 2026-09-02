import { Link } from "react-router-dom";
import { useState } from "react";
import { Plus } from "lucide-react";
import type { components } from "@open-relay/api-client";
import {
  DEFAULT_RESUBMIT_LABEL,
  DEFAULT_THANKS,
} from "@open-relay/form-renderer";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
} from "@open-relay/ui";
import {
  useCreateForm,
  useUpdateForm,
} from "../../../lib/forms/useFormMutations";
import type { FormDto } from "../../../lib/forms/useForms";
import { useBackendsList } from "../../../lib/backends/useBackends";
import { useRepsList } from "../../../lib/reps/useReps";

type BackendBinding = components["schemas"]["BackendBinding"];
type MetadataEntry = components["schemas"]["MetadataEntry"];
type SourceParam = components["schemas"]["SourceParam"];
type PostSubmissionAction = components["schemas"]["PostSubmissionAction"];

const EMAIL_DEDUP_KEY = "email_deduplication";

/// Read the email-deduplication toggle off a form's metadata list. Defaults to
/// off when the key is absent.
function emailDedupFromMetadata(metadata: MetadataEntry[]): boolean {
  return metadata.some((m) => m.key === EMAIL_DEDUP_KEY && m.value === true);
}

const OPEN_RELAY_KIND = "open-relay";
const openRelayBinding = (): BackendBinding => ({
  kind: OPEN_RELAY_KIND,
  instance_id: null,
});

function bindingKey(b: BackendBinding): string {
  return `${b.kind}:${b.instance_id ?? ""}`;
}

export interface FormFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existingForm?: FormDto | null;
  onSaved?: (form: FormDto) => void;
}


interface ValidationResult {
  ok: boolean;
  message?: string;
}

function validate(input: {
  name: string;
  slug: string;
  backends: BackendBinding[];
}): ValidationResult {
  if (!input.name.trim()) return { ok: false, message: "Name is required." };
  if (input.slug) {
    if (!/^[a-z0-9-]+$/.test(input.slug)) {
      return {
        ok: false,
        message:
          "Slug may only contain lowercase letters, digits, and hyphens.",
      };
    }
    if (input.slug.startsWith("-") || input.slug.endsWith("-")) {
      return { ok: false, message: "Slug cannot start or end with a hyphen." };
    }
    if (input.slug.includes("--")) {
      return { ok: false, message: "Slug cannot contain consecutive hyphens." };
    }
  }
  if (input.backends.length === 0) {
    return {
      ok: false,
      message: "Pick at least one delivery destination.",
    };
  }
  return { ok: true };
}

export function FormFormDialog({
  open,
  onOpenChange,
  existingForm,
  onSaved,
}: FormFormDialogProps) {
  const isEdit = Boolean(existingForm);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-3xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit form" : "New form"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update this form's schema. Embedded copies pick up the change on the next page load."
              : "Define a form schema. Enable the standard fields you need and add custom fields for anything else."}
          </DialogDescription>
        </DialogHeader>
        {isEdit && existingForm ? (
          <EditForm
            key={existingForm.id}
            form={existingForm}
            onSaved={(f) => {
              onSaved?.(f);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        ) : (
          <CreateForm
            onSaved={(f) => {
              onSaved?.(f);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function CreateForm({
  onSaved,
  onCancel,
}: {
  onSaved: (f: FormDto) => void;
  onCancel: () => void;
}) {
  const create = useCreateForm();
  const [name, setName] = useState("");
  const [slug, setSlug] = useState("");
  const [backends, setBackends] = useState<BackendBinding[]>([openRelayBinding()]);
  const [tags, setTags] = useState<string[]>([]);
  const [reps, setReps] = useState<number[]>([]);
  const [sourceParams, setSourceParams] = useState<SourceParam[]>([]);
  const [postSubmission, setPostSubmission] = useState<PostSubmissionAction>(
    defaultPostSubmissionAction,
  );
  const [emailDedup, setEmailDedup] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, backends });
        if (!v.ok) {
          setFormError(v.message ?? "Form has errors.");
          return;
        }
        const spErr = validateSourceParams(sourceParams);
        if (spErr) {
          setFormError(spErr);
          return;
        }
        const psErr = validatePostSubmission(postSubmission);
        if (psErr) {
          setFormError(psErr);
          return;
        }
        setFormError(null);
        create.mutate(
          {
            name: name.trim(),
            slug: slug.trim() ? slug.trim() : null,
            backends,
            tags,
            reps,
            source_params: cleanSourceParams(sourceParams),
            post_submission_action: cleanPostSubmission(postSubmission),
            metadata: [{ key: EMAIL_DEDUP_KEY, value: emailDedup }],
          },
          {
            onSuccess: (f) => onSaved(f),
            onError: (err) => setFormError(err.message),
          },
        );
      }}
      noValidate
      className="space-y-6"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't create form</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}
      <BasicsSection name={name} slug={slug} onNameChange={setName} onSlugChange={setSlug} />
      <Section
        title="Fields"
        hint="A new form starts with name and email. Arrange the rest in the field builder once it's created."
      >
        {hasGoHighLevel(backends) && <GoHighLevelKeyNotice />}
        <p className="text-sm text-muted-foreground">
          After saving you'll be taken to the field builder, where you can add,
          reorder and configure fields.
        </p>
      </Section>
      <Section
        title="Delivery destinations"
        hint="Every submission fans out to each selected backend."
      >
        <DeliveryDestinations value={backends} onChange={setBackends} />
      </Section>
      <Section
        title="Sales reps"
        hint="Reps offered on this form. A QR link's ?rep=<key> attributes the lead to one of these (owner + rep tag in GoHighLevel)."
      >
        <RepsSelector value={reps} onChange={setReps} />
      </Section>
      <Section
        title="Source parameters"
        hint="Extra URL query params to capture as tags, e.g. an event name. A QR link adds ?<param>=<value>."
      >
        <SourceParamsEditor value={sourceParams} onChange={setSourceParams} />
      </Section>
      <Section
        title="Tags"
        hint="Labels dispatched to backends with every submission. Press Enter or comma to add."
      >
        <TagsEditor value={tags} onChange={setTags} />
      </Section>
      <Section
        title="After submission"
        hint="What the visitor sees once the form is submitted."
      >
        <PostSubmissionEditor value={postSubmission} onChange={setPostSubmission} />
      </Section>
      <Section title="Deduplication">
        <DeduplicationToggle value={emailDedup} onChange={setEmailDedup} />
      </Section>
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          onClick={onCancel}
          disabled={create.isPending}
        >
          Cancel
        </Button>
        <Button type="submit" disabled={create.isPending}>
          {create.isPending ? "Creating…" : "Create form"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function EditForm({
  form,
  onSaved,
  onCancel,
}: {
  form: FormDto;
  onSaved: (f: FormDto) => void;
  onCancel: () => void;
}) {
  const update = useUpdateForm();
  const [name, setName] = useState(form.name);
  const [slug, setSlug] = useState(form.slug);
  const [backends, setBackends] = useState<BackendBinding[]>(form.backends);
  const [tags, setTags] = useState<string[]>(form.tags);
  const [reps, setReps] = useState<number[]>(form.reps);
  const [sourceParams, setSourceParams] = useState<SourceParam[]>(
    form.source_params,
  );
  const [postSubmission, setPostSubmission] = useState<PostSubmissionAction>(
    form.post_submission_action,
  );
  const [emailDedup, setEmailDedup] = useState(
    emailDedupFromMetadata(form.metadata),
  );
  const [formError, setFormError] = useState<string | null>(null);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, backends });
        if (!v.ok) {
          setFormError(v.message ?? "Form has errors.");
          return;
        }
        const spErr = validateSourceParams(sourceParams);
        if (spErr) {
          setFormError(spErr);
          return;
        }
        const psErr = validatePostSubmission(postSubmission);
        if (psErr) {
          setFormError(psErr);
          return;
        }
        setFormError(null);
        const existingKeys = new Set(form.backends.map(bindingKey));
        const nextKeys = new Set(backends.map(bindingKey));
        const backendsChanged =
          existingKeys.size !== nextKeys.size ||
          [...existingKeys].some((k) => !nextKeys.has(k));
        const tagsChanged =
          tags.join(",") !== form.tags.join(",");
        const repsChanged =
          [...reps].sort().join(",") !== [...form.reps].sort().join(",");
        const cleanedParams = cleanSourceParams(sourceParams);
        const sourceParamsChanged =
          JSON.stringify(cleanedParams) !== JSON.stringify(form.source_params);
        const cleanedAction = cleanPostSubmission(postSubmission);
        const postSubmissionChanged =
          JSON.stringify(cleanedAction) !==
          JSON.stringify(cleanPostSubmission(form.post_submission_action));
        const dedupChanged = emailDedup !== emailDedupFromMetadata(form.metadata);
        update.mutate(
          {
            id: form.id,
            input: {
              name: name.trim() !== form.name ? name.trim() : undefined,
              slug: slug.trim() !== form.slug ? slug.trim() : undefined,
              backends: backendsChanged ? backends : undefined,
              tags: tagsChanged ? tags : undefined,
              reps: repsChanged ? reps : undefined,
              source_params: sourceParamsChanged ? cleanedParams : undefined,
              post_submission_action: postSubmissionChanged
                ? cleanedAction
                : undefined,
              metadata: dedupChanged
                ? [{ key: EMAIL_DEDUP_KEY, value: emailDedup }]
                : undefined,
            },
          },
          {
            onSuccess: (f) => onSaved(f),
            onError: (err) => setFormError(err.message),
          },
        );
      }}
      noValidate
      className="space-y-6"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't update form</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}
      <BasicsSection name={name} slug={slug} onNameChange={setName} onSlugChange={setSlug} />
      <Section title="Fields" hint="Add, reorder and configure fields in the builder.">
        {hasGoHighLevel(backends) && <GoHighLevelKeyNotice />}
        <Button type="button" variant="outline" size="sm" asChild>
          <Link to={`/forms/${form.id}/build`}>Open field builder</Link>
        </Button>
      </Section>
      <Section
        title="Delivery destinations"
        hint="Every submission fans out to each selected backend."
      >
        <DeliveryDestinations value={backends} onChange={setBackends} />
      </Section>
      <Section
        title="Sales reps"
        hint="Reps offered on this form. A QR link's ?rep=<key> attributes the lead to one of these (owner + rep tag in GoHighLevel)."
      >
        <RepsSelector value={reps} onChange={setReps} />
      </Section>
      <Section
        title="Source parameters"
        hint="Extra URL query params to capture as tags, e.g. an event name. A QR link adds ?<param>=<value>."
      >
        <SourceParamsEditor value={sourceParams} onChange={setSourceParams} />
      </Section>
      <Section
        title="Tags"
        hint="Labels dispatched to backends with every submission. Press Enter or comma to add."
      >
        <TagsEditor value={tags} onChange={setTags} />
      </Section>
      <Section
        title="After submission"
        hint="What the visitor sees once the form is submitted."
      >
        <PostSubmissionEditor value={postSubmission} onChange={setPostSubmission} />
      </Section>
      <Section title="Deduplication">
        <DeduplicationToggle value={emailDedup} onChange={setEmailDedup} />
      </Section>
      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          onClick={onCancel}
          disabled={update.isPending}
        >
          Cancel
        </Button>
        <Button type="submit" disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save changes"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function BasicsSection({
  name,
  slug,
  onNameChange,
  onSlugChange,
}: {
  name: string;
  slug: string;
  onNameChange: (s: string) => void;
  onSlugChange: (s: string) => void;
}) {
  return (
    <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
      <FormField id="form-name" label="Name">
        <Input
          value={name}
          placeholder="Contact us"
          onChange={(e) => onNameChange(e.target.value)}
        />
      </FormField>
      <FormField
        id="form-slug"
        label="Slug"
        hint="URL-safe. Leave blank to derive from name."
      >
        <Input
          value={slug}
          placeholder="contact-us"
          onChange={(e) => onSlugChange(e.target.value)}
        />
      </FormField>
    </div>
  );
}

function DeliveryDestinations({
  value,
  onChange,
}: {
  value: BackendBinding[];
  onChange: (next: BackendBinding[]) => void;
}) {
  const { data, isLoading, isError, error, refetch } = useBackendsList();
  const selectedKeys = new Set(value.map(bindingKey));

  const toggle = (next: BackendBinding) => {
    const key = bindingKey(next);
    if (selectedKeys.has(key)) {
      onChange(value.filter((b) => bindingKey(b) !== key));
    } else {
      onChange([...value, next]);
    }
  };

  const openRelay = openRelayBinding();
  const items: { binding: BackendBinding; label: string; description: string }[] = [
    {
      binding: openRelay,
      label: "OpenRelay",
      description: "Store the submission in this dashboard.",
    },
    ...(data?.items ?? []).map((b) => ({
      binding: { kind: b.kind, instance_id: b.id } as BackendBinding,
      label: b.name,
      description: kindDescription(b.kind),
    })),
  ];

  return (
    <div className="space-y-2">
      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load backends</AlertTitle>
          <AlertDescription>
            {(error as Error | undefined)?.message ?? "Unknown error."}{" "}
            <button
              type="button"
              className="underline font-medium"
              onClick={() => refetch()}
            >
              Try again
            </button>
          </AlertDescription>
        </Alert>
      )}
      <div className="border border-border rounded-md divide-y divide-border">
        {items.map(({ binding, label, description }) => {
          const key = bindingKey(binding);
          const checked = selectedKeys.has(key);
          return (
            <label
              key={key}
              className="flex items-start gap-3 px-3 py-2 cursor-pointer hover:bg-accent/40"
            >
              <input
                type="checkbox"
                className="mt-1 h-4 w-4"
                checked={checked}
                onChange={() => toggle(binding)}
              />
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium">{label}</div>
                <div className="text-xs text-muted-foreground">{description}</div>
              </div>
            </label>
          );
        })}
        {!isLoading && (data?.items?.length ?? 0) === 0 && (
          <div className="px-3 py-2 text-xs text-muted-foreground">
            No configured backends yet. Add one in the Backends section to
            relay submissions to a CRM.
          </div>
        )}
      </div>
    </div>
  );
}

const GOHIGHLEVEL_KIND = "gohighlevel";

function hasGoHighLevel(backends: BackendBinding[]): boolean {
  return backends.some((b) => b.kind === GOHIGHLEVEL_KIND);
}

function GoHighLevelKeyNotice() {
  return (
    <Alert>
      <AlertTitle>Matching GoHighLevel custom fields</AlertTitle>
      <AlertDescription>
        GoHighLevel only stores a custom value when its key matches a custom
        field that already exists in your location — unknown keys are silently
        dropped. Set each custom field's <strong>Key</strong> to the exact
        GoHighLevel field <em>unique key</em> (e.g.{" "}
        <code>contact.how_did_you_hear</code>) or field id. Standard fields map
        automatically.
      </AlertDescription>
    </Alert>
  );
}

function kindDescription(kind: string): string {
  switch (kind) {
    case "gohighlevel":
      return "GoHighLevel — upserts a contact.";
    default:
      return kind;
  }
}

function TagsEditor({
  value,
  onChange,
}: {
  value: string[];
  onChange: (tags: string[]) => void;
}) {
  const [input, setInput] = useState("");

  const add = () => {
    const trimmed = input.trim();
    if (trimmed && !value.includes(trimmed)) {
      onChange([...value, trimmed]);
    }
    setInput("");
  };

  const remove = (index: number) => {
    onChange(value.filter((_, i) => i !== index));
  };

  return (
    <div className="space-y-2">
      {value.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {value.map((tag, i) => (
            <span
              key={`${tag}-${i}`}
              className="inline-flex items-center gap-1 rounded-md border border-border bg-muted px-2 py-0.5 text-xs"
            >
              {tag}
              <button
                type="button"
                className="text-muted-foreground hover:text-foreground leading-none"
                onClick={() => remove(i)}
              >
                &times;
              </button>
            </span>
          ))}
        </div>
      )}
      <Input
        value={input}
        placeholder="Add a tag..."
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            add();
          } else if (e.key === ",") {
            e.preventDefault();
            add();
          }
        }}
      />
    </div>
  );
}

function DeduplicationToggle({
  value,
  onChange,
}: {
  value: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <div className="border border-border rounded-md">
      <label className="flex items-start gap-3 px-3 py-2 cursor-pointer hover:bg-accent/40">
        <input
          type="checkbox"
          className="mt-1 h-4 w-4"
          checked={value}
          onChange={(e) => onChange(e.target.checked)}
        />
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium">Deduplicate by email</div>
          <div className="text-xs text-muted-foreground">
            If an email was already submitted to this form, still accept the
            submission and show success, but flag it as a duplicate and don't
            deliver it to any backend.
          </div>
        </div>
      </label>
    </div>
  );
}

function RepsSelector({
  value,
  onChange,
}: {
  value: number[];
  onChange: (next: number[]) => void;
}) {
  const { data, isLoading, isError, error, refetch } = useRepsList();
  const selected = new Set(value);

  const toggle = (id: number) => {
    if (selected.has(id)) {
      onChange(value.filter((v) => v !== id));
    } else {
      onChange([...value, id]);
    }
  };

  return (
    <div className="space-y-2">
      {isError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't load reps</AlertTitle>
          <AlertDescription>
            {(error as Error | undefined)?.message ?? "Unknown error."}{" "}
            <button
              type="button"
              className="underline font-medium"
              onClick={() => refetch()}
            >
              Try again
            </button>
          </AlertDescription>
        </Alert>
      )}
      <div className="border border-border rounded-md divide-y divide-border">
        {(data?.items ?? []).map((r) => (
          <label
            key={r.id}
            className="flex items-start gap-3 px-3 py-2 cursor-pointer hover:bg-accent/40"
          >
            <input
              type="checkbox"
              className="mt-1 h-4 w-4"
              checked={selected.has(r.id)}
              onChange={() => toggle(r.id)}
            />
            <div className="flex-1 min-w-0">
              <div className="text-sm font-medium">{r.name}</div>
              <div className="text-xs text-muted-foreground">
                <code className="rounded bg-muted px-1 py-0.5">?rep={r.key}</code>
                {r.ghl_user_id ? " · GHL owner set" : " · no GHL owner id"}
              </div>
            </div>
          </label>
        ))}
        {!isLoading && (data?.items?.length ?? 0) === 0 && (
          <div className="px-3 py-2 text-xs text-muted-foreground">
            No reps yet. Add reps in the Sales reps section, then attach them
            here.
          </div>
        )}
      </div>
    </div>
  );
}

const RESERVED_PARAM = "rep";

/** Trim rows, drop blank params, normalise an empty prefix to null. */
/**
 * The three admin-facing choices map onto two wire variants — "message" and
 * "message with a submit-another button" differ only by `allow_resubmit`.
 */
type PostSubmissionChoice = "message" | "message_resubmit" | "redirect";

function defaultPostSubmissionAction(): PostSubmissionAction {
  return { action: "message", config: { allow_resubmit: false } };
}

function choiceOf(action: PostSubmissionAction): PostSubmissionChoice {
  if (action.action === "redirect") return "redirect";
  return action.config.allow_resubmit ? "message_resubmit" : "message";
}

/** Returns an error message, or null when the action is valid. */
function validatePostSubmission(action: PostSubmissionAction): string | null {
  if (action.action !== "redirect") return null;
  const url = action.config.url.trim();
  if (!url) return "Enter the URL to redirect to after submission.";
  if (!/^https?:\/\/[^/?#\s]/i.test(url)) {
    return "The redirect URL must be an absolute http(s) URL, e.g. https://example.com/thanks.";
  }
  return null;
}

/** Trim free text so an all-whitespace entry saves as "use the default". */
function cleanPostSubmission(action: PostSubmissionAction): PostSubmissionAction {
  if (action.action === "redirect") {
    return { action: "redirect", config: { url: action.config.url.trim() } };
  }
  const message = action.config.message?.trim();
  const label = action.config.resubmit_label?.trim();
  return {
    action: "message",
    config: {
      message: message ? message : null,
      allow_resubmit: action.config.allow_resubmit ?? false,
      resubmit_label: label ? label : null,
    },
  };
}

function cleanSourceParams(params: SourceParam[]): SourceParam[] {
  return params
    .map((p) => ({
      param: p.param.trim(),
      tag_prefix: p.tag_prefix?.trim() ? p.tag_prefix.trim() : null,
    }))
    .filter((p) => p.param.length > 0);
}

/** Returns an error message, or null when the source params are valid. */
function validateSourceParams(params: SourceParam[]): string | null {
  const seen = new Set<string>();
  for (const p of params) {
    const param = p.param.trim();
    if (!param) continue; // blank rows are dropped on save
    if (/\s/.test(param)) return `Source param "${param}" cannot contain spaces.`;
    if (param === RESERVED_PARAM) {
      return `"rep" is reserved for rep attribution — use the Sales reps section.`;
    }
    if (seen.has(param)) return `Duplicate source param "${param}".`;
    seen.add(param);
  }
  return null;
}

const RADIO_ROW =
  "flex items-start gap-3 px-3 py-2 cursor-pointer hover:bg-accent/40 border-b border-border last:border-b-0";
const TEXT_INPUT =
  "w-full rounded border border-border bg-background px-2 py-1.5 text-sm";

function PostSubmissionEditor({
  value,
  onChange,
}: {
  value: PostSubmissionAction;
  onChange: (next: PostSubmissionAction) => void;
}) {
  const choice = choiceOf(value);
  const message = value.action === "message" ? value.config : null;
  const url = value.action === "redirect" ? value.config.url : "";

  // Switching between the two message choices, or away to redirect and back,
  // must not discard copy the admin already typed — so the message body is
  // held here rather than only inside the wire value.
  const [draft, setDraft] = useState(() => ({
    message: message?.message ?? "",
    resubmit_label: message?.resubmit_label ?? "",
    url,
  }));

  const select = (next: PostSubmissionChoice) => {
    if (next === "redirect") {
      onChange({ action: "redirect", config: { url: draft.url } });
      return;
    }
    onChange({
      action: "message",
      config: {
        message: draft.message ? draft.message : null,
        allow_resubmit: next === "message_resubmit",
        resubmit_label: draft.resubmit_label ? draft.resubmit_label : null,
      },
    });
  };

  const patchMessage = (patch: { message?: string; resubmit_label?: string }) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    onChange({
      action: "message",
      config: {
        message: next.message ? next.message : null,
        allow_resubmit: choice === "message_resubmit",
        resubmit_label: next.resubmit_label ? next.resubmit_label : null,
      },
    });
  };

  const patchUrl = (next: string) => {
    setDraft({ ...draft, url: next });
    onChange({ action: "redirect", config: { url: next } });
  };

  const option = (
    key: PostSubmissionChoice,
    title: string,
    description: string,
  ) => (
    <label className={RADIO_ROW}>
      <input
        type="radio"
        name="post-submission-action"
        className="mt-1 h-4 w-4"
        checked={choice === key}
        onChange={() => select(key)}
      />
      <div className="flex-1 min-w-0">
        <div className="text-sm font-medium">{title}</div>
        <div className="text-xs text-muted-foreground">{description}</div>
      </div>
    </label>
  );

  return (
    <div className="space-y-3">
      <div className="border border-border rounded-md">
        {option(
          "message",
          "Show a thank-you message",
          "The form is replaced by a confirmation message.",
        )}
        {option(
          "message_resubmit",
          'Thank-you message with "submit another"',
          "Adds a button that clears the form for another response — useful at a kiosk or trade-show stand.",
        )}
        {option(
          "redirect",
          "Redirect to a URL",
          "Sends the visitor to your own thank-you page, where your conversion tracking lives.",
        )}
      </div>

      {choice !== "redirect" && (
        <>
          <FormField
            id="post-submission-message"
            label="Message"
            hint="Plain text; line breaks are kept. Leave blank to use the default."
          >
            <textarea
              rows={3}
              className={TEXT_INPUT}
              placeholder={DEFAULT_THANKS}
              value={draft.message}
              onChange={(e) => patchMessage({ message: e.target.value })}
            />
          </FormField>
          {choice === "message_resubmit" && (
            <FormField
              id="post-submission-resubmit-label"
              label="Button label"
              hint={`Leave blank to use "${DEFAULT_RESUBMIT_LABEL}".`}
            >
              <Input
                placeholder={DEFAULT_RESUBMIT_LABEL}
                value={draft.resubmit_label}
                onChange={(e) => patchMessage({ resubmit_label: e.target.value })}
              />
            </FormField>
          )}
        </>
      )}

      {choice === "redirect" && (
        <FormField
          id="post-submission-url"
          label="Redirect URL"
          hint="Must be an absolute http(s) URL. The visitor is sent here as-is — nothing is appended."
        >
          <Input
            placeholder="https://example.com/thanks"
            value={draft.url}
            onChange={(e) => patchUrl(e.target.value)}
          />
        </FormField>
      )}
    </div>
  );
}

function SourceParamsEditor({
  value,
  onChange,
}: {
  value: SourceParam[];
  onChange: (next: SourceParam[]) => void;
}) {
  const update = (i: number, patch: Partial<SourceParam>) => {
    onChange(value.map((p, idx) => (idx === i ? { ...p, ...patch } : p)));
  };
  const remove = (i: number) => onChange(value.filter((_, idx) => idx !== i));
  const add = () => onChange([...value, { param: "", tag_prefix: null }]);

  return (
    <div className="space-y-2">
      {value.length > 0 && (
        <div className="space-y-2">
          {value.map((p, i) => (
            <div key={i} className="flex items-center gap-2">
              <Input
                value={p.param}
                placeholder="event"
                aria-label="Param name"
                onChange={(e) => update(i, { param: e.target.value })}
              />
              <Input
                value={p.tag_prefix ?? ""}
                placeholder="tag prefix (optional)"
                aria-label="Tag prefix"
                onChange={(e) => update(i, { tag_prefix: e.target.value })}
              />
              <button
                type="button"
                className="shrink-0 text-muted-foreground hover:text-foreground px-2 leading-none"
                aria-label="Remove source param"
                onClick={() => remove(i)}
              >
                &times;
              </button>
            </div>
          ))}
        </div>
      )}
      <Button type="button" variant="outline" size="sm" onClick={add}>
        <Plus className="h-4 w-4" />
        Add source param
      </Button>
      {value.length > 0 && (
        <p className="text-xs text-muted-foreground">
          A captured value becomes a tag. With a prefix it's{" "}
          <code className="rounded bg-muted px-1 py-0.5">prefix:value</code>;
          without one, the value verbatim.
        </p>
      )}
    </div>
  );
}

function Section({
  title,
  hint,
  children,
}: {
  title: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="space-y-2">
      <div>
        <h3 className="text-sm font-semibold">{title}</h3>
        {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
      </div>
      {children}
    </section>
  );
}
