import { useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
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
  Skeleton,
} from "@open-relay/ui";
import {
  useMyIdentities,
  useOAuthPublicConfig,
  useStartLink,
  useUnlinkIdentity,
} from "../../../lib/oauth/useOAuth";
import { useAuth } from "../../../lib/auth/useAuth";

export function ProfilePage() {
  const { user } = useAuth();
  const identities = useMyIdentities();
  const pub = useOAuthPublicConfig();
  const unlink = useUnlinkIdentity();
  const startLinkMutation = useStartLink();
  const [search, setSearch] = useSearchParams();
  const [topError, setTopError] = useState<string | null>(null);
  const [topSuccess, setTopSuccess] = useState<string | null>(null);
  const [confirmId, setConfirmId] = useState<number | null>(null);

  useEffect(() => {
    if (search.get("linked") === "ok") {
      setTopSuccess("Account linked.");
      search.delete("linked");
      setSearch(search, { replace: true });
    }
  }, [search, setSearch]);

  const hasLinkForActive =
    pub.data?.enabled &&
    identities.data &&
    identities.data.length > 0;

  const startLink = async () => {
    setTopError(null);
    setTopSuccess(null);
    try {
      const url = await startLinkMutation.mutateAsync();
      window.location.href = url;
    } catch (err) {
      setTopError((err as Error).message);
    }
  };

  const doUnlink = async () => {
    if (confirmId == null) return;
    setTopError(null);
    setTopSuccess(null);
    try {
      await unlink.mutateAsync({ id: confirmId });
      setTopSuccess("Account unlinked.");
      setConfirmId(null);
    } catch (err) {
      setTopError((err as Error).message);
      setConfirmId(null);
    }
  };

  return (
    <div className="space-y-6 max-w-2xl">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Profile</h1>
        <p className="text-sm text-muted-foreground">
          Your account and any linked identity providers.
        </p>
      </div>

      {topError && (
        <Alert variant="destructive">
          <AlertTitle>Error</AlertTitle>
          <AlertDescription>{topError}</AlertDescription>
        </Alert>
      )}
      {topSuccess && (
        <Alert>
          <AlertTitle>OK</AlertTitle>
          <AlertDescription>{topSuccess}</AlertDescription>
        </Alert>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Account</CardTitle>
        </CardHeader>
        <CardContent className="space-y-1 text-sm">
          <div>
            <span className="text-muted-foreground">Email:</span>{" "}
            <span>{user?.email}</span>
          </div>
          {user?.display_name && (
            <div>
              <span className="text-muted-foreground">Display name:</span>{" "}
              <span>{user.display_name}</span>
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Linked accounts</CardTitle>
          <CardDescription>
            Identity providers connected to your account.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {identities.isPending ? (
            <Skeleton className="h-16 w-full" />
          ) : identities.data && identities.data.length > 0 ? (
            <ul className="divide-y divide-border rounded border border-border">
              {identities.data.map((id) => (
                <li
                  key={id.id}
                  className="flex items-center justify-between px-3 py-2 text-sm"
                >
                  <div>
                    <div className="font-medium">{id.provider_display_name}</div>
                    {id.email_at_link && (
                      <div className="text-xs text-muted-foreground">
                        {id.email_at_link}
                      </div>
                    )}
                  </div>
                  <Button
                    size="sm"
                    variant="outline"
                    onClick={() => setConfirmId(id.id)}
                  >
                    Unlink
                  </Button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted-foreground">No linked accounts.</p>
          )}

          {pub.data?.enabled && !hasLinkForActive && (
            <Button type="button" onClick={startLink}>
              Link {pub.data.display_name ?? "OAuth"} account
            </Button>
          )}
        </CardContent>
      </Card>

      <ConfirmDialog
        open={confirmId != null}
        onOpenChange={(open) => !open && setConfirmId(null)}
        title="Unlink this account?"
        description="If this is your only sign-in method and you have no password, you'll be locked out."
        confirmLabel="Unlink"
        confirmVariant="destructive"
        pending={unlink.isPending}
        onConfirm={doUnlink}
      />
    </div>
  );
}
