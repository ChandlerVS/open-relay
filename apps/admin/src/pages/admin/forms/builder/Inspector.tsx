import { Input, Label } from "@open-relay/ui";
import { STANDARD_FIELDS } from "@open-relay/form-renderer";
import {
  CUSTOM_FIELD_TYPES,
  canBeConditional,
  elementRule,
  fieldHasOptions,
  retypeCustomField,
  withRule,
  type BuilderElement,
  type ControllerCandidate,
  type CustomTypeName,
  type FieldWidth,
  type FormElement,
  type VisibilityRule,
} from "./model";
import { VisibilityRuleEditor } from "./VisibilityRuleEditor";

export interface InspectorProps {
  item: BuilderElement | null;
  onChange: (next: FormElement) => void;
  /**
   * Fields earlier in the layout, which are the only legal controllers for a
   * visibility rule — see `controllerCandidates`.
   */
  candidates: ControllerCandidate[];
  /**
   * Country fields earlier in the layout — the only legal parents for a state
   * picker, see `countryFieldCandidates`.
   */
  countryCandidates: ControllerCandidate[];
  /** Elements after this one, for the rule editor's bulk apply. */
  ruleTargets: BuilderElement[];
  /** Whether the selected element sits inside a row, which changes what width means. */
  inRow: boolean;
  onApplyRuleToMany: (ids: string[], rule: VisibilityRule) => void;
}

const SELECT_CLASS =
  "h-8 w-full rounded border border-border bg-background px-2 text-sm";

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1">
      <Label className="text-xs text-muted-foreground">{label}</Label>
      {children}
    </div>
  );
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        className="h-4 w-4 rounded border-border accent-primary"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
      {label}
    </label>
  );
}

const WIDTHS: { value: FieldWidth; label: string }[] = [
  { value: "full", label: "Full width" },
  { value: "two_thirds", label: "Two thirds" },
  { value: "half", label: "Half width" },
  { value: "third", label: "One third" },
];

/**
 * Width is read two ways, which is why the hint changes with context: outside a
 * row it is a share of the form's six-column grid, and fields whose widths add
 * up land on one line by themselves; inside a row it is a share of that row.
 */
function WidthRow({
  value,
  inRow,
  onChange,
}: {
  value: FieldWidth | undefined;
  inRow: boolean;
  onChange: (v: FieldWidth) => void;
}) {
  return (
    <>
      <Row label="Width">
        <select
          className={SELECT_CLASS}
          value={value ?? "full"}
          onChange={(e) => onChange(e.target.value as FieldWidth)}
        >
          {WIDTHS.map((w) => (
            <option key={w.value} value={w.value}>
              {w.label}
            </option>
          ))}
        </select>
      </Row>
      {inRow && (value ?? "full") === "full" && (
        <p className="text-xs text-muted-foreground">
          Fields in a row share the line evenly unless you narrow them.
        </p>
      )}
    </>
  );
}

/** Settings for the selected element. */
export function Inspector({
  item,
  onChange,
  inRow,
  candidates,
  countryCandidates,
  ruleTargets,
  onApplyRuleToMany,
}: InspectorProps) {
  if (!item) {
    return (
      <p className="text-sm text-muted-foreground">
        Select an element to edit its settings.
      </p>
    );
  }

  const el = item.element;

  // Rendered from every branch whose element kind can carry a rule, so the
  // control sits in the same place whatever is selected.
  const visibility = canBeConditional(el) ? (
    <VisibilityRuleEditor
      rule={elementRule(el)}
      candidates={candidates}
      targets={ruleTargets}
      onChange={(rule) => onChange(withRule(el, rule))}
      onApplyToMany={onApplyRuleToMany}
    />
  ) : null;

  if (el.element === "standard") {
    const cfg = el.config;
    const def = STANDARD_FIELDS.find((d) => d.key === cfg.key);
    const patch = (p: Partial<typeof cfg>) =>
      onChange({ element: "standard", config: { ...cfg, ...p } });
    return (
      <div className="space-y-3">
        <p className="text-sm font-medium">{def?.default_label ?? cfg.key}</p>
        <p className="text-xs text-muted-foreground">
          Standard field — delivered as <code>{cfg.key}</code>.
        </p>
        <Row label="Label override">
          <Input
            className="h-8 text-sm"
            value={cfg.label ?? ""}
            placeholder={def?.default_label}
            onChange={(e) => patch({ label: e.target.value || null })}
          />
        </Row>
        <Row label="Placeholder">
          <Input
            className="h-8 text-sm"
            value={cfg.placeholder ?? ""}
            onChange={(e) => patch({ placeholder: e.target.value || null })}
          />
        </Row>
        <Row label="Help text">
          <Input
            className="h-8 text-sm"
            value={cfg.help_text ?? ""}
            onChange={(e) => patch({ help_text: e.target.value || null })}
          />
        </Row>
        <Row label="Default value">
          <Input
            className="h-8 text-sm"
            value={cfg.default_value ?? ""}
            onChange={(e) => patch({ default_value: e.target.value || null })}
          />
        </Row>
        <WidthRow value={cfg.width} inRow={inRow} onChange={(width) => patch({ width })} />
        <Check
          label="Required"
          checked={cfg.required ?? false}
          onChange={(required) => patch({ required })}
        />
        {visibility}
        {(cfg.key === "country" || cfg.key === "state") && (
          <div className="space-y-1">
            <Check
              label="Render as a dropdown"
              checked={cfg.input_override === "select"}
              onChange={(v) => patch({ input_override: v ? "select" : null })}
            />
            <p className="text-xs text-muted-foreground">
              {cfg.key === "country"
                ? "Submits an ISO country code instead of free text, which delivers more reliably to external systems."
                : "Lists the states or provinces of the chosen country. Needs the standard Country field earlier in the form, also set to a dropdown — a free-text country holds a name, not a code."}
            </p>
          </div>
        )}
      </div>
    );
  }

  if (el.element === "custom") {
    const cfg = el.config;
    const patch = (p: Partial<typeof cfg>) =>
      onChange({ element: "custom", config: { ...cfg, ...p } as typeof cfg });
    return (
      <div className="space-y-3">
        <Row label="Label">
          <Input
            className="h-8 text-sm"
            value={cfg.label}
            onChange={(e) => patch({ label: e.target.value })}
          />
        </Row>
        <Row label="Key">
          <Input
            className="h-8 text-sm font-mono"
            value={cfg.key}
            placeholder="how_did_you_hear"
            onChange={(e) => patch({ key: e.target.value })}
          />
        </Row>
        <p className="text-xs text-muted-foreground">
          The key names this field in submissions and in backend mappings.
          Renaming it on a live form orphans values already collected under the
          old name.
        </p>
        <Row label="Type">
          <select
            className={SELECT_CLASS}
            value={cfg.type}
            onChange={(e) =>
              onChange({
                element: "custom",
                config: retypeCustomField(cfg, e.target.value as CustomTypeName),
              })
            }
          >
            {CUSTOM_FIELD_TYPES.map((t) => (
              <option key={t.type} value={t.type}>
                {t.label}
              </option>
            ))}
          </select>
        </Row>
        {fieldHasOptions(cfg) && (
          <Row label="Options (one per line)">
            <textarea
              className="w-full min-h-24 rounded border border-border bg-background p-2 text-sm"
              value={(cfg.options ?? []).join("\n")}
              onChange={(e) =>
                patch({
                  options: e.target.value.split("\n").map((o) => o.replace(/^\s+/, "")),
                } as Partial<typeof cfg>)
              }
              onBlur={(e) =>
                patch({
                  options: e.target.value
                    .split("\n")
                    .map((o) => o.trim())
                    .filter(Boolean),
                } as Partial<typeof cfg>)
              }
            />
          </Row>
        )}
        {cfg.type === "state" && (
          <div className="space-y-1">
            <Row label="Country field">
              <select
                className={SELECT_CLASS}
                value={cfg.country_field ?? ""}
                onChange={(e) =>
                  patch({ country_field: e.target.value || undefined } as Partial<typeof cfg>)
                }
              >
                <option value="">Not linked — free text</option>
                {/*
                  A reference that is no longer earlier in the form still has to
                  be shown, or the picker would silently read as "not linked"
                  while the saved value still points at it.
                */}
                {cfg.country_field &&
                  !countryCandidates.some((c) => c.key === cfg.country_field) && (
                    <option value={cfg.country_field}>
                      {cfg.country_field} — no longer earlier in the form
                    </option>
                  )}
                {countryCandidates.map((c) => (
                  <option key={c.key} value={c.key}>
                    {c.label}
                  </option>
                ))}
              </select>
            </Row>
            <p className="text-xs text-muted-foreground">
              Lists the states or provinces of whichever country is chosen
              there, and submits the ISO code. The country has to come earlier
              in the form. Unlinked, this is a plain text box.
            </p>
          </div>
        )}
        {cfg.type !== "country" && cfg.type !== "state" && (
          <Row label="Placeholder">
            <Input
              className="h-8 text-sm"
              value={cfg.placeholder ?? ""}
              onChange={(e) => patch({ placeholder: e.target.value || null })}
            />
          </Row>
        )}
        <Row label="Help text">
          <Input
            className="h-8 text-sm"
            value={cfg.help_text ?? ""}
            onChange={(e) => patch({ help_text: e.target.value || null })}
          />
        </Row>
        {/*
          A default country code is genuinely useful; a default subdivision is
          not, since the list it belongs to depends on an answer nobody has
          given yet.
        */}
        {cfg.type !== "checkbox" && cfg.type !== "state" && (
          <Row label="Default value">
            <Input
              className="h-8 text-sm"
              value={cfg.default_value ?? ""}
              onChange={(e) => patch({ default_value: e.target.value || null })}
            />
          </Row>
        )}
        <WidthRow value={cfg.width} inRow={inRow} onChange={(width) => patch({ width })} />
        <Check
          label="Required"
          checked={cfg.required ?? false}
          onChange={(required) => patch({ required })}
        />
        {visibility}
      </div>
    );
  }

  if (el.element === "heading") {
    const cfg = el.config;
    return (
      <div className="space-y-3">
        <Row label="Text">
          <Input
            className="h-8 text-sm"
            value={cfg.text}
            onChange={(e) =>
              onChange({ element: "heading", config: { ...cfg, text: e.target.value } })
            }
          />
        </Row>
        <Row label="Level">
          <select
            className={SELECT_CLASS}
            value={cfg.level}
            onChange={(e) =>
              onChange({
                element: "heading",
                config: { ...cfg, level: Number(e.target.value) },
              })
            }
          >
            {[1, 2, 3, 4, 5, 6].map((l) => (
              <option key={l} value={l}>
                H{l}
              </option>
            ))}
          </select>
        </Row>
        {visibility}
      </div>
    );
  }

  if (el.element === "paragraph") {
    const cfg = el.config;
    return (
      <div className="space-y-3">
        <Row label="Text">
          <textarea
            className="w-full min-h-32 rounded border border-border bg-background p-2 text-sm"
            value={cfg.text}
            onChange={(e) =>
              // Spread the existing config: rebuilding it as `{ text }` alone
              // would silently drop the visibility rule below.
              onChange({ element: "paragraph", config: { ...cfg, text: e.target.value } })
            }
          />
        </Row>
        {visibility}
      </div>
    );
  }

  if (el.element === "page_break") {
    const cfg = el.config;
    return (
      <div className="space-y-2">
        <Row label="Title for the step that follows">
          <Input
            className="h-8 text-sm"
            value={cfg.title ?? ""}
            placeholder="e.g. Your details"
            onChange={(e) =>
              onChange({ element: "page_break", config: { title: e.target.value || null } })
            }
          />
        </Row>
        <p className="text-xs text-muted-foreground">
          Everything below this break becomes the next step. Visitors must fill
          in the required fields on a step before moving on.
        </p>
      </div>
    );
  }

  if (el.element === "row_start") {
    const cfg = el.config;
    return (
      <div className="space-y-2">
        <Row label="Group label">
          <Input
            className="h-8 text-sm"
            value={cfg.label ?? ""}
            placeholder="e.g. City, state, ZIP"
            onChange={(e) =>
              onChange({ element: "row_start", config: { label: e.target.value || null } })
            }
          />
        </Row>
        <p className="text-xs text-muted-foreground">
          Fields between this and the end of the row sit on one line, sharing it
          evenly unless you set their widths. The label is read out by screen
          readers and is not shown on the form — leave it blank if the fields'
          own labels already say what the group is.
        </p>
        <p className="text-xs text-muted-foreground">
          On narrow screens the fields stack, and a row whose fields are all
          hidden by a rule disappears with them.
        </p>
      </div>
    );
  }

  if (el.element === "row_end") {
    return (
      <p className="text-sm text-muted-foreground">
        Marks where the row above ends. Drag fields above this to add them to
        the row.
      </p>
    );
  }

  return <p className="text-sm text-muted-foreground">A divider has no settings.</p>;
}
