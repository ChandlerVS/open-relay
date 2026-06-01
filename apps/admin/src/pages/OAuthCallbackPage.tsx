import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Alert, AlertDescription, AlertTitle, Skeleton } from "@open-relay/ui";
import { useAuth } from "../lib/auth/useAuth";

/**
 * Lands here after the OAuth provider redirects the user back through the
 * server's `/auth/oauth/callback`. The server packed the outcome into either
 * the URL fragment (sign-in: contains a JWT) or the query string (link mode).
 *
 * Reads it, calls into the auth context, and bounces forward — clearing the
 * token out of the URL bar before any navigation history is written.
 */
export function OAuthCallbackPage() {
  const navigate = useNavigate();
  const { signInWithToken } = useAuth();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const hash = window.location.hash.startsWith("#")
      ? window.location.hash.slice(1)
      : window.location.hash;
    const fragment = new URLSearchParams(hash);
    const search = new URLSearchParams(window.location.search);
    const mode = fragment.get("mode") ?? search.get("mode") ?? "signin";
    const errMsg = fragment.get("error") ?? search.get("error");

    // Always strip the token/error from the URL bar.
    window.history.replaceState(null, "", "/oauth/callback");

    if (errMsg) {
      setError(errMsg);
      return;
    }

    if (mode === "link") {
      navigate("/profile?linked=ok", { replace: true });
      return;
    }

    const token = fragment.get("token");
    const refreshToken = fragment.get("refresh");
    if (!token || !refreshToken) {
      setError("Missing token in callback response.");
      return;
    }
    void signInWithToken(token, refreshToken).then(() => {
      navigate("/", { replace: true });
    });
  }, [navigate, signInWithToken]);

  if (error) {
    return (
      <main className="min-h-screen grid place-items-center bg-background text-foreground p-8">
        <div className="max-w-md w-full space-y-4">
          <Alert variant="destructive">
            <AlertTitle>Sign-in failed</AlertTitle>
            <AlertDescription>{error}</AlertDescription>
          </Alert>
          <a
            href="/login"
            className="block text-sm text-primary underline underline-offset-4"
          >
            Back to sign in
          </a>
        </div>
      </main>
    );
  }

  return (
    <main className="min-h-screen grid place-items-center bg-background text-foreground p-8">
      <div className="max-w-md w-full space-y-3 text-center">
        <Skeleton className="h-6 w-40 mx-auto" />
        <p className="text-sm text-muted-foreground">Finishing sign in…</p>
      </div>
    </main>
  );
}
