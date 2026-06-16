import { useState } from "react";
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
import { useCreateRep, useUpdateRep } from "../../../lib/reps/useRepMutations";
import type { RepDto } from "../../../lib/reps/useReps";

export interface RepFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  existing?: RepDto | null;
}

function validateKey(key: string): string | null {
  if (!key) return null; // optional — server derives from name
  if (!/^[a-z0-9-]+$/.test(key)) {
    return "Key may only contain lowercase letters, digits, and hyphens.";
  }
  if (key.startsWith("-") || key.endsWith("-")) {
    return "Key cannot start or end with a hyphen.";
  }
  if (key.includes("--")) return "Key cannot contain consecutive hyphens.";
  return null;
}

export function RepFormDialog({ open, onOpenChange, existing }: RepFormDialogProps) {
  const isEdit = Boolean(existing);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? "Edit rep" : "New rep"}</DialogTitle>
          <DialogDescription>
            Reps are reusable across forms. The <strong>key</strong> is what a QR
            code carries as <code>?rep=&lt;key&gt;</code>; the GoHighLevel user id
            makes the rep the contact owner on delivery.
          </DialogDescription>
        </DialogHeader>
        <RepForm
          key={existing?.id ?? "new"}
          existing={existing ?? null}
          onDone={() => onOpenChange(false)}
        />
      </DialogContent>
    </Dialog>
  );
}

function RepForm({
  existing,
  onDone,
}: {
  existing: RepDto | null;
  onDone: () => void;
}) {
  const create = useCreateRep();
  const update = useUpdateRep();
  const [name, setName] = useState(existing?.name ?? "");
  const [key, setKey] = useState(existing?.key ?? "");
  const [email, setEmail] = useState(existing?.email ?? "");
  const [ghlUserId, setGhlUserId] = useState(existing?.ghl_user_id ?? "");
  const [error, setError] = useState<string | null>(null);

  const pending = create.isPending || update.isPending;

  const submit = () => {
    if (!name.trim()) {
      setError("Name is required.");
      return;
    }
    const keyErr = validateKey(key.trim());
    if (keyErr) {
      setError(keyErr);
      return;
    }
    setError(null);
    if (existing) {
      update.mutate(
        {
          id: existing.id,
          input: {
            name: name.trim() !== existing.name ? name.trim() : undefined,
            key: key.trim() !== existing.key ? key.trim() : undefined,
            // Send "" to clear a previously-set value.
            email: email.trim() !== (existing.email ?? "") ? email.trim() : undefined,
            ghl_user_id:
              ghlUserId.trim() !== (existing.ghl_user_id ?? "")
                ? ghlUserId.trim()
                : undefined,
          },
        },
        { onSuccess: onDone, onError: (e) => setError(e.message) },
      );
    } else {
      create.mutate(
        {
          name: name.trim(),
          key: key.trim() ? key.trim() : null,
          email: email.trim() ? email.trim() : null,
          ghl_user_id: ghlUserId.trim() ? ghlUserId.trim() : null,
        },
        { onSuccess: onDone, onError: (e) => setError(e.message) },
      );
    }
  };

  return (
    <form
      noValidate
      className="space-y-4"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      {error && (
        <Alert variant="destructive">
          <AlertTitle>Couldn't save rep</AlertTitle>
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <FormField id="rep-name" label="Name">
        <Input
          value={name}
          placeholder="Jane Doe"
          onChange={(e) => setName(e.target.value)}
        />
      </FormField>
      <FormField id="rep-key" label="Key" hint="URL-safe. Leave blank to derive from name.">
        <Input
          value={key}
          placeholder="jane-doe"
          onChange={(e) => setKey(e.target.value)}
        />
      </FormField>
      <FormField id="rep-email" label="Email" hint="Optional.">
        <Input
          type="email"
          value={email}
          placeholder="jane@example.com"
          onChange={(e) => setEmail(e.target.value)}
        />
      </FormField>
      <FormField
        id="rep-ghl"
        label="GoHighLevel user id"
        hint="Optional. Set this to assign the contact owner in GHL."
      >
        <Input
          value={ghlUserId}
          placeholder="usr_abc123"
          onChange={(e) => setGhlUserId(e.target.value)}
        />
      </FormField>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onDone} disabled={pending}>
          Cancel
        </Button>
        <Button type="submit" disabled={pending}>
          {pending ? "Saving…" : existing ? "Save changes" : "Create rep"}
        </Button>
      </DialogFooter>
    </form>
  );
}
