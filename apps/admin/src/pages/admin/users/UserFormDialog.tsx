import { useEffect, useState } from "react";
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
} from "@open-relay/ui";
import {
  useCreateUser,
  useUpdateUser,
} from "../../../lib/users/useUserMutations";
import type { UserDto } from "../../../lib/users/useUsers";
import { usePermissions } from "../../../lib/auth/usePermissions";
import { RoleMultiSelect } from "../../../components/roles/RoleMultiSelect";

const baseFields = {
  email: z.string().email("Enter a valid email address."),
  display_name: z
    .string()
    .trim()
    .max(255, "255 characters max.")
    .optional(),
};

const createSchema = z
  .object({
    ...baseFields,
    password: z.string().min(12, "Minimum 12 characters."),
    confirm: z.string(),
    role_ids: z.array(z.number()).default([]),
  })
  .refine((d) => d.password === d.confirm, {
    path: ["confirm"],
    message: "Passwords don't match.",
  });

const editSchema = z.object({
  ...baseFields,
  role_ids: z.array(z.number()).default([]),
});

type CreateValues = z.infer<typeof createSchema>;
type EditValues = z.infer<typeof editSchema>;

export interface UserFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existingUser?: UserDto | null;
  onSaved?: (user: UserDto) => void;
}

export function UserFormDialog({
  open,
  onOpenChange,
  existingUser,
  onSaved,
}: UserFormDialogProps) {
  const isEdit = Boolean(existingUser);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit user" : "New user"}</DialogTitle>
          <DialogDescription>
            {isEdit
              ? "Update this user's profile. Use the password action separately to change their password."
              : "Create a new admin user. They'll be able to sign in immediately."}
          </DialogDescription>
        </DialogHeader>
        {isEdit && existingUser ? (
          <EditForm
            key={existingUser.id}
            user={existingUser}
            onSaved={(u) => {
              onSaved?.(u);
              onOpenChange(false);
            }}
            onCancel={() => onOpenChange(false)}
          />
        ) : (
          <CreateForm
            onSaved={(u) => {
              onSaved?.(u);
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
  onSaved: (u: UserDto) => void;
  onCancel: () => void;
}) {
  const [formError, setFormError] = useState<string | null>(null);
  const create = useCreateUser();
  const { has } = usePermissions();
  const canAssign = has("roles:assign");
  const {
    register,
    handleSubmit,
    formState: { errors },
    reset,
    watch,
    setValue,
  } = useForm<CreateValues>({
    resolver: zodResolver(createSchema),
    defaultValues: { role_ids: [] },
  });

  useEffect(() => {
    reset({ role_ids: [] });
    setFormError(null);
  }, [reset]);

  const roleIds = watch("role_ids") ?? [];

  return (
    <form
      onSubmit={handleSubmit((values) => {
        setFormError(null);
        const displayName = values.display_name?.trim() ?? "";
        create.mutate(
          {
            email: values.email.trim(),
            password: values.password,
            display_name: displayName ? displayName : null,
            // Omit role_ids entirely when the user can't assign — keeps the
            // server's `roles:assign` gate from rejecting an inert vec![].
            ...(canAssign ? { role_ids: values.role_ids } : {}),
          },
          {
            onSuccess: (u) => onSaved(u),
            onError: (err) => setFormError(err.message),
          },
        );
      })}
      noValidate
      className="space-y-4"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't create user</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}
      <FormField
        id="user-display_name"
        label="Display name (optional)"
        error={errors.display_name?.message}
      >
        <Input autoComplete="name" placeholder="Ada Lovelace" {...register("display_name")} />
      </FormField>
      <FormField id="user-email" label="Email" error={errors.email?.message}>
        <Input
          type="email"
          autoComplete="email"
          placeholder="user@example.com"
          {...register("email")}
        />
      </FormField>
      <FormField
        id="user-password"
        label="Password"
        hint="At least 12 characters."
        error={errors.password?.message}
      >
        <Input type="password" autoComplete="new-password" {...register("password")} />
      </FormField>
      <FormField id="user-confirm" label="Confirm password" error={errors.confirm?.message}>
        <Input type="password" autoComplete="new-password" {...register("confirm")} />
      </FormField>
      {canAssign && (
        <div className="space-y-2">
          <div className="text-sm font-medium">Roles</div>
          <RoleMultiSelect
            value={roleIds}
            onChange={(next) =>
              setValue("role_ids", next, { shouldDirty: true })
            }
          />
        </div>
      )}
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel} disabled={create.isPending}>
          Cancel
        </Button>
        <Button type="submit" disabled={create.isPending}>
          {create.isPending ? "Creating…" : "Create user"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function EditForm({
  user,
  onSaved,
  onCancel,
}: {
  user: UserDto;
  onSaved: (u: UserDto) => void;
  onCancel: () => void;
}) {
  const [formError, setFormError] = useState<string | null>(null);
  const update = useUpdateUser();
  const { has } = usePermissions();
  const canAssign = has("roles:assign");
  const existingRoleIds = (user.roles ?? []).map((r) => r.id);
  const {
    register,
    handleSubmit,
    formState: { errors },
    watch,
    setValue,
  } = useForm<EditValues>({
    resolver: zodResolver(editSchema),
    defaultValues: {
      email: user.email,
      display_name: user.display_name ?? "",
      role_ids: existingRoleIds,
    },
  });
  const roleIds = watch("role_ids") ?? [];

  return (
    <form
      onSubmit={handleSubmit((values) => {
        setFormError(null);
        const email = values.email.trim();
        const displayName = (values.display_name ?? "").trim();
        const next = values.role_ids ?? [];
        const rolesChanged =
          canAssign &&
          (next.length !== existingRoleIds.length ||
            next.some((id) => !existingRoleIds.includes(id)));
        update.mutate(
          {
            id: user.id,
            input: {
              email: email !== user.email ? email : undefined,
              display_name:
                displayName !== (user.display_name ?? "") ? displayName : undefined,
              role_ids: rolesChanged ? next : undefined,
            },
          },
          {
            onSuccess: (u) => onSaved(u),
            onError: (err) => setFormError(err.message),
          },
        );
      })}
      noValidate
      className="space-y-4"
    >
      {formError && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't update user</AlertTitle>
          <AlertDescription>{formError}</AlertDescription>
        </Alert>
      )}
      <FormField
        id="user-display_name"
        label="Display name (optional)"
        error={errors.display_name?.message}
      >
        <Input autoComplete="name" placeholder="Ada Lovelace" {...register("display_name")} />
      </FormField>
      <FormField id="user-email" label="Email" error={errors.email?.message}>
        <Input type="email" autoComplete="email" {...register("email")} />
      </FormField>
      {canAssign && (
        <div className="space-y-2">
          <div className="text-sm font-medium">Roles</div>
          <RoleMultiSelect
            value={roleIds}
            onChange={(next) =>
              setValue("role_ids", next, { shouldDirty: true })
            }
          />
        </div>
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
