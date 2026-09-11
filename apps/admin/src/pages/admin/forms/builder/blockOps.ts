import {
  MAX_CUSTOM_FIELDS,
  MAX_KEY_LEN,
  MAX_LAYOUT_ELEMENTS,
  MAX_PAGES,
  STANDARD_KEYS,
  allowedInRow,
  countryFieldRef,
  elementKey,
  elementRule,
  isCountryField,
  isRowMarker,
  newId,
  normalizeRows,
  renameCountryReferences,
  renameRuleReferences,
  renumberPositions,
  rowPartner,
  stripCountryReferences,
  stripRuleReferences,
  withCountryField,
  withRule,
  type BuilderElement,
  type FormElement,
} from "./model";

/**
 * Whole-block operations: lifting a run of elements out of one form and
 * grafting it into another.
 *
 * The reason this is more than an `array.splice` is that an explicit `layout`
 * write gets **no server-side repair**. `strip_dangling_rules` and
 * `strip_dangling_country_refs` (`service.rs`) run only on the legacy
 * `standard_fields`/`custom_fields` paths, on the grounds that a caller sending
 * a `layout` sent the rules too and deserves to be told what is wrong with
 * them. A pasted block is the one case where that reasoning doesn't hold: the
 * rules came from a *different form*, and the author never typed them here. So
 * the repair the server declines to do happens at paste time instead, and the
 * layout that reaches `PATCH /forms/{id}` is already valid.
 */

/** Elements in document order, deep-copied and with their row markers balanced. */
export function extractBlock(items: BuilderElement[], ids: ReadonlySet<string>): FormElement[] {
  const picked = items
    .filter((i) => ids.has(i.id))
    // `structuredClone` rather than a spread: `divider` and `row_end` have no
    // `config` member for a spread to copy, and a shallow one would leave the
    // copy's `options`/`accept` arrays aliased to the original's.
    .map((i) => ({ id: i.id, element: structuredClone(i.element) }));
  // Balance the markers *before* the payload leaves this form. A half-copied
  // row would otherwise carry a stray `row_end` that closes whatever row it
  // lands in on the other side, where nothing can tell it apart from a real one.
  return normalizeRows(picked).map((i) => i.element);
}

export interface PasteOutcome {
  /** The whole new layout. Unchanged from `target` when `refused` is set. */
  items: BuilderElement[];
  /** Ids of the elements that landed, to select afterwards. */
  pastedIds: string[];
  /** Standard keys dropped because the target already had them. */
  droppedStandard: string[];
  /** Custom keys that had to be renamed, old → new. */
  renamed: Record<string, string>;
  /** References repaired because what they named didn't come along. */
  repaired: number;
  /** Set when a cap would be breached; nothing was applied. */
  refused: string | null;
}

/**
 * Graft `incoming` into `target` at `insertIndex`, repairing everything the
 * server would otherwise reject.
 *
 * Pure, and the order of the passes is load-bearing — see the comments on each.
 */
export function prepareForPaste(
  incoming: FormElement[],
  target: BuilderElement[],
  insertIndex: number,
): PasteOutcome {
  let block: BuilderElement[] = incoming.map((element) => ({
    id: newId(),
    element: structuredClone(element),
  }));

  // 1. A standard field is a singleton per form, so one already on the target
  //    has nowhere to go. Dropping it beats refusing the whole paste: the rest
  //    of the block is still what the author asked for.
  const usedStandard = new Set<string>();
  for (const item of target) {
    if (item.element.element === "standard") usedStandard.add(item.element.config.key);
  }
  const droppedStandard: string[] = [];
  block = block.filter(({ element }) => {
    if (element.element !== "standard") return true;
    if (usedStandard.has(element.config.key)) {
      droppedStandard.push(element.config.key);
      return false;
    }
    usedStandard.add(element.config.key);
    return true;
  });

  // 2. Re-key colliding custom fields. `taken` is seeded with the block's own
  //    keys as well as the target's, so a generated name can never collide with
  //    a key still to be processed — which would make the rename map ambiguous
  //    and repoint the wrong element's rules in pass 3.
  const targetKeys = new Set<string>(STANDARD_KEYS);
  for (const item of target) {
    if (item.element.element !== "custom") continue;
    const key = item.element.config.key.trim();
    if (key) targetKeys.add(key);
  }
  const taken = new Set<string>(targetKeys);
  for (const { element } of block) {
    if (element.element !== "custom") continue;
    const key = element.config.key.trim();
    if (key) taken.add(key);
  }
  const renamed: Record<string, string> = {};
  block = block.map((item) => {
    const { element } = item;
    if (element.element !== "custom") return item;
    const key = element.config.key.trim();
    // A blank key is a freshly added field that was never named; leave it be
    // and let the inline validation ask for a name, as it does everywhere else.
    if (!key || !targetKeys.has(key)) return item;
    const next = uniqueKey(key, taken);
    taken.add(next);
    renamed[key] = next;
    return { ...item, element: { ...element, config: { ...element.config, key: next } } };
  });

  // 3. Repoint the block's own rules and country bindings at the new keys, so a
  //    block pasted twice into the same form stays two independent copies
  //    rather than the second one steering off the first.
  for (const [from, to] of Object.entries(renamed)) {
    block = renameRuleReferences(block, from, to);
    block = renameCountryReferences(block, from, to);
  }

  // 4. Splice, then normalise. `normalizeRows` is what repairs a block landing
  //    inside an open row, and it can only be run once everything is in place.
  const at = insertionPoint(target, insertIndex, block);
  const spliced = normalizeRows([...target.slice(0, at), ...block, ...target.slice(at)]);

  // 5. Caps come *after* normalisation, which can synthesize a closing marker
  //    for an unclosed row and so add an element the pre-check never saw.
  const refused = capBreach(spliced);
  if (refused) {
    return {
      items: target,
      pastedIds: [],
      droppedStandard: [],
      renamed: {},
      repaired: 0,
      refused,
    };
  }

  // 6. Repair what the block still names but no longer has.
  const pasted = new Set(block.map((i) => i.id));
  const { items, repaired } = repairReferences(spliced, pasted);

  return {
    items: renumberPositions(items),
    pastedIds: items.filter((i) => pasted.has(i.id)).map((i) => i.id),
    droppedStandard,
    renamed,
    repaired,
    refused: null,
  };
}

/**
 * `base_2`, `base_3`, … The server's only rules for a key are 1..=64 characters
 * with no whitespace or control characters, so a suffix is always legal; the
 * truncation counts **code points**, because the server counts `chars()`.
 */
function uniqueKey(base: string, taken: ReadonlySet<string>): string {
  for (let n = 2; ; n += 1) {
    const suffix = `_${n}`;
    const chars = [...base];
    const room = MAX_KEY_LEN - suffix.length;
    const candidate = (chars.length > room ? chars.slice(0, room).join("") : base) + suffix;
    if (!taken.has(candidate)) return candidate;
  }
}

/**
 * Where the block actually lands.
 *
 * A row holds fields and nothing else, so a block carrying a heading or a page
 * break cannot be dropped into the middle of one — `normalizeRows` would repair
 * it by splitting the row in two, which is not what anyone meant by "paste
 * after this field". Push past the row's closer instead.
 */
function insertionPoint(
  target: BuilderElement[],
  index: number,
  block: BuilderElement[],
): number {
  const at = Math.max(0, Math.min(index, target.length));
  if (block.every((i) => allowedInRow(i.element))) return at;
  let open = false;
  for (let i = 0; i < at; i += 1) {
    const kind = target[i]!.element.element;
    if (kind === "row_start") open = true;
    else if (kind === "row_end") open = false;
  }
  if (!open) return at;
  for (let i = at; i < target.length; i += 1) {
    if (target[i]!.element.element === "row_end") return i + 1;
  }
  return target.length;
}

/**
 * The three whole-form ceilings. None of these is reported per element by
 * `validateLayout`, so breaching one would surface as an opaque 400 on save
 * naming nothing the author could click — worth refusing up front instead.
 */
function capBreach(items: BuilderElement[]): string | null {
  if (items.length > MAX_LAYOUT_ELEMENTS) {
    return `That would take this form past ${MAX_LAYOUT_ELEMENTS} elements, which is the most one form can hold.`;
  }
  const customs = items.filter((i) => i.element.element === "custom").length;
  if (customs > MAX_CUSTOM_FIELDS) {
    return `That would take this form past ${MAX_CUSTOM_FIELDS} custom fields, which is the most one form can hold.`;
  }
  const pages = items.filter((i) => i.element.element === "page_break").length + 1;
  if (pages > MAX_PAGES) {
    return `That would take this form past ${MAX_PAGES} steps, which is the most one form can have.`;
  }
  return null;
}

/**
 * Drop what the pasted elements name but cannot reach, in one forward pass.
 *
 * The maps mirror the ones `validate.ts` builds — key → is-it-a-checkbox, plus
 * the country-valued subset — because agreeing with the inline validation *is*
 * the requirement. Three things get repaired, and all three are references
 * whose target stayed behind in the source form:
 *
 *  - a condition naming a field that doesn't appear strictly earlier, or that
 *    does but isn't a checkbox when the operator demands one;
 *  - a state picker's `country_field` naming something that isn't an earlier
 *    country picker, which falls back to free text — exactly what
 *    `strip_dangling_country_refs` does server-side;
 *  - a standard `state` dropdown with no standard `country` dropdown ahead of
 *    it, which the server rejects outright and which downgrades to free text
 *    for the same reason.
 *
 * Only pasted elements are touched. An element already on the form had valid
 * references a moment ago and still does — nothing moved above it.
 */
function repairReferences(
  items: BuilderElement[],
  pasted: ReadonlySet<string>,
): { items: BuilderElement[]; repaired: number } {
  const seen = new Map<string, boolean>();
  const countries = new Set<string>();
  let repaired = 0;

  const out = items.map((item) => {
    let el = item.element;

    if (pasted.has(item.id)) {
      const rule = elementRule(el);
      if (rule) {
        const kept = rule.conditions.filter((c) => {
          if (!seen.has(c.field)) return false;
          const checkboxOp = c.op === "is_checked" || c.op === "is_not_checked";
          return !checkboxOp || seen.get(c.field) === true;
        });
        if (kept.length !== rule.conditions.length) {
          repaired += 1;
          el = withRule(el, kept.length ? { ...rule, conditions: kept } : null);
        }
      }

      const parent = countryFieldRef(el);
      if (parent && !countries.has(parent)) {
        repaired += 1;
        el = withCountryField(el, undefined);
      }

      if (
        el.element === "standard" &&
        el.config.key === "state" &&
        el.config.input_override === "select" &&
        !countries.has("country")
      ) {
        repaired += 1;
        // Delete the key rather than writing null, like `withCountryField` —
        // the dirty check is a `JSON.stringify` comparison.
        const { input_override: _drop, ...rest } = el.config;
        el = { element: "standard", config: rest };
      }
    }

    // Recorded *after* the checks, so a self-reference reads as "does not
    // appear earlier" — the same ordering `validate.ts` and the server use.
    if (el.element === "standard") seen.set(el.config.key, false);
    if (el.element === "custom" && el.config.key.trim()) {
      seen.set(el.config.key, el.config.type === "checkbox");
    }
    if (isCountryField(el)) {
      countries.add(el.element === "custom" ? el.config.key : "country");
    }

    return el === item.element ? item : { ...item, element: el };
  });

  return { items: out, repaired };
}

/**
 * Delete elements, repairing whatever pointed at them.
 *
 * One path serves the trash button and a bulk delete alike: a row marker always
 * takes its partner, because half a pair is a layout the server rejects and
 * "un-row these fields" is the only reading a delete could have. The fields it
 * held stay exactly where they are and go back to being full width.
 */
export function removeMany(
  items: BuilderElement[],
  ids: ReadonlySet<string>,
): BuilderElement[] {
  const drop = new Set(ids);
  for (const item of items) {
    if (!drop.has(item.id) || !isRowMarker(item.element)) continue;
    const partner = rowPartner(items, item.id);
    if (partner) drop.add(partner);
  }

  const orphanedKeys = items
    .filter((i) => drop.has(i.id))
    .map((i) => elementKey(i.element))
    .filter((k): k is string => k !== null && k !== "");

  let next = items.filter((i) => !drop.has(i.id));
  for (const key of orphanedKeys) {
    // Same repair the server applies on a legacy write: a rule that named a
    // deleted field loses that condition, and a state picker it drove goes back
    // to free text. Without this, deleting a controller leaves the form in a
    // state that will not save.
    next = stripCountryReferences(stripRuleReferences(next, key), key);
  }
  // A row emptied by the delete is cosmetic debris; `normalizeRows` drops it
  // quietly, exactly as `normalize_layout` does server-side.
  return renumberPositions(normalizeRows(next));
}
