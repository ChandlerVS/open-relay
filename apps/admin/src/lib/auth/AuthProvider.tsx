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
} from "./AuthContext";

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [token, setTokenState] = useState<string | null>(() => getCurrentToken());
  const [status, setStatus] = useState<AuthStatus>(() =>
    getCurrentToken() ? "loading" : "anonymous",
  );

  // Hydrate user on mount if we have a stored token. The API client picks
  // up the token from the module-level holder, so no header wiring here.
  useEffect(() => {
    if (status !== "loading") return;
    let cancelled = false;
    (async () => {
      const { data, error, response } = await api.client.GET("/auth/me");
      if (cancelled) return;
      if (data) {
        setUser(data);
        setStatus("authenticated");
        return;
      }
      // 401 → middleware already cleared storage + token source; just reflect it.
      if (response.status === 401 || error) {
        clearToken();
        setCurrentToken(null);
        setTokenState(null);
        setUser(null);
        setStatus("anonymous");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [status]);

  // React to 401 from anywhere in the app.
  useEffect(() => {
    const onExpired = () => {
      setTokenState(null);
      setUser(null);
      setStatus("anonymous");
    };
    window.addEventListener(SESSION_EXPIRED_EVENT, onExpired);
    return () => window.removeEventListener(SESSION_EXPIRED_EVENT, onExpired);
  }, []);

  const signIn = useCallback((nextToken: string, nextUser: AuthUser) => {
    setToken(nextToken);
    setCurrentToken(nextToken);
    setTokenState(nextToken);
    setUser(nextUser);
    setStatus("authenticated");
  }, []);

  const signOut = useCallback(() => {
    clearToken();
    setCurrentToken(null);
    setTokenState(null);
    setUser(null);
    setStatus("anonymous");
  }, []);

  const value = useMemo<AuthContextValue>(
    () => ({ user, token, status, signIn, signOut }),
    [user, token, status, signIn, signOut],
  );
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}
