import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { api } from "../api/client";
import {
  SESSION_EXPIRED_EVENT,
  getCurrentToken,
  setCurrentToken,
} from "../api/tokenSource";
import { clearToken, setToken } from "./storage";
import {
  AuthContext,
  type AuthContextValue,
  type AuthStatus,
  type AuthUser,
  type Permission,
  type RoleSummary,
} from "./AuthContext";

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [permissions, setPermissions] = useState<Permission[]>([]);
  const [roles, setRoles] = useState<RoleSummary[]>([]);
  const [token, setTokenState] = useState<string | null>(() => getCurrentToken());
  const [status, setStatus] = useState<AuthStatus>(() =>
    getCurrentToken() ? "loading" : "anonymous",
  );

  // Single source of session truth. Called on initial hydrate, after signIn
  // (to populate the permission set that LoginResponse intentionally omits),
  // and on window focus so permission changes propagate without a re-login.
  const fetchSession = useCallback(async () => {
    const { data, error, response } = await api.client.GET("/auth/me");
    if (data) {
      setUser(data.user);
      setPermissions(data.permissions);
      setRoles(data.roles);
      setStatus("authenticated");
      return;
    }
    if (response.status === 401 || error) {
      clearToken();
      setCurrentToken(null);
      setTokenState(null);
      setUser(null);
      setPermissions([]);
      setRoles([]);
      setStatus("anonymous");
    }
  }, []);

  // Initial hydrate when bootstrapped with a stored token.
  useEffect(() => {
    if (status !== "loading") return;
    let cancelled = false;
    (async () => {
      if (cancelled) return;
      await fetchSession();
    })();
    return () => {
      cancelled = true;
    };
  }, [status, fetchSession]);

  // Refetch when the tab regains focus — surfaces role changes within
  // seconds rather than waiting for the next sign-in. The 24h JWT TTL still
  // governs hard revocation; this is best-effort propagation.
  useEffect(() => {
    if (status !== "authenticated") return;
    const onFocus = () => {
      void fetchSession();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [status, fetchSession]);

  // React to 401 from anywhere in the app.
  useEffect(() => {
    const onExpired = () => {
      setTokenState(null);
      setUser(null);
      setPermissions([]);
      setRoles([]);
      setStatus("anonymous");
    };
    window.addEventListener(SESSION_EXPIRED_EVENT, onExpired);
    return () => window.removeEventListener(SESSION_EXPIRED_EVENT, onExpired);
  }, []);

  const signIn = useCallback(
    (nextToken: string, nextUser: AuthUser) => {
      setToken(nextToken);
      setCurrentToken(nextToken);
      setTokenState(nextToken);
      setUser(nextUser);
      setStatus("authenticated");
      // Background: fetch the full session so `permissions`/`roles` populate
      // and the user shape is refreshed with role badges.
      void fetchSession();
    },
    [fetchSession],
  );

  const signOut = useCallback(() => {
    clearToken();
    setCurrentToken(null);
    setTokenState(null);
    setUser(null);
    setPermissions([]);
    setRoles([]);
    setStatus("anonymous");
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({ user, token, status, permissions, roles, signIn, signOut }),
    [user, token, status, permissions, roles, signIn, signOut],
  );
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
