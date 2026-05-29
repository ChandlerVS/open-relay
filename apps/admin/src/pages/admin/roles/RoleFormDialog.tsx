import { useMemo, useState } from "react";
import { useForm } from "react-hook-form";
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
  Skeleton,
} from "@open-relay/ui";
import type { Permission } from "../../../lib/auth/AuthContext";
import { usePermissionCatalog } from "../../../lib/permissions/usePermissionCatalog";
import {
  useCreateRole,
  useUpdateRole,
} from "../../../lib/roles/useRoleMutations";
import type { RoleDto } from "../../../lib/roles/useRoles";

const schema = z.object({
  name: z
    .string()
    .trim()
    .min(1, "Name is required.")
    .max(255, "255 characters max."),
  description: z.string().trim().max(1024, "1024 characters max.").optional(),
  permissions: z.array(z.string()),
});

type FormValues = z.infer<typeof schema>;

export interface RoleFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existingRole?: RoleDto | null;
  onSaved?: (role: RoleDto) => void;
}

export function RoleFormDialog({
  open,
  onOpenChange,
  existingRole,
  onSaved,
}: RoleFormDialogProps) {
  const isEdit = Boolean(existingRole);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit role" : "New role"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update the role's name, description, and which permissions it grants."
              : "Bundle a set of permissions under a name. Assign the role to users from the user form."}
          </DialogDescription>
        </DialogHeader>
        <RoleForm
          key={existingRole?.id ?? "new"}
          existing={existingRole ?? null}
          onSaved={(r) => {
            onSaved?.(r);
            onOpenChange(false);
          }}
          onCancel={() => onOpenChange(false)}
        />
      </DialogContent>
    </Dialog>
  );
}

function RoleForm({
  existing,
  onSaved,
  onCancel,
}: {
  existing: RoleDto | null;
  onSaved: (r: RoleDto) => void;
  onCancel: () => void;
}) {
  const [formError, setFormError] = useState<string | null>(null);
  const create = useCreateRole();
  const update = useUpdateRole();
  const catalog = usePermissionCatalog();

  const {
    register,
    handleSubmit,
    formState: { errors },
    watch,
    setValue,
  } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: existing?.name ?? "",
      description: existing?.description ?? "",
      permissions: existing?.permissions ?? [],
    },
  });
  const selected = watch("permissions") ?? [];

  const grouped = useMemo(() => {
    const groups = new Map<string, typeof catalog.data>();
    for (const info of catalog.data ?? []) {
      const list = groups.get(info.resource) ?? [];
      list.push(info);
      groups.set(info.resource, list);
    }
    return groups;
  }, [catalog.data]);

  const pending = create.isPending || update.isPending;

  return (
    <form
      onSubmit={handleSubmit((values) => {
        setFormError(null);
        const desc = values.description?.trim() ?? "";
        const permissions = values.permissions as Permission[];
        if (existing) {
          update.mutate(
            {
              id: existing.id,
              input: {
                name: values.name !== existing.name ? values.name.trim() : undefined,
                description:
                  desc !== (existing.description ?? "") ? desc : undefined,
                permissions,
              },
            },
            {
              onSuccess: (r) => onSaved(r),
              onError: (err) => setFormError(err.message),
            },
          );
        } else {
          create.mutate(
            {
              name: values.name.trim(),
              description: desc || null,
              permissions,
            },
            {
              onSuccess: (r) => onSaved(r),
              onError: (err) => setFormError(err.message),
            },
          );
        }
      })}
      noValidate
      className="space-y-4"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>
            {existing ? "Couldn't update role" : "Couldn't create role"}
          </AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}
      <FormField id="role-name" label="Name" error={errors.name?.message}>
        <Input autoComplete="off" placeholder="Editor" {...register("name")} />
      </FormField>
      <FormField
        id="role-description"
        label="Description (optional)"
        error={errors.description?.message}
      >
        <Input
          autoComplete="off"
          placeholder="What is this role for?"
          {...register("description")}
        />
      </FormField>

      <div className="space-y-2">
        <div className="text-sm font-medium">Permissions</div>
        {catalog.isLoading ? (
          <Skeleton className="h-32 w-full" />
        ) : catalog.isError ? (
          <Alert variant="destructive">
            <AlertTitle>Couldn't load permissions catalogue</AlertTitle>
            <AlertDescription>
              {(catalog.error as Error | undefined)?.message ?? "Unknown error."}
            </AlertDescription>
          </Alert>
        ) : (
          <div className="rounded border border-border divide-y divide-border">
            {Array.from(grouped.entries()).map(([resource, items]) => (
              <fieldset key={resource} className="p-3 space-y-2">
                <legend className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
                  {resource}
                </legend>
                <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                  {(items ?? []).map((p) => {
                    const isOn = selected.includes(p.key);
                    return (
                      <label
                        key={p.key}
                        className="flex items-center gap-2 text-sm cursor-pointer select-none"
                      >
                        <input
                          type="checkbox"
                          checked={isOn}
                          onChange={(e) => {
                            const next = e.target.checked
                              ? [...selected, p.key]
                              : selected.filter((s) => s !== p.key);
                            setValue("permissions", next, { shouldDirty: true });
                          }}
                          className="h-4 w-4 rounded border-border accent-primary"
                        />
                        <span className="flex-1">{p.label}</span>
                        <span className="font-mono text-[10px] text-muted-foreground">
                          {p.action}
                        </span>
                      </label>
                    );
                  })}
                </div>
              </fieldset>
            ))}
          </div>
        )}
      </div>

      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel} disabled={pending}>
          Cancel
        </Button>
        <Button type="submit" disabled={pending}>
          {pending ? "Saving…" : existing ? "Save changes" : "Create role"}
        </Button>
      </DialogFooter>
    </form>
  );
}
