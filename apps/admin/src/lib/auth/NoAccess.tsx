import { Link } from "react-router-dom";
import { Lock } from "lucide-react";
import { Button, Card, CardContent } from "@open-relay/ui";
import type { Permission } from "./AuthContext";

export interface NoAccessProps {
  /** The permission the caller was missing. Named verbatim so the user can
   *  quote it to whoever administers their roles. */
  perm: Permission;
  /** What they were trying to reach, e.g. "view users". */
  action?: string;
}

/**
 * Terminal "you don't hold this permission" panel. Rendered *in place* rather
 * than redirected to, so a shared URL still explains itself instead of
 * silently landing the recipient on the dashboard.
 */
export function NoAccess({ perm, action }: NoAccessProps) {
  return (
    <div className="max-w-xl">
      <Card>
        <CardContent className="flex flex-col items-start gap-3 py-8">
          <div className="flex items-center gap-2">
            <Lock className="h-5 w-5 text-muted-foreground" />
            <h1 className="text-lg font-semibold tracking-tight">
              You don't have access
            </h1>
          </div>
          <p className="text-sm text-muted-foreground">
            {action ? `To ${action} you need` : "This page requires"} the{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 text-xs font-mono text-foreground">
              {perm}
            </code>{" "}
            permission. Ask an administrator to add it to one of your roles.
          </p>
          <Button variant="outline" size="sm" asChild>
            <Link to="/">Back to dashboard</Link>
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}
