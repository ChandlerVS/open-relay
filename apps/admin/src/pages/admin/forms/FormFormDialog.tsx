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
import { StandardFieldsList } from "./StandardFieldsList";
import { CustomFieldsEditor } from "./CustomFieldsEditor";

type StandardFieldsConfig = components["schemas"]["StandardFieldsConfig"];
type StandardFieldConfig = components["schemas"]["StandardFieldConfig"];
type CustomField = components["schemas"]["CustomField"];

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
    if (!/^[a-z][a-z0-9_]*$/.test(f.key)) {
      customErrors[i] = "Key must be snake_case (a-z, 0-9, _).";
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
  const [formError, setFormError] = useState<string | null>(null);
  const [customErrors, setCustomErrors] = useState<Record<number, string | undefined>>({});

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, customFields });
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
        <CustomFieldsEditor
          value={customFields}
          onChange={setCustomFields}
          errors={customErrors}
        />
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
  const [formError, setFormError] = useState<string | null>(null);
  const [customErrors, setCustomErrors] = useState<Record<number, string | undefined>>({});

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const v = validate({ name, slug, customFields });
        setCustomErrors(v.customFieldErrors ?? {});
        if (!v.ok) {
          setFormError(v.message ?? "Form has errors.");
          return;
        }
        setFormError(null);
        update.mutate(
          {
            id: form.id,
            input: {
              name: name.trim() !== form.name ? name.trim() : undefined,
              slug: slug.trim() !== form.slug ? slug.trim() : undefined,
              standard_fields: standardFields,
              custom_fields: customFields,
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
        <CustomFieldsEditor
          value={customFields}
          onChange={setCustomFields}
          errors={customErrors}
        />
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
