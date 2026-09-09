import type { ReactNode } from "react";
import type { Permission } from "./AuthContext";

export interface PermissionNoticeProps {
  perm: Permission;
  /** What the permission would let them do here, e.g. "change the delivery
   *  destinations". Reads as "You need `x:read` to <action>." */
  action: string;
  /**
   * What is already configured, when the control is hiding a live value. The
   * surrounding form submits its untouched state either way, so saying so is
   * what stops this reading as "your selection was dropped".
   */
  current?: ReactNode;
}

/**
 * Inline stand-in for a control whose data the current user may not read.
 * Deliberately muted, not a destructive `Alert` — lacking a permission is a
 * normal state of the app, not a fault the user should try to recover from.
 */
export function PermissionNotice({ perm, action, current }: PermissionNoticeProps) {
  return (
    <div className="rounded-md border border-dashed border-border px-3 py-2 text-xs text-muted-foreground space-y-1">
      {current != null && <div className="text-foreground">{current}</div>}
      <div>
        You need{" "}
        <code className="rounded bg-muted px-1 py-0.5 font-mono">{perm}</code> to{" "}
        {action}.
      </div>
    </div>
  );
}
