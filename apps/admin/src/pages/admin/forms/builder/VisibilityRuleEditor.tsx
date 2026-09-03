import { Button, Input } from "@open-relay/ui";
import { Plus, Trash2 } from "lucide-react";
import {
  canBeConditional,
  elementTitle,
  type BuilderElement,
  type Condition,
  type ConditionOp,
  type ControllerCandidate,
  type VisibilityRule,
} from "./model";

const SELECT_CLASS =
  "h-8 w-full rounded border border-border bg-background px-2 text-sm";

/**
 * The operators, in the order they read best in a menu. `is_checked` /
 * `is_not_checked` are offered only for a checkbox controller — the server
 * rejects them anywhere else — and the three at the top are the ones that take
 * an operand.
 */
const OPS: { op: ConditionOp; label: string; takesValue: boolean; checkboxOnly?: boolean }[] = [
  { op: "equals", label: "is", takesValue: true },
  { op: "not_equals", label: "is not", takesValue: true },
  { op: "contains", label: "contains", takesValue: true },
  { op: "is_not_empty", label: "is answered", takesValue: false },
  { op: "is_empty", label: "is blank", takesValue: false },
  { op: "is_checked", label: "is ticked", takesValue: false, checkboxOnly: true },
  { op: "is_not_checked", label: "is not ticked", takesValue: false, checkboxOnly: true },
];

export function opTakesValue(op: ConditionOp): boolean {
  return OPS.find((o) => o.op === op)?.takesValue ?? false;
}

function opsFor(candidate: ControllerCandidate | undefined) {
  const isCheckbox = candidate?.type === "checkbox";
  return OPS.filter((o) => (o.checkboxOnly ? isCheckbox : true));
}

/**
 * The operand half of a condition, for a given controller and operator.
 *
 * A choice controller (dropdown / radio) renders its operand as a `<select>` of
 * that field's options, and a `<select>` whose `value` matches none of them
 * *displays the first option anyway* — so carrying a value across a controller
 * change would show "Yes" while the rule actually held `""` (or a stale option
 * from the previous field), and the only hint would be a validation error
 * naming a value the editor appears to have. So the operand is always re-based
 * on the new controller's option set: kept when it is still one of them,
 * otherwise the first option.
 */
function valueFor(
  candidate: ControllerCandidate | undefined,
  op: ConditionOp,
  prev: string | null | undefined,
): { value?: string } {
  if (!opTakesValue(op)) return {};
  const options = candidate?.options;
  if (options?.length) {
    return { value: prev && options.includes(prev) ? prev : options[0]! };
  }
  return { value: prev ?? "" };
}

/** The rule a freshly enabled toggle starts from: the first available field. */
function defaultRule(candidates: ControllerCandidate[]): VisibilityRule {
  const first = candidates[0];
  const isCheckbox = first?.type === "checkbox";
  const op: ConditionOp = isCheckbox ? "is_checked" : "equals";
  return {
    conditions: [{ field: first?.key ?? "", op, ...valueFor(first, op, "") }],
  };
}

/**
 * Per-element "show this only when…" editor.
 *
 * Only fields *earlier* in the layout can be controllers — that is the server's
 * rule and the reason a form resolves in one forward pass — so the candidate
 * list is computed from the element's position, and an element at the top of
 * the form simply has nothing to depend on.
 *
 * Shaped like `PostSubmissionEditor` in `FormFormDialog` (a `value`/`onChange`
 * sub-component that rebuilds the whole value) but styled with the Inspector's
 * own raw `<select>`/`<input>` idiom, since `packages/ui` has no Select.
 */
export function VisibilityRuleEditor({
  rule,
  candidates,
  targets,
  onChange,
  onApplyToMany,
}: {
  rule: VisibilityRule | null;
  candidates: ControllerCandidate[];
  /** Elements after this one that the same rule could be copied onto. */
  targets: BuilderElement[];
  onChange: (next: VisibilityRule | null) => void;
  onApplyToMany: (ids: string[], rule: VisibilityRule) => void;
}) {
  if (candidates.length === 0) {
    return (
      <div className="space-y-1 border-t border-border pt-3">
        <p className="text-xs font-medium">Visibility</p>
        <p className="text-xs text-muted-foreground">
          Always shown. To make this conditional, put the field it depends on
          earlier in the form.
        </p>
      </div>
    );
  }

  const patch = (conditions: Condition[], match?: VisibilityRule["match"]) =>
    onChange({ match: match ?? rule?.match, conditions });

  const setCondition = (i: number, next: Condition) =>
    patch((rule?.conditions ?? []).map((c, j) => (j === i ? next : c)));

  return (
    <div className="space-y-2 border-t border-border pt-3">
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          className="h-4 w-4 rounded border-border accent-primary"
          checked={rule !== null}
          onChange={(e) => onChange(e.target.checked ? defaultRule(candidates) : null)}
        />
        Only show this when…
      </label>

      {rule && (
        <div className="space-y-2 rounded border border-border p-2">
          {rule.conditions.length > 1 && (
            <select
              className={SELECT_CLASS}
              value={rule.match ?? "all"}
              onChange={(e) => patch(rule.conditions, e.target.value as "all" | "any")}
            >
              <option value="all">All of these are true</option>
              <option value="any">Any of these is true</option>
            </select>
          )}

          {rule.conditions.map((cond, i) => {
            const candidate = candidates.find((c) => c.key === cond.field);
            const ops = opsFor(candidate);
            const takesValue = opTakesValue(cond.op);
            return (
              <div key={i} className="space-y-1">
                <div className="flex items-start gap-1">
                  <div className="flex-1 space-y-1 min-w-0">
                    <select
                      className={SELECT_CLASS}
                      value={cond.field}
                      onChange={(e) => {
                        const next = candidates.find((c) => c.key === e.target.value);
                        const stillValid = opsFor(next).some((o) => o.op === cond.op);
                        const op: ConditionOp = stillValid
                          ? cond.op
                          : next?.type === "checkbox"
                            ? "is_checked"
                            : "equals";
                        setCondition(i, {
                          field: e.target.value,
                          op,
                          ...valueFor(next, op, cond.value),
                        });
                      }}
                    >
                      {/* Same reason as the operand's placeholder below: a
                          controller can be dragged after its dependent, and a
                          select that silently shows a field the rule does not
                          name would make the "has to appear earlier" error
                          unreadable. */}
                      {!candidates.some((c) => c.key === cond.field) && (
                        <option value={cond.field}>
                          {cond.field
                            ? `${cond.field} — no longer earlier in the form`
                            : "Choose a field…"}
                        </option>
                      )}
                      {candidates.map((c) => (
                        <option key={c.key} value={c.key}>
                          {c.label}
                        </option>
                      ))}
                    </select>
                    <select
                      className={SELECT_CLASS}
                      value={cond.op}
                      onChange={(e) => {
                        const op = e.target.value as ConditionOp;
                        setCondition(i, {
                          field: cond.field,
                          op,
                          ...valueFor(candidate, op, cond.value),
                        });
                      }}
                    >
                      {ops.map((o) => (
                        <option key={o.op} value={o.op}>
                          {o.label}
                        </option>
                      ))}
                    </select>
                    {takesValue &&
                      // A dropdown or radio group has a known answer set, so
                      // offer it rather than inviting a typo that silently
                      // never matches.
                      (candidate?.options?.length ? (
                        <select
                          className={SELECT_CLASS}
                          value={cond.value ?? ""}
                          onChange={(e) =>
                            setCondition(i, { ...cond, value: e.target.value })
                          }
                        >
                          {/* The options can move out from under a rule that
                              was already saved — an option renamed or deleted.
                              Without a slot to hold it the browser would show
                              the first option while the rule still held the
                              old one, so keep the odd value visible and say
                              so. */}
                          {!candidate.options.includes(cond.value ?? "") && (
                            <option value={cond.value ?? ""}>
                              {cond.value
                                ? `${cond.value} — no longer an option`
                                : "Choose a value…"}
                            </option>
                          )}
                          {candidate.options.map((o) => (
                            <option key={o} value={o}>
                              {/* A country's stored value is its ISO code; show
                                  the name so the rule reads as English. */}
                              {candidate.optionLabels?.[o] ?? o}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <Input
                          className="h-8 text-sm"
                          value={cond.value ?? ""}
                          placeholder="value"
                          onChange={(e) =>
                            setCondition(i, { ...cond, value: e.target.value })
                          }
                        />
                      ))}
                  </div>
                  {rule.conditions.length > 1 && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 w-8 p-0 shrink-0"
                      aria-label="Remove condition"
                      onClick={() => patch(rule.conditions.filter((_, j) => j !== i))}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              </div>
            );
          })}

          <Button
            type="button"
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={() => patch([...rule.conditions, defaultRule(candidates).conditions[0]!])}
          >
            <Plus className="h-3 w-3 mr-1" />
            Add condition
          </Button>

          <ApplyToMany rule={rule} targets={targets} onApply={onApplyToMany} />
        </div>
      )}
    </div>
  );
}

/**
 * Copy this rule onto other elements in one go.
 *
 * Rules are per-element by design — the layout stays a flat list — but a real
 * branch hides a whole block, so setting seven fields one at a time is the
 * common case and worth collapsing into one action.
 */
function ApplyToMany({
  rule,
  targets,
  onApply,
}: {
  rule: VisibilityRule;
  targets: BuilderElement[];
  onApply: (ids: string[], rule: VisibilityRule) => void;
}) {
  const eligible = targets.filter((t) => canBeConditional(t.element));
  if (eligible.length === 0) return null;

  return (
    <details className="text-xs">
      <summary className="cursor-pointer text-muted-foreground">
        Apply this rule to other fields…
      </summary>
      <div className="mt-2 space-y-1">
        {eligible.map((t) => (
          <label key={t.id} className="flex items-center gap-2">
            <input
              type="checkbox"
              className="h-3.5 w-3.5 rounded border-border accent-primary"
              data-apply-id={t.id}
            />
            <span className="truncate">{elementTitle(t.element)}</span>
          </label>
        ))}
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="h-7 text-xs mt-1"
          onClick={(e) => {
            const root = e.currentTarget.parentElement;
            const ids = Array.from(
              root?.querySelectorAll<HTMLInputElement>("input[data-apply-id]") ?? [],
            )
              .filter((i) => i.checked)
              .map((i) => i.dataset["applyId"]!);
            if (ids.length) onApply(ids, rule);
          }}
        >
          Apply to selected
        </Button>
      </div>
    </details>
  );
}
