import { useState } from "react";
import {
  Alert,
  AlertDescription,
  AlertTitle,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
  ConfirmDialog,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Skeleton,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@open-relay/ui";
import {
  useApiKeys,
  useCreateApiKey,
  useRevokeApiKey,
  type ApiKeyDto,
} from "../../../lib/apiKeys/useApiKeys";

function formatDate(value: string | null | undefined): string {
  if (!value) return "—";
  return new Date(value).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function statusOf(key: ApiKeyDto): { label: string; muted: boolean } {
  if (key.revoked_at) return { label: "Revoked", muted: true };
  if (key.expires_at && new Date(key.expires_at) < new Date())
    return { label: "Expired", muted: true };
  return { label: "Active", muted: false };
}

export function ApiKeysCard() {
  const keys = useApiKeys();
  const create = useCreateApiKey();
  const revoke = useRevokeApiKey();

  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [expiresInDays, setExpiresInDays] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  // Held only until dismissed. The server stores a hash, so once this leaves
  // the screen the secret is gone for good.
  const [issued, setIssued] = useState<{ name: string; token: string } | null>(
    null,
  );
  const [confirmId, setConfirmId] = useState<number | null>(null);
  const [topError, setTopError] = useState<string | null>(null);

  const submit = async () => {
    setFormError(null);
    const trimmed = name.trim();
    if (!trimmed) {
      setFormError("Give the key a name so you can tell it apart later.");
      return;
    }
    const days = expiresInDays.trim();
    if (days && (!/^\d+$/.test(days) || Number(days) < 1)) {
      setFormError("Expiry must be a whole number of days, or left blank.");
      return;
    }
    try {
      const result = await create.mutateAsync({
        name: trimmed,
        ...(days ? { expires_in_days: Number(days) } : {}),
      });
      setIssued({ name: result.name, token: result.token });
      setCreating(false);
      setName("");
      setExpiresInDays("");
    } catch (err) {
      setFormError((err as Error).message);
    }
  };

  const doRevoke = async () => {
    if (confirmId == null) return;
    setTopError(null);
    try {
      await revoke.mutateAsync({ id: confirmId });
    } catch (err) {
      setTopError((err as Error).message);
    } finally {
      setConfirmId(null);
    }
  };

  const rows = keys.data ?? [];

  return (
    <Card>
      <CardHeader>
        <CardTitle>API keys</CardTitle>
        <CardDescription>
          Long-lived credentials for agents and scripts — including the MCP
          server, which lets an AI assistant read and edit your forms. A key
          carries <strong>your</strong> permissions, and narrows automatically if
          your roles change.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {topError && (
          <Alert variant="destructive">
            <AlertDescription>{topError}</AlertDescription>
          </Alert>
        )}

        {issued && (
          <Alert>
            <AlertTitle>Copy “{issued.name}” now</AlertTitle>
            <AlertDescription className="space-y-2">
              <p>
                This is the only time the key is shown. We store only a hash, so
                it cannot be recovered — if you lose it, revoke it and make
                another.
              </p>
              <code className="block w-full overflow-x-auto rounded bg-muted px-3 py-2 font-mono text-xs">
                {issued.token}
              </code>
              <div className="flex gap-2 pt-1">
                <Button
                  type="button"
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    void navigator.clipboard?.writeText(issued.token);
                  }}
                >
                  Copy
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onClick={() => setIssued(null)}
                >
                  Done
                </Button>
              </div>
            </AlertDescription>
          </Alert>
        )}

        {keys.isLoading ? (
          <div className="space-y-2">
            <Skeleton className="h-8 w-full" />
            <Skeleton className="h-8 w-full" />
          </div>
        ) : keys.isError ? (
          <Alert variant="destructive">
            <AlertDescription>{(keys.error as Error).message}</AlertDescription>
          </Alert>
        ) : rows.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            No API keys yet.
          </p>
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Name</TableHead>
                <TableHead>Key</TableHead>
                <TableHead>Last used</TableHead>
                <TableHead>Expires</TableHead>
                <TableHead>Status</TableHead>
                <TableHead className="w-1" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((key) => {
                const status = statusOf(key);
                return (
                  <TableRow key={key.id}>
                    <TableCell className="font-medium">{key.name}</TableCell>
                    <TableCell className="font-mono text-xs text-muted-foreground">
                      {key.prefix}…
                    </TableCell>
                    <TableCell>{formatDate(key.last_used_at)}</TableCell>
                    <TableCell>
                      {key.expires_at ? formatDate(key.expires_at) : "Never"}
                    </TableCell>
                    <TableCell
                      className={
                        status.muted ? "text-muted-foreground" : undefined
                      }
                    >
                      {status.label}
                    </TableCell>
                    <TableCell>
                      {!key.revoked_at && (
                        <Button
                          type="button"
                          size="sm"
                          variant="ghost"
                          onClick={() => setConfirmId(key.id)}
                        >
                          Revoke
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        )}

        <Button type="button" onClick={() => setCreating(true)}>
          Create key
        </Button>
      </CardContent>

      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create an API key</DialogTitle>
            <DialogDescription>
              The key is shown once, immediately after it is created.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-4">
            {formError && (
              <Alert variant="destructive">
                <AlertDescription>{formError}</AlertDescription>
              </Alert>
            )}
            <FormField
              id="api-key-name"
              label="Name"
              hint="What will use this key, e.g. “Claude Desktop”."
            >
              <Input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Claude Desktop"
                autoFocus
              />
            </FormField>
            <FormField
              id="api-key-expiry"
              label="Expires in (days)"
              hint="Leave blank for a key that never expires."
            >
              <Input
                value={expiresInDays}
                onChange={(e) => setExpiresInDays(e.target.value)}
                placeholder="Never"
                inputMode="numeric"
              />
            </FormField>
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => setCreating(false)}
            >
              Cancel
            </Button>
            <Button
              type="button"
              onClick={() => void submit()}
              disabled={create.isPending}
            >
              {create.isPending ? "Creating…" : "Create key"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={confirmId != null}
        onOpenChange={(open) => !open && setConfirmId(null)}
        title="Revoke this key?"
        description="Anything using it stops working immediately. This cannot be undone."
        confirmLabel="Revoke"
        pending={revoke.isPending}
        onConfirm={() => void doRevoke()}
      />
    </Card>
  );
}
