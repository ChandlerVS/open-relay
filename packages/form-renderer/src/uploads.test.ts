import { test } from "node:test";
import assert from "node:assert/strict";

import { localRejection } from "./uploads.ts";

/**
 * `localRejection` mirrors the server's `accept_matches` + size check. Drift
 * here is a UX bug, never a hole: the server re-validates everything, and the
 * signed `Content-Length`/`Content-Type` on the presigned URL are what
 * actually enforce the limits. These tests exist so an honest visitor keeps
 * getting the instant, specific message instead of a round trip.
 */

/** Minimal stand-in — `localRejection` only reads name/size/type. */
function file(name: string, size: number, type = ""): File {
  return { name, size, type } as File;
}

test("an empty list accepts anything", () => {
  assert.equal(localRejection(file("x.exe", 10), [], 5), null);
  assert.equal(localRejection(file("x.exe", 10), undefined, 5), null);
});

test("rejects an empty file", () => {
  assert.match(localRejection(file("x.pdf", 0), [], 5) ?? "", /empty/i);
});

test("rejects a file over the field's cap, and names the cap", () => {
  const tooBig = localRejection(file("x.pdf", 6 * 1024 * 1024), [], 5);
  assert.match(tooBig ?? "", /5 MB/);
  // Exactly at the limit is fine — the bound is inclusive on both sides.
  assert.equal(localRejection(file("x.pdf", 5 * 1024 * 1024), [], 5), null);
});

test("matches an extension case-insensitively", () => {
  assert.equal(localRejection(file("CV.PDF", 10, "application/pdf"), [".pdf"], 5), null);
  assert.notEqual(localRejection(file("notes.txt", 10, "text/plain"), [".pdf"], 5), null);
});

test("matches a wildcard MIME type", () => {
  assert.equal(localRejection(file("a.png", 10, "image/png"), ["image/*"], 5), null);
  assert.notEqual(localRejection(file("a.pdf", 10, "application/pdf"), ["image/*"], 5), null);
});

test("matches a full MIME type exactly", () => {
  assert.equal(localRejection(file("d.csv", 10, "text/csv"), ["text/csv"], 5), null);
  // Same extension, different reported type — the type is what's checked.
  assert.notEqual(localRejection(file("d.csv", 10, "text/plain"), ["text/csv"], 5), null);
});

test("any one pattern in the list is enough", () => {
  const accept = [".pdf", "image/*"];
  assert.equal(localRejection(file("a.pdf", 10, "application/pdf"), accept, 5), null);
  assert.equal(localRejection(file("b.png", 10, "image/png"), accept, 5), null);
  assert.notEqual(localRejection(file("c.zip", 10, "application/zip"), accept, 5), null);
});

test("a file the browser cannot type still matches by extension", () => {
  // Browsers report "" for unknown types; an extension rule must still work.
  assert.equal(localRejection(file("resume.pdf", 10, ""), [".pdf"], 5), null);
});
