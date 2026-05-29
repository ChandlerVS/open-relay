import { createContext } from "react";
import type { components } from "@open-relay/api-client";

export type AuthUser = components["schemas"]["UserDto"];

export type AuthStatus = "loading" | "anonymous" | "authenticated";

export interface AuthContextValue {
  user: AuthUser | null;
  token: string | null;
  status: AuthStatus;
  signIn: (token: string, user: AuthUser) => void;
  signOut: () => void;
}

export const AuthContext = createContext<AuthContextValue | null>(null);
