// ISO 3166 catalogues, unpacked from the generated tables in `regions.data.ts`.
//
// Must stay in step with `crates/core/src/forms/regions.rs`, which parses the
// byte-identical payload — but unlike `visibility.ts` this is not a second
// hand-written copy of a spec: both sides are emitted by
// `scripts/gen-regions.mjs` from one vendored source, so drift is a generator
// bug rather than a transcription one. See that module's docs for the packed
// format and why only top-level subdivisions are included.
//
// The country list is small (~2 KB gzipped) and ships in the embed bundle,
// because the country picker is the entry point and has to render
// synchronously. The subdivision table is ~40 KB gzipped and does NOT: the
// server sends it on `PublicFormDto.regions`, and only for forms that actually
// contain a state field. `PACKED_SUBDIVISIONS` is re-exported for the admin
// preview, which builds its schema client-side and has no such budget.

import { PACKED_COUNTRIES, PACKED_SUBDIVISIONS } from "./regions.data";

export { PACKED_SUBDIVISIONS };

export interface RegionOption {
  /** ISO code — the submitted value. */
  code: string;
  /** Display name. */
  name: string;
}

function parsePairs(packed: string): RegionOption[] {
  const out: RegionOption[] = [];
  for (const entry of packed.split("|")) {
    if (!entry) continue;
    const sep = entry.indexOf(":");
    if (sep === -1) continue;
    out.push({ code: entry.slice(0, sep), name: entry.slice(sep + 1) });
  }
  return out;
}

/** Every ISO 3166-1 country as `{ code, name }`, name-sorted. */
export const COUNTRIES: readonly RegionOption[] = parsePairs(PACKED_COUNTRIES);

// One parsed table per distinct packed string. A form's blob is parsed on the
// first render that needs it and reused for every keystroke after — the map is
// keyed by the string itself so the admin preview's copy and a server-sent one
// don't collide.
const cache = new Map<string, Map<string, RegionOption[]>>();

function tableFor(packed: string): Map<string, RegionOption[]> {
  let table = cache.get(packed);
  if (table) return table;
  table = new Map();
  for (const entry of packed.split("~")) {
    if (!entry) continue;
    const sep = entry.indexOf("=");
    if (sep === -1) continue;
    table.set(entry.slice(0, sep), parsePairs(entry.slice(sep + 1)));
  }
  cache.set(packed, table);
  return table;
}

/**
 * The top-level subdivisions of `country`, or `undefined` when the table has
 * none for it.
 *
 * `undefined` covers two different situations that the renderer deliberately
 * treats alike, because both mean "there is no list to choose from": 49 of the
 * 249 countries genuinely have no ISO 3166-2 entries, and `packed` may be
 * absent entirely on a form the server didn't send a table for. Either way the
 * caller draws a text input, which is what the server accepts in both cases.
 */
export function subdivisionsFor(
  packed: string | null | undefined,
  country: string | null | undefined,
): readonly RegionOption[] | undefined {
  if (!packed || !country) return undefined;
  const list = tableFor(packed).get(country);
  return list && list.length > 0 ? list : undefined;
}
