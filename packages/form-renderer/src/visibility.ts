import type { Condition, FormElement, VisibilityRule } from "./schema";
import type { FormPage, LayoutEntry } from "./layout";

/**
 * Evaluating a layout's conditional-visibility rules.
 *
 * This is one of two copies of a single spec — the other is
 * `crates/core/src/forms/visibility.rs`, which the server runs to decide what a
 * submission is allowed to omit and which answers to throw away. **They must
 * agree exactly**, or a visitor fills in one form while the server validates a
 * different one. Any change here is a change there. That module's doc comment
 * is the authoritative statement of the rules; the short version:
 *
 * - A value canonicalises to a string: strings trim, booleans become
 *   `"true"`/`"false"`, numbers stringify, absent/null is `""`.
 * - `equals`/`not_equals`/`contains` compare case-insensitively.
 * - `is_checked` is "truthy" using the same vocabulary the server's submission
 *   coercion accepts (`true`/`on`/`yes`/`1`); `is_not_checked` is its negation,
 *   deliberately not "equals false" — an unanswered checkbox is empty, never
 *   `false`.
 * - One forward pass: a condition naming a controller that is *itself hidden*
 *   is false, which is what makes hiding transitive and what stops a hidden
 *   field's prefilled `default_value` from steering a later element.
 *
 * Dependency-free on purpose: this ships inside the embed bundle.
 */

export type Values = Record<string, string | boolean>;

export function canonical(value: string | boolean | undefined): string {
  if (typeof value === "boolean") return value ? "true" : "false";
  if (typeof value === "string") return value.trim();
  return "";
}

function isTruthy(value: string): boolean {
  const v = value.toLowerCase();
  return v === "true" || v === "on" || v === "yes" || v === "1";
}

function evalCondition(
  cond: Condition,
  values: Values,
  visibleKeys: Map<string, boolean>,
): boolean {
  if (!visibleKeys.get(cond.field)) return false;
  const actual = canonical(values[cond.field]);
  const operand = cond.value ?? "";
  switch (cond.op) {
    case "equals":
      return actual.toLowerCase() === operand.toLowerCase();
    case "not_equals":
      return actual.toLowerCase() !== operand.toLowerCase();
    case "contains":
      return actual.toLowerCase().includes(operand.toLowerCase());
    case "is_empty":
      return actual === "";
    case "is_not_empty":
      return actual !== "";
    case "is_checked":
      return isTruthy(actual);
    case "is_not_checked":
      return !isTruthy(actual);
  }
}

function evalRule(
  rule: VisibilityRule,
  values: Values,
  visibleKeys: Map<string, boolean>,
): boolean {
  const test = (c: Condition) => evalCondition(c, values, visibleKeys);
  return (rule.match ?? "all") === "any"
    ? rule.conditions.some(test)
    : rule.conditions.every(test);
}

/** The rule an element carries, if its kind can carry one. */
function ruleOf(el: FormElement): VisibilityRule | null {
  switch (el.element) {
    case "standard":
    case "custom":
    case "heading":
    case "paragraph":
      return el.config.visible_when ?? null;
    default:
      return null;
  }
}

export interface Visibility {
  /** Parallel to the layout: is element `i` shown? */
  visible: boolean[];
  /** Submission keys whose field is hidden, so their answers must not be sent. */
  hiddenKeys: Set<string>;
}

/**
 * Resolve every element's visibility in one forward pass over the layout.
 *
 * `values` should already have defaults folded in — the caller passes
 * `{...defaults, ...typed}` rather than raw state, because the renderer's
 * prefill lands in an effect and a controller with a `default_value` would
 * otherwise read as empty on the first committed frame and flash its dependents
 * into view on the second.
 */
export function computeVisibility(layout: FormElement[], values: Values): Visibility {
  const visibleKeys = new Map<string, boolean>();
  const visible: boolean[] = [];
  const hiddenKeys = new Set<string>();

  for (const el of layout) {
    const rule = ruleOf(el);
    const shown = rule ? evalRule(rule, values, visibleKeys) : true;
    if (el.element === "standard" || el.element === "custom") {
      visibleKeys.set(el.config.key, shown);
      if (!shown) hiddenKeys.add(el.config.key);
    }
    visible.push(shown);
  }
  return { visible, hiddenKeys };
}

/**
 * A page's elements paired with their layout index, filtered to the visible
 * ones, with decoration that no longer decorates anything removed.
 *
 * Neither a `divider` nor a row marker can carry a rule of its own (see
 * `schema.ts`), so hiding the block around one would otherwise leave it
 * floating between unrelated fields — or, for a row, leave an empty flex box
 * contributing a gap. Collapsing leading, trailing and doubled dividers, and
 * rows whose every field has hidden, is the cheap fix, and it is purely
 * presentational: nothing here affects what is submitted.
 *
 * Doing the row collapse *here* rather than at render time is load-bearing for
 * multi-step forms. `Form.tsx` decides which steps are live by asking whether a
 * page has any visible elements left, so a page holding nothing but an
 * all-hidden row has to come back empty or it would count as a step with
 * nothing on it.
 *
 * The layout index rides along so React keys stay stable: keying decoration by
 * its position in the *filtered* list would remount a heading whenever a
 * sibling appeared or disappeared.
 */
export function visibleElements(page: FormPage, visible: boolean[]): LayoutEntry[] {
  const kept = page.elements
    .map((el, i) => ({ el, index: page.offset + i }))
    .filter(({ index }) => visible[index] !== false);

  // Rows first: a row that collapses can strand a divider beside it, and the
  // divider pass below should see the list as it will actually render.
  const rows: LayoutEntry[] = [];
  for (let i = 0; i < kept.length; i += 1) {
    const entry = kept[i]!;
    if (entry.el.element === "row_start" && kept[i + 1]?.el.element === "row_end") {
      i += 1; // drop the closer along with the opener
      continue;
    }
    rows.push(entry);
  }

  return rows.filter(({ el }, i) => {
    if (el.element !== "divider") return true;
    const prev = rows[i - 1];
    const next = rows[i + 1];
    return prev !== undefined && next !== undefined && prev.el.element !== "divider";
  });
}
