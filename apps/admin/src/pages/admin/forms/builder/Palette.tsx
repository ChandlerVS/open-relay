import { Button } from "@open-relay/ui";
import { STANDARD_FIELDS } from "@open-relay/form-renderer";
import { CUSTOM_FIELD_TYPES, type CustomTypeName } from "./model";

export interface PaletteProps {
  usedStandard: Set<string>;
  onAddStandard: (key: string) => void;
  onAddCustom: (type: CustomTypeName) => void;
  onAddDecoration: (kind: "heading" | "paragraph" | "divider" | "page_break") => void;
}

const DECORATIONS = [
  { kind: "heading", label: "Heading" },
  { kind: "paragraph", label: "Paragraph" },
  { kind: "divider", label: "Divider" },
  { kind: "page_break", label: "Page break" },
] as const;

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="space-y-1.5">
      <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
        {title}
      </h3>
      <div className="flex flex-col gap-1">{children}</div>
    </div>
  );
}

function PaletteButton({
  onClick,
  disabled,
  children,
  hint,
}: {
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
  hint?: string;
}) {
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="justify-start h-8 text-sm font-normal"
      disabled={disabled}
      title={hint}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

/** Click-to-append source of new elements. */
export function Palette({
  usedStandard,
  onAddStandard,
  onAddCustom,
  onAddDecoration,
}: PaletteProps) {
  return (
    <div className="space-y-4">
      <Group title="Standard fields">
        {STANDARD_FIELDS.map((def) => {
          const used = usedStandard.has(def.key);
          return (
            <PaletteButton
              key={def.key}
              disabled={used}
              hint={used ? "Already on this form" : `Add ${def.default_label}`}
              onClick={() => onAddStandard(def.key)}
            >
              {def.default_label}
            </PaletteButton>
          );
        })}
      </Group>

      <Group title="Custom fields">
        {CUSTOM_FIELD_TYPES.map((t) => (
          <PaletteButton key={t.type} onClick={() => onAddCustom(t.type)}>
            {t.label}
          </PaletteButton>
        ))}
      </Group>

      <Group title="Layout">
        {DECORATIONS.map((d) => (
          <PaletteButton key={d.kind} onClick={() => onAddDecoration(d.kind)}>
            {d.label}
          </PaletteButton>
        ))}
      </Group>
    </div>
  );
}
