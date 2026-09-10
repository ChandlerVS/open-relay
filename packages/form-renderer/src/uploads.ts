/**
 * Client half of the file-upload flow.
 *
 * Bytes never touch the OpenRelay API. The browser asks it for a presigned
 * `PUT`, uploads straight to the object store, and keeps the opaque *receipt*
 * the server sealed — that receipt, not a URL, is what gets submitted as the
 * field's value. The server exchanges it for the stored URL, which is why a
 * client can't nominate an arbitrary URL for a form field.
 *
 * Kept out of `Form.tsx` so the sequence reads in one place, and because the
 * error strings are the only thing a visitor ever sees when a bucket is
 * misconfigured.
 */

/** What the server hands back for one authorised upload. */
interface UploadTicket {
  upload_url: string;
  method: string;
  headers: Record<string, string>;
  token: string;
}

/** Thrown with a message meant to be shown to the visitor as-is. */
export class UploadError extends Error {}

/**
 * Presign, upload, and return the receipt to store as the field's value.
 *
 * `base` is the API root with no trailing slash, matching the two fetches in
 * `Form.tsx`.
 */
export async function uploadFile(
  base: string,
  formId: string | number,
  fieldKey: string,
  file: File,
  signal?: AbortSignal,
): Promise<string> {
  const ticket = await requestTicket(base, formId, fieldKey, file, signal);

  // `Content-Length` is deliberately absent from `headers`: it is signed into
  // the URL but is a forbidden header for `fetch`, so the browser sets it from
  // the body. A mismatch fails the store's signature check, which is what
  // actually enforces the size limit.
  let res: Response;
  try {
    res = await fetch(ticket.upload_url, {
      method: ticket.method || "PUT",
      headers: ticket.headers,
      body: file,
      signal,
    });
  } catch {
    // A CORS rejection is indistinguishable from a network failure here, and
    // a missing bucket CORS rule is by far the likeliest cause on first setup.
    throw new UploadError(
      "Could not reach the file storage service. Please try again.",
    );
  }
  if (!res.ok) {
    throw new UploadError(`Upload failed (${res.status}). Please try again.`);
  }
  return ticket.token;
}

async function requestTicket(
  base: string,
  formId: string | number,
  fieldKey: string,
  file: File,
  signal?: AbortSignal,
): Promise<UploadTicket> {
  const res = await fetch(
    `${base}/public/forms/${encodeURIComponent(String(formId))}/uploads`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      signal,
      body: JSON.stringify({
        field_key: fieldKey,
        filename: file.name,
        // Browsers report "" for a type they can't classify; the server
        // normalises that to application/octet-stream, and it has to sign
        // whatever it will actually receive.
        content_type: file.type || "application/octet-stream",
        size: file.size,
      }),
    },
  );
  if (!res.ok) {
    const body = (await res.json().catch(() => ({}))) as { error?: string };
    throw new UploadError(body.error ?? "This file could not be accepted.");
  }
  return (await res.json()) as UploadTicket;
}

/**
 * Pre-flight the file against the field's own limits so an honest visitor gets
 * an instant, specific message instead of a round trip. The server re-checks
 * everything — this is convenience, not a control.
 */
export function localRejection(
  file: File,
  accept: readonly string[] | undefined,
  maxSizeMb: number | undefined,
): string | null {
  if (file.size === 0) return "That file is empty.";
  if (maxSizeMb && file.size > maxSizeMb * 1024 * 1024) {
    return `That file is larger than ${maxSizeMb} MB.`;
  }
  if (accept && accept.length > 0 && !matchesAccept(file, accept)) {
    return `That file type isn't accepted (${accept.join(", ")}).`;
  }
  return null;
}

/**
 * Mirrors the server's `accept_matches`: an extension (`.pdf`), a wildcard
 * type (`image/*`), or a full MIME type.
 */
function matchesAccept(file: File, accept: readonly string[]): boolean {
  const name = file.name.toLowerCase();
  const type = (file.type || "").toLowerCase();
  return accept.some((raw) => {
    const p = raw.trim().toLowerCase();
    if (p.endsWith("/*")) return type.startsWith(p.slice(0, -1));
    if (p.startsWith(".")) return name.endsWith(p);
    return type === p;
  });
}
