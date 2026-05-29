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
import { useChangeUserPassword } from "../../../lib/users/useUserMutations";
import type { UserDto } from "../../../lib/users/useUsers";

const schema = z
  .object({
    password: z.string().min(12, "Minimum 12 characters."),
    confirm: z.string(),
  })
  .refine((d) => d.password === d.confirm, {
    path: ["confirm"],
    message: "Passwords don't match.",
  });

type FormValues = z.infer<typeof schema>;

export interface ChangePasswordDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  user: UserDto | null;
}

export function ChangePasswordDialog({ open, onOpenChange, user }: ChangePasswordDialogProps) {
  const [formError, setFormError] = useState<string | null>(null);
  const mutation = useChangeUserPassword();
  const {
    register,
    handleSubmit,
    formState: { errors },
    reset,
  } = useForm<FormValues>({ resolver: zodResolver(schema) });

  useEffect(() => {
    if (open) {
      reset();
      setFormError(null);
    }
  }, [open, reset]);

  const label =
    user?.display_name?.trim() || user?.email || "this user";

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>Change password</DialogTitle>
          <DialogDescription>Set a new password for {label}.</DialogDescription>
        </DialogHeader>
        <form
          onSubmit={handleSubmit((values) => {
            if (!user) return;
            setFormError(null);
            mutation.mutate(
              { id: user.id, input: { password: values.password } },
              {
                onSuccess: () => onOpenChange(false),
                onError: (err) => setFormError(err.message),
              },
            );
          })}
          noValidate
          className="space-y-4"
        >
          {formError && (
            <Alert variant="destructive">
              <AlertTitle>Couldn't change password</AlertTitle>
              <AlertDescription>{formError}</AlertDescription>
            </Alert>
          )}
          <FormField
            id="cp-password"
            label="New password"
            hint="At least 12 characters."
            error={errors.password?.message}
          >
            <Input type="password" autoComplete="new-password" {...register("password")} />
          </FormField>
          <FormField id="cp-confirm" label="Confirm" error={errors.confirm?.message}>
            <Input type="password" autoComplete="new-password" {...register("confirm")} />
          </FormField>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
              disabled={mutation.isPending}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Updating…" : "Update password"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
