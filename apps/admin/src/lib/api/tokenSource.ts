/**
 * Module-level token holders so the openapi-fetch layer can read the current
 * access + refresh tokens without subscribing to React state. AuthProvider
 * keeps these in sync; the client's 401-retry refreshes them, and on a failed
 * refresh `emitSessionExpired` clears both and notifies the React tree.
 */
import {
  clearToken as clearStoredToken,
  getRefreshToken as readStoredRefreshToken,
  getToken as readStoredToken,
  setRefreshToken as writeStoredRefreshToken,
  setToken as writeStoredToken,
} from "../auth/storage";

let inMemoryToken: string | null = null;
let inMemoryRefreshToken: string | null = null;
let primed = false;

function primeFromStorage() {
  if (primed) return;
  primed = true;
  inMemoryToken = readStoredToken();
  inMemoryRefreshToken = readStoredRefreshToken();
}

export function getCurrentToken(): string | null {
  primeFromStorage();
  return inMemoryToken;
}

export function setCurrentToken(token: string | null): void {
  primed = true;
  inMemoryToken = token;
}

export function getCurrentRefreshToken(): string | null {
  primeFromStorage();
  return inMemoryRefreshToken;
}

export function setCurrentRefreshToken(token: string | null): void {
  primed = true;
  inMemoryRefreshToken = token;
}

/**
 * Persist a freshly-rotated token pair to both the in-memory holders and
 * localStorage. Called by the 401-retry path after a successful refresh.
 */
export function storeTokens(token: string, refreshToken: string): void {
  setCurrentToken(token);
  setCurrentRefreshToken(refreshToken);
  writeStoredToken(token);
  writeStoredRefreshToken(refreshToken);
}

export const SESSION_EXPIRED_EVENT = "open-relay:session-expired";

export function emitSessionExpired(): void {
  clearStoredToken();
  setCurrentToken(null);
  setCurrentRefreshToken(null);
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(SESSION_EXPIRED_EVENT));
  }
}
