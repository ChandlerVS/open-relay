import { createContext } from "react";
import type { components } from "@open-relay/api-client";

export type AuthUser = components["schemas"]["UserDto"];
export type Permission = components["schemas"]["Permission"];
export type RoleSummary = components["schemas"]["RoleSummary"];

export type AuthStatus = "loading" | "anonymous" | "authenticated";

export interface AuthContextValue {
  user: AuthUser | null;
  token: string | null;
  status: AuthStatus;
  /**
   * Flat set of permissions the current user holds across all assigned roles.
   * Hydrated from `/auth/me`. Empty until status is `"authenticated"` AND the
   * background session fetch has completed.
   */
  permissions: Permission[];
  roles: RoleSummary[];
  signIn: (token: string, user: AuthUser) => void;
  /**
   * Token-only sign in for flows (like OAuth) where the server hands us a JWT
   * without a user payload. Stores the token then loads the session shape via
   * `/auth/me`.
   */
  signInWithToken: (token: string) => Promise<void>;
  signOut: () => void;
}

export const AuthContext = createContext<AuthContextValue | null>(null);
