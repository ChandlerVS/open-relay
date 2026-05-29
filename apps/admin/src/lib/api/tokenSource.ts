/**
 * Module-level token holder so the openapi-fetch middleware can read the
 * current JWT without subscribing to React state. AuthProvider keeps this
 * in sync; on 401 the response middleware clears it and emits a session
 * event so the React tree can react.
 */
import { clearToken as clearStoredToken, getToken as readStoredToken } from "../auth/storage";

let inMemoryToken: string | null = null;
let primed = false;

function primeFromStorage() {
  if (primed) return;
  primed = true;
  inMemoryToken = readStoredToken();
}

export function getCurrentToken(): string | null {
  primeFromStorage();
  return inMemoryToken;
}

export function setCurrentToken(token: string | null): void {
  primed = true;
  inMemoryToken = token;
}

export const SESSION_EXPIRED_EVENT = "open-relay:session-expired";

export function emitSessionExpired(): void {
  clearStoredToken();
  setCurrentToken(null);
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(SESSION_EXPIRED_EVENT));
  }
}
