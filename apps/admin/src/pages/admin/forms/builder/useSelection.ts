import { useCallback, useMemo, useState } from "react";
import type { BuilderElement } from "./model";

export interface SelectMods {
  shift?: boolean;
  meta?: boolean;
}

export interface Selection {
  /** Selected ids in **document order**, never click order. */
  ids: string[];
  set: ReadonlySet<string>;
  select: (id: string, mods?: SelectMods) => void;
  replace: (ids: string[]) => void;
  selectAll: () => void;
  clear: () => void;
}

/**
 * Multi-selection over the canvas.
 *
 * Two things here are less obvious than they look.
 *
 * **The selection is pruned against the live list on every read, not trimmed on
 * write.** Nearly every mutation can invalidate an id: `normalizeRows` drops
 * stranded markers *and mints a fresh id* for a closer it has to synthesize, a
 * paste can drop a standard field it couldn't place, and a delete obviously
 * does. Deriving the live set means no mutation has to remember to tidy up, and
 * a stale id can never reach the range arithmetic below as a `-1`.
 *
 * **`ids` is always in document order.** The raw list accumulates in click
 * order, which would put a Cmd-clicked block on the clipboard scrambled.
 */
export function useSelection(items: BuilderElement[] | null): Selection {
  const [raw, setRaw] = useState<string[]>([]);
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const list = useMemo(() => items ?? [], [items]);

  const ids = useMemo(() => {
    if (raw.length === 0) return [];
    const want = new Set(raw);
    return list.filter((i) => want.has(i.id)).map((i) => i.id);
  }, [list, raw]);

  const set = useMemo(() => new Set(ids), [ids]);

  const select = useCallback(
    (id: string, mods: SelectMods = {}) => {
      if (mods.shift && anchorId && anchorId !== id) {
        const from = list.findIndex((i) => i.id === anchorId);
        const to = list.findIndex((i) => i.id === id);
        if (from !== -1 && to !== -1) {
          const [lo, hi] = from < to ? [from, to] : [to, from];
          // The anchor deliberately stays put, so a second shift-click grows or
          // shrinks the same range rather than starting a new one.
          setRaw(list.slice(lo, hi + 1).map((i) => i.id));
          return;
        }
      }
      if (mods.meta) {
        setRaw((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
        setAnchorId(id);
        return;
      }
      setRaw([id]);
      setAnchorId(id);
    },
    [list, anchorId],
  );

  const replace = useCallback((next: string[]) => {
    setRaw(next);
    setAnchorId(next[next.length - 1] ?? null);
  }, []);

  const selectAll = useCallback(() => {
    setRaw(list.map((i) => i.id));
    setAnchorId(list[0]?.id ?? null);
  }, [list]);

  const clear = useCallback(() => {
    setRaw([]);
    setAnchorId(null);
  }, []);

  return { ids, set, select, replace, selectAll, clear };
}
