import type { components } from "@open-relay/api-client";
import { ChevronDown, ChevronUp, Plus, Trash2 } from "lucide-react";
import { Button, FormField, Input } from "@open-relay/ui";

type CustomField = components["schemas"]["CustomField"];
type CustomFieldType = CustomField["type"];

const TYPE_OPTIONS: { value: CustomFieldType; label: string }[] = [
  { value: "text", label: "Text" },
  { value: "email", label: "Email" },
  { value: "number", label: "Number" },
  { value: "tel", label: "Phone" },
  { value: "url", label: "URL" },
  { value: "textarea", label: "Long text" },
  { value: "select", label: "Select" },
  { value: "checkbox", label: "Checkbox" },
];

export interface CustomFieldsEditorProps {
  value: CustomField[];
  onChange: (next: CustomField[]) => void;
  errors?: Record<number, string | undefined>;
}

function blankField(position: number): CustomField {
  return {
    key: "",
    label: "",
    type: "text",
    required: false,
    placeholder: null,
    help_text: null,
    position,
  };
}

export function CustomFieldsEditor({ value, onChange, errors }: CustomFieldsEditorProps) {
  const update = (index: number, patch: Partial<CustomField>) => {
    const next = value.map((f, i) => (i === index ? { ...f, ...patch } : f));
    onChange(next);
  };

  const changeType = (index: number, nextType: CustomFieldType) => {
    const current = value[index];
    if (!current) return;
    // `options` is only meaningful for `select`. Drop it from any other
    // variant so the discriminated union stays clean over the wire.
    const base = {
      key: current.key,
      label: current.label,
      required: current.required,
      placeholder: current.placeholder,
      help_text: current.help_text,
      position: current.position,
    };
    if (nextType === "select") {
      onChange(
        value.map((f, i) =>
          i === index
            ? ({ ...base, type: "select", options: [] } as CustomField)
            : f,
        ),
      );
    } else {
      onChange(
        value.map((f, i) =>
          i === index ? ({ ...base, type: nextType } as CustomField) : f,
        ),
      );
    }
  };

  const move = (index: number, dir: -1 | 1) => {
    const target = index + dir;
    if (target < 0 || target >= value.length) return;
    const next = value.slice();
    const [a, b] = [next[index]!, next[target]!];
    next[index] = { ...b, position: index };
    next[target] = { ...a, position: target };
    onChange(next);
  };

  const remove = (index: number) => {
    onChange(value.filter((_, i) => i !== index).map((f, i) => ({ ...f, position: i })));
  };

  const add = () => {
    onChange([...value, blankField(value.length)]);
  };

  return (
    <div className="space-y-3">
      {value.length === 0 && (
        <p className="text-xs text-muted-foreground py-4 text-center border border-dashed border-border rounded">
          No custom fields. Add one to collect anything not in the standard set.
        </p>
      )}
      {value.map((field, index) => {
        const id = `cf-${index}`;
        const isSelect = field.type === "select";
        const options = isSelect ? (field.options ?? []) : null;
        return (
          <div
            key={index}
            className="rounded border border-border p-3 space-y-3 bg-background"
          >
            <div className="flex items-center justify-between gap-2">
              <div className="text-xs font-medium text-muted-foreground">
                Field {index + 1}
              </div>
              <div className="flex items-center gap-1">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={index === 0}
                  onClick={() => move(index, -1)}
                  aria-label="Move up"
                >
                  <ChevronUp className="h-4 w-4" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={index === value.length - 1}
                  onClick={() => move(index, 1)}
                  aria-label="Move down"
                >
                  <ChevronDown className="h-4 w-4" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => remove(index)}
                  aria-label="Remove field"
                  className="text-destructive hover:text-destructive"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            </div>

            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <FormField
                id={`${id}-label`}
                label="Label"
                error={errors?.[index]}
              >
                <Input
                  value={field.label}
                  placeholder="Shoe size"
                  onChange={(e) => update(index, { label: e.target.value })}
                />
              </FormField>
              <FormField
                id={`${id}-key`}
                label="Key"
                hint="Unique within the form. Match your backend's field key/id."
              >
                <Input
                  value={field.key}
                  placeholder="shoe_size"
                  onChange={(e) => update(index, { key: e.target.value })}
                />
              </FormField>
              <FormField id={`${id}-type`} label="Type">
                <select
                  className="flex h-9 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  value={field.type}
                  onChange={(e) => changeType(index, e.target.value as CustomFieldType)}
                >
                  {TYPE_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </FormField>
              <FormField id={`${id}-required`} label="Required">
                <label className="flex h-9 items-center gap-2 text-sm">
                  <input
                    type="checkbox"
                    className="h-4 w-4 rounded border-border accent-primary"
                    checked={field.required}
                    onChange={(e) =>
                      update(index, { required: e.target.checked })
                    }
                  />
                  <span className="text-muted-foreground">
                    Submitters must fill this in
                  </span>
                </label>
              </FormField>
            </div>

            {isSelect && (
              <FormField
                id={`${id}-options`}
                label="Options"
                hint="One option per line."
              >
                <textarea
                  className="flex w-full rounded-md border border-input bg-background px-3 py-2 text-sm shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring min-h-[80px]"
                  value={(options ?? []).join("\n")}
                  onChange={(e) => {
                    const opts = e.target.value
                      .split("\n")
                      .map((s) => s.trimStart());
                    onChange(
                      value.map((f, i) =>
                        i === index
                          ? ({ ...f, type: "select", options: opts } as CustomField)
                          : f,
                      ),
                    );
                  }}
                />
              </FormField>
            )}

            <FormField id={`${id}-placeholder`} label="Placeholder (optional)">
              <Input
                value={field.placeholder ?? ""}
                onChange={(e) =>
                  update(index, { placeholder: e.target.value || null })
                }
              />
            </FormField>
            <FormField id={`${id}-help`} label="Help text (optional)">
              <Input
                value={field.help_text ?? ""}
                onChange={(e) =>
                  update(index, { help_text: e.target.value || null })
                }
              />
            </FormField>
          </div>
        );
      })}
      <Button type="button" variant="outline" size="sm" onClick={add}>
        <Plus className="h-4 w-4" />
        Add custom field
      </Button>
    </div>
  );
}
