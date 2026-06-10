import { useState } from "react";
import type { components } from "@open-relay/api-client";
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
import { STANDARD_FIELDS } from "../../../lib/forms/standardFields";
import { useBackendsList } from "../../../lib/backends/useBackends";
import { StandardFieldsList } from "./StandardFieldsList";
import { CustomFieldsEditor } from "./CustomFieldsEditor";

type StandardFieldsConfig = components["schemas"]["StandardFieldsConfig"];
type StandardFieldConfig = components["schemas"]["StandardFieldConfig"];
type CustomField = components["schemas"]["CustomField"];
type BackendBinding = components["schemas"]["BackendBinding"];

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

function defaultStandardFields(): StandardFieldsConfig {
  // Starter config: name + email enabled+required, message enabled, rest off.
  // Mirrors the server-side default so a freshly-created form behaves the
  // same whether the admin tweaks defaults or sends them through unchanged.
  const off: StandardFieldConfig = { enabled: false, required: false, label: null };
  const onReq: StandardFieldConfig = { enabled: true, required: true, label: null };
  const on: StandardFieldConfig = { enabled: true, required: false, label: null };
  const cfg = Object.fromEntries(
    STANDARD_FIELDS.map(({ key }) => {
      if (key === "first_name" || key === "last_name" || key === "email") return [key, onReq];
      if (key === "message") return [key, on];
      return [key, off];
    }),
  ) as StandardFieldsConfig;
  return cfg;
}

interface ValidationResult {
  ok: boolean;
  message?: string;
  /** Per-custom-field index → error message, surfaced inline on the row. */
  customFieldErrors?: Record<number, string | undefined>;
}

function validate(input: {
  name: string;
  slug: string;
  customFields: CustomField[];
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
  const customErrors: Record<number, string | undefined> = {};
  const seenKeys = new Set<string>();
  const standardKeys = new Set(STANDARD_FIELDS.map((f) => f.key));
  for (let i = 0; i < input.customFields.length; i++) {
    const f = input.customFields[i]!;
    if (!f.label.trim()) {
      customErrors[i] = "Label is required.";
      continue;
    }
    if (!f.key.trim()) {
      customErrors[i] = "Key is required.";
      continue;
    }
    // Keys are format-agnostic so they can match a backend's field key/id
    // (e.g. GoHighLevel) exactly — only whitespace and length are constrained.
    if (/\s/.test(f.key)) {
      customErrors[i] = "Key cannot contain whitespace.";
      continue;
    }
    if (f.key.length > 64) {
      customErrors[i] = "Key must be 64 characters or fewer.";
      continue;
    }
    if (standardKeys.has(f.key)) {
      customErrors[i] = `'${f.key}' is a standard field key — pick another.`;
      continue;
    }
    if (seenKeys.has(f.key)) {
      customErrors[i] = `Duplicate key '${f.key}'.`;
      continue;
    }
    seenKeys.add(f.key);
    if (f.type === "select") {
      const opts = (f.options ?? []).map((o) => o.trim()).filter(Boolean);
      if (opts.length === 0) {
        customErrors[i] = "Select fields need at least one option.";
        continue;
      }
    }
  }
  if (Object.keys(customErrors).length > 0) {
    return {
      ok: false,
      message: "Fix the highlighted custom fields.",
      customFieldErrors: customErrors,
    };
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
  const [standardFields, setStandardFields] = useState<StandardFieldsConfig>(
    defaultStandardFields(),
  );
  const [customFields, setCustomFields] = useState<CustomField[]>([]);
  const [backends, setBackends] = useState<BackendBinding[]>([openRelayBinding()]);
  const [tags, setTags] = useState<string[]>([]);
  const [formError, setFormError] = useState<string | null>(null);
  const [customErrors, setCustomErrors] = useState<Record<number, string | undefined>>({});

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, customFields, backends });
        setCustomErrors(v.customFieldErrors ?? {});
        if (!v.ok) {
          setFormError(v.message ?? "Form has errors.");
          return;
        }
        setFormError(null);
        create.mutate(
          {
            name: name.trim(),
            slug: slug.trim() ? slug.trim() : null,
            standard_fields: standardFields,
            custom_fields: customFields,
            backends,
            tags,
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
      <Section title="Standard fields" hint="Enable the ones your backend cares about.">
        <StandardFieldsList value={standardFields} onChange={setStandardFields} />
      </Section>
      <Section title="Custom fields" hint="Anything not in the standard set.">
        {hasGoHighLevel(backends) && <GoHighLevelKeyNotice />}
        <CustomFieldsEditor
          value={customFields}
          onChange={setCustomFields}
          errors={customErrors}
        />
      </Section>
      <Section
        title="Delivery destinations"
        hint="Every submission fans out to each selected backend."
      >
        <DeliveryDestinations value={backends} onChange={setBackends} />
      </Section>
      <Section
        title="Tags"
        hint="Labels dispatched to backends with every submission. Press Enter or comma to add."
      >
        <TagsEditor value={tags} onChange={setTags} />
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
  const [standardFields, setStandardFields] = useState<StandardFieldsConfig>(
    form.standard_fields,
  );
  const [customFields, setCustomFields] = useState<CustomField[]>(form.custom_fields);
  const [backends, setBackends] = useState<BackendBinding[]>(form.backends);
  const [tags, setTags] = useState<string[]>(form.tags);
  const [formError, setFormError] = useState<string | null>(null);
  const [customErrors, setCustomErrors] = useState<Record<number, string | undefined>>({});

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, customFields, backends });
        setCustomErrors(v.customFieldErrors ?? {});
        if (!v.ok) {
          setFormError(v.message ?? "Form has errors.");
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
        update.mutate(
          {
            id: form.id,
            input: {
              name: name.trim() !== form.name ? name.trim() : undefined,
              slug: slug.trim() !== form.slug ? slug.trim() : undefined,
              standard_fields: standardFields,
              custom_fields: customFields,
              backends: backendsChanged ? backends : undefined,
              tags: tagsChanged ? tags : undefined,
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
      <Section title="Standard fields" hint="Enable the ones your backend cares about.">
        <StandardFieldsList value={standardFields} onChange={setStandardFields} />
      </Section>
      <Section title="Custom fields" hint="Anything not in the standard set.">
        {hasGoHighLevel(backends) && <GoHighLevelKeyNotice />}
        <CustomFieldsEditor
          value={customFields}
          onChange={setCustomFields}
          errors={customErrors}
        />
      </Section>
      <Section
        title="Delivery destinations"
        hint="Every submission fans out to each selected backend."
      >
        <DeliveryDestinations value={backends} onChange={setBackends} />
      </Section>
      <Section
        title="Tags"
        hint="Labels dispatched to backends with every submission. Press Enter or comma to add."
      >
        <TagsEditor value={tags} onChange={setTags} />
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
