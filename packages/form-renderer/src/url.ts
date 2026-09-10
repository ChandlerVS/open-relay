/**
 * URL checks shared by everything in this package that hands an
 * author-supplied string to the browser.
 *
 * This lives in its own module because it has two callers now — the
 * post-submission redirect in `Form.tsx` and every link in a rich-text block —
 * and a second copy-paste of a security check is exactly the
 * `visibility.rs`/`visibility.ts` hazard CLAUDE.md warns about.
 */

/** Whitespace and C0/DEL control characters, in any position. */
const UNSAFE = /[\s\u0000-\u001f\u007f]/;

/**
 * Absolute http(s) only. Mirrors `service::validate_redirect_url` on the
 * server; duplicated here because these values reach `window.location` and
 * `<a href>` on a page we don't own.
 *
 * **Do not widen this.** A `javascript:` or `data:` href is live, React does
 * not sanitise `href`, and whitespace and control characters are stripped by
 * browsers during URL parsing — which is why they are rejected before the
 * scheme is even inspected, or `java\nscript:` re-forms into a working scheme.
 */
export function isHttpUrl(url: string): boolean {
  const trimmed = url.trim();
  if (trimmed !== url || url === "") return false;
  if (UNSAFE.test(url)) return false;
  const lower = url.toLowerCase();
  const rest = lower.startsWith("https://")
    ? lower.slice(8)
    : lower.startsWith("http://")
      ? lower.slice(7)
      : null;
  if (rest === null || rest === "") return false;
  // Also rejects scheme-relative "//evil.example", which would otherwise
  // inherit the host page's scheme and navigate off-site.
  return !/^[/?#]/.test(rest);
}
