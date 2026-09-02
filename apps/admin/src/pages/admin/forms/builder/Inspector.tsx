import { Input, Label } from "@open-relay/ui";
import { STANDARD_FIELDS } from "@open-relay/form-renderer";
import {
  CUSTOM_FIELD_TYPES,
  retypeCustomField,
  type BuilderElement,
  type CustomTypeName,
  type FormElement,
} from "./model";

export interface InspectorProps {
  item: BuilderElement | null;
  onChange: (next: FormElement) => void;
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

function WidthRow({
  value,
  onChange,
}: {
  value: "full" | "half" | undefined;
  onChange: (v: "full" | "half") => void;
}) {
  return (
    <Row label="Width">
      <select
        className={SELECT_CLASS}
        value={value ?? "full"}
        onChange={(e) => onChange(e.target.value as "full" | "half")}
      >
        <option value="full">Full width</option>
        <option value="half">Half width</option>
      </select>
    </Row>
  );
}

/** Settings for the selected element. */
export function Inspector({ item, onChange }: InspectorProps) {
  if (!item) {
    return (
      <p className="text-sm text-muted-foreground">
        Select an element to edit its settings.
      </p>
    );
  }

  const el = item.element;

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
        <WidthRow value={cfg.width} onChange={(width) => patch({ width })} />
        <Check
          label="Required"
          checked={cfg.required ?? false}
          onChange={(required) => patch({ required })}
        />
        {cfg.key === "country" && (
          <div className="space-y-1">
            <Check
              label="Render as a dropdown"
              checked={cfg.input_override === "select"}
              onChange={(v) => patch({ input_override: v ? "select" : null })}
            />
            <p className="text-xs text-muted-foreground">
              Submits an ISO country code instead of free text, which delivers
              more reliably to external systems.
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
        {cfg.type === "select" && (
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
        {cfg.type !== "checkbox" && (
          <Row label="Default value">
            <Input
              className="h-8 text-sm"
              value={cfg.default_value ?? ""}
              onChange={(e) => patch({ default_value: e.target.value || null })}
            />
          </Row>
        )}
        <WidthRow value={cfg.width} onChange={(width) => patch({ width })} />
        <Check
          label="Required"
          checked={cfg.required ?? false}
          onChange={(required) => patch({ required })}
        />
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
      </div>
    );
  }

  if (el.element === "paragraph") {
    const cfg = el.config;
    return (
      <Row label="Text">
        <textarea
          className="w-full min-h-32 rounded border border-border bg-background p-2 text-sm"
          value={cfg.text}
          onChange={(e) =>
            onChange({ element: "paragraph", config: { text: e.target.value } })
          }
        />
      </Row>
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

  return <p className="text-sm text-muted-foreground">A divider has no settings.</p>;
}
