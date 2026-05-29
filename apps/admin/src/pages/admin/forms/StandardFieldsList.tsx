import type { components } from "@open-relay/api-client";
import { Input } from "@open-relay/ui";
import { STANDARD_FIELDS, type StandardFieldKey } from "../../../lib/forms/standardFields";

type StandardFieldsConfig = components["schemas"]["StandardFieldsConfig"];
type StandardFieldConfig = components["schemas"]["StandardFieldConfig"];

export interface StandardFieldsListProps {
  value: StandardFieldsConfig;
  onChange: (next: StandardFieldsConfig) => void;
}

/**
 * Per-field toggles for the fixed standard-field set. Each row gets an
 * enabled checkbox, a required checkbox (disabled when not enabled), and a
 * label override.
 */
export function StandardFieldsList({ value, onChange }: StandardFieldsListProps) {
  const updateField = (
    key: StandardFieldKey,
    patch: Partial<StandardFieldConfig>,
  ) => {
    const current = value[key as keyof StandardFieldsConfig];
    onChange({
      ...value,
      [key]: { ...current, ...patch },
    });
  };

  return (
    <div className="rounded border border-border overflow-hidden">
      <div className="grid grid-cols-[1fr_auto_auto_2fr] gap-3 px-3 py-2 bg-muted/50 text-xs font-medium text-muted-foreground uppercase tracking-wider">
        <div>Field</div>
        <div className="text-center">Enabled</div>
        <div className="text-center">Required</div>
        <div>Label override</div>
      </div>
      <div className="divide-y divide-border">
        {STANDARD_FIELDS.map(({ key, default_label }) => {
          const cfg = value[key as keyof StandardFieldsConfig];
          return (
            <div
              key={key}
              className="grid grid-cols-[1fr_auto_auto_2fr] gap-3 px-3 py-2 items-center"
            >
              <div className="text-sm font-medium">{default_label}</div>
              <div className="flex justify-center">
                <input
                  type="checkbox"
                  className="h-4 w-4 rounded border-border accent-primary"
                  checked={cfg.enabled}
                  onChange={(e) =>
                    updateField(key as StandardFieldKey, {
                      enabled: e.target.checked,
                      // Disabling a field also clears `required` so the state
                      // stays internally consistent.
                      required: e.target.checked ? cfg.required : false,
                    })
                  }
                />
              </div>
              <div className="flex justify-center">
                <input
                  type="checkbox"
                  className="h-4 w-4 rounded border-border accent-primary disabled:opacity-40"
                  checked={cfg.required}
                  disabled={!cfg.enabled}
                  onChange={(e) =>
                    updateField(key as StandardFieldKey, {
                      required: e.target.checked,
                    })
                  }
                />
              </div>
              <div>
                <Input
                  value={cfg.label ?? ""}
                  placeholder={default_label}
                  onChange={(e) =>
                    updateField(key as StandardFieldKey, {
                      label: e.target.value || null,
                    })
                  }
                  className="h-8 text-sm"
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
