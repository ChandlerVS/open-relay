import { useEffect, useState } from "react";
import { useForm, type UseFormRegister, type FieldValues } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
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
import { useBackendKinds, type BackendInstanceDto } from "../../../lib/backends/useBackends";
import {
  useCreateBackend,
  useUpdateBackend,
} from "../../../lib/backends/useBackendMutations";

const NAME_FIELD = z.string().trim().min(1, "Required.").max(200, "200 characters max.");
const LOCATION_FIELD = z.string().trim().min(1, "Required.");

const createSchema = z.object({
  kind: z.string().min(1, "Pick a backend kind."),
  name: NAME_FIELD,
  location_id: LOCATION_FIELD,
  private_integration_token: z.string().min(1, "Paste the token."),
});

const editSchema = z.object({
  name: NAME_FIELD,
  location_id: LOCATION_FIELD,
  // Empty string means "keep the existing token unchanged".
  private_integration_token: z.string(),
});

type CreateValues = z.infer<typeof createSchema>;
type EditValues = z.infer<typeof editSchema>;

export interface BackendFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existing?: BackendInstanceDto | null;
  onSaved?: (b: BackendInstanceDto) => void;
}

export function BackendFormDialog({
  open,
  onOpenChange,
  existing,
  onSaved,
}: BackendFormDialogProps) {
  const isEdit = Boolean(existing);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit backend" : "New backend"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update this backend's name or credentials. Leave the token blank to keep the current one."
              : "Configure a delivery destination. Forms attach to it by selecting the backend on the form's delivery list."}
          </DialogDescription>
        </DialogHeader>
        {isEdit && existing ? (
          <EditForm
            key={existing.id}
            backend={existing}
            onSaved={(b) => {
              onSaved?.(b);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        ) : (
          <CreateForm
            onSaved={(b) => {
              onSaved?.(b);
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
  onSaved: (b: BackendInstanceDto) => void;
  onCancel: () => void;
}) {
  const [formError, setFormError] = useState<string | null>(null);
  const create = useCreateBackend();
  const kindsQuery = useBackendKinds();
  const configurableKinds = (kindsQuery.data ?? []).filter((k) => k.configurable);
  const onlyKind = configurableKinds.length === 1 ? configurableKinds[0]!.kind : undefined;

  const {
    register,
    handleSubmit,
    formState: { errors },
    watch,
    reset,
  } = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: { kind: onlyKind ?? "" },
  });

  useEffect(() => {
    if (onlyKind) {
      reset({ kind: onlyKind, name: "", location_id: "", private_integration_token: "" });
    }
  }, [onlyKind, reset]);

  const kind = watch("kind");

  return (
    <form
      onSubmit={handleSubmit((values) => {
        setFormError(null);
        create.mutate(
          {
            kind: values.kind,
            name: values.name.trim(),
            config: {
              location_id: values.location_id.trim(),
              private_integration_token: values.private_integration_token,
            },
          },
          {
            onSuccess: (b) => onSaved(b),
            onError: (err) => setFormError(err.message),
          },
        );
      })}
      noValidate
      className="space-y-4"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't create backend</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}

      <FormField id="backend-kind" label="Kind" error={errors.kind?.message}>
        <select
          className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          {...register("kind")}
        >
          <option value="">Pick a backend…</option>
          {configurableKinds.map((k) => (
            <option key={k.kind} value={k.kind}>
              {labelForKind(k.kind)}
            </option>
          ))}
        </select>
      </FormField>

      <FormField id="backend-name" label="Name" hint="Shown in the admin." error={errors.name?.message}>
        <Input placeholder="Acme HQ" {...register("name")} />
      </FormField>

      {kind === "gohighlevel" && (
        <GoHighLevelFields
          register={register as unknown as UseFormRegister<FieldValues>}
          errors={errors}
          tokenLabel="Private integration token"
          tokenHint="Paste the PIT from your GHL location's 'Private Integrations' settings."
        />
      )}

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel} disabled={create.isPending}>
          Cancel
        </Button>
        <Button type="submit" disabled={create.isPending}>
          {create.isPending ? "Creating…" : "Create backend"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function EditForm({
  backend,
  onSaved,
  onCancel,
}: {
  backend: BackendInstanceDto;
  onSaved: (b: BackendInstanceDto) => void;
  onCancel: () => void;
}) {
  const [formError, setFormError] = useState<string | null>(null);
  const update = useUpdateBackend();

  const existingLocation =
    typeof (backend.config as { location_id?: unknown })?.location_id === "string"
      ? ((backend.config as { location_id: string }).location_id)
      : "";

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: {
      name: backend.name,
      location_id: existingLocation,
      private_integration_token: "",
    },
  });

  return (
    <form
      onSubmit={handleSubmit((values) => {
        setFormError(null);
        const trimmedToken = values.private_integration_token;
        const trimmedLocation = values.location_id.trim();
        const newName = values.name.trim();

        // The server requires the full config blob on update — so always
        // send location_id + token. If the user left the token blank, fall
        // back to the existing value so the backend isn't accidentally
        // wiped.
        const existingToken =
          typeof (backend.config as { private_integration_token?: unknown })
            ?.private_integration_token === "string"
            ? ((backend.config as { private_integration_token: string })
                .private_integration_token)
            : "";

        update.mutate(
          {
            id: backend.id,
            input: {
              name: newName !== backend.name ? newName : undefined,
              config: {
                location_id: trimmedLocation,
                private_integration_token:
                  trimmedToken.length > 0 ? trimmedToken : existingToken,
              },
            },
          },
          {
            onSuccess: (b) => onSaved(b),
            onError: (err) => setFormError(err.message),
          },
        );
      })}
      noValidate
      className="space-y-4"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't update backend</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}

      <FormField
        id="backend-kind"
        label="Kind"
        hint="Kind can't change after creation."
      >
        <Input value={labelForKind(backend.kind)} disabled readOnly />
      </FormField>

      <FormField id="backend-name" label="Name" error={errors.name?.message}>
        <Input placeholder="Acme HQ" {...register("name")} />
      </FormField>

      {backend.kind === "gohighlevel" && (
        <GoHighLevelFields
          register={register as unknown as UseFormRegister<FieldValues>}
          errors={errors}
          tokenLabel="Private integration token"
          tokenHint="Leave blank to keep the current token. Paste a new PIT to replace it."
        />
      )}

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel} disabled={update.isPending}>
          Cancel
        </Button>
        <Button type="submit" disabled={update.isPending}>
          {update.isPending ? "Saving…" : "Save changes"}
        </Button>
      </DialogFooter>
    </form>
  );
}

interface ConfigFieldsProps {
  // Typed as `FieldValues` so the widget is reusable between create + edit
  // forms without TS unifying their two distinct schemas. The field names
  // below are constant strings shared by both schemas.
  register: UseFormRegister<FieldValues>;
  errors: Partial<
    Record<"location_id" | "private_integration_token", { message?: string }>
  >;
  tokenLabel: string;
  tokenHint: string;
}

function GoHighLevelFields({
  register,
  errors,
  tokenLabel,
  tokenHint,
}: ConfigFieldsProps) {
  return (
    <>
      <FormField
        id="backend-location_id"
        label="Location ID"
        hint="The GHL sub-account / location this PIT was issued for."
        error={errors.location_id?.message}
      >
        <Input placeholder="loc_…" {...register("location_id")} />
      </FormField>
      <FormField
        id="backend-token"
        label={tokenLabel}
        hint={tokenHint}
        error={errors.private_integration_token?.message}
      >
        <Input
          type="password"
          autoComplete="off"
          placeholder="pit-…"
          {...register("private_integration_token")}
        />
      </FormField>
    </>
  );
}

function labelForKind(kind: string): string {
  switch (kind) {
    case "gohighlevel":
      return "GoHighLevel";
    case "open-relay":
      return "OpenRelay";
    default:
      return kind;
  }
}
