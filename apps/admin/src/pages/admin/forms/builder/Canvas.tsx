import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { restrictToVerticalAxis } from "@dnd-kit/modifiers";
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GitBranch, GripVertical, Trash2 } from "lucide-react";
import { Button, cn } from "@open-relay/ui";
import { elementRule, elementTitle, type BuilderElement } from "./model";
import type { LayoutErrors } from "./validate";

export interface CanvasProps {
  items: BuilderElement[];
  selectedId: string | null;
  errors: LayoutErrors;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
  onReorder: (next: BuilderElement[]) => void;
}

const KIND_LABEL: Record<BuilderElement["element"]["element"], string> = {
  standard: "Standard",
  custom: "Custom",
  heading: "Heading",
  paragraph: "Text",
  divider: "Divider",
  page_break: "Page break",
};

function Row({
  item,
  selected,
  error,
  stepNumber,
  onSelect,
  onRemove,
}: {
  item: BuilderElement;
  selected: boolean;
  error: string | undefined;
  stepNumber: number | null;
  onSelect: () => void;
  onRemove: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
  });
  const el = item.element;
  const isBreak = el.element === "page_break";
  const required =
    (el.element === "standard" || el.element === "custom") && el.config.required;

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Transform.toString(transform), transition }}
      className={cn(
        "group flex items-center gap-2 rounded border px-2 py-1.5 bg-card",
        selected ? "border-primary ring-1 ring-primary" : "border-border",
        error && "border-destructive",
        isDragging && "opacity-50",
        isBreak && "bg-muted/60 border-dashed",
      )}
    >
      <button
        type="button"
        className="cursor-grab text-muted-foreground hover:text-foreground touch-none"
        aria-label={`Reorder ${elementTitle(el)}`}
        {...attributes}
        {...listeners}
      >
        <GripVertical className="h-4 w-4" />
      </button>

      <button
        type="button"
        onClick={onSelect}
        className="flex-1 min-w-0 text-left"
      >
        <div className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">
            {isBreak && stepNumber !== null
              ? `Page break — step ${stepNumber} starts here`
              : elementTitle(el)}
          </span>
          {required && <span className="text-destructive text-sm">*</span>}
        </div>
        <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span>{KIND_LABEL[el.element]}</span>
          {(el.element === "standard" || el.element === "custom") && el.config.key && (
            <code className="truncate">{el.config.key}</code>
          )}
          {/* The rule itself lives in the Inspector; this is just so a
              conditional element is findable in a long flat list. */}
          {elementRule(el) && (
            <span
              className="inline-flex items-center gap-0.5 shrink-0"
              title="Only shown when a condition is met"
            >
              <GitBranch className="h-3 w-3" />
              if
            </span>
          )}
        </div>
        {error && <p className="text-xs text-destructive mt-0.5">{error}</p>}
      </button>

      <Button
        type="button"
        variant="ghost"
        size="sm"
        className="h-7 w-7 p-0 shrink-0 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
        aria-label={`Remove ${elementTitle(el)}`}
        onClick={onRemove}
      >
        <Trash2 className="h-4 w-4" />
      </Button>
    </div>
  );
}

/**
 * One flat sortable list. Pages are a visual grouping computed from the
 * `page_break` elements rather than a nested structure, which keeps
 * drag-and-drop across a page boundary free.
 */
export function Canvas({
  items,
  selectedId,
  errors,
  onSelect,
  onRemove,
  onReorder,
}: CanvasProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const onDragEnd = (e: DragEndEvent) => {
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const from = items.findIndex((i) => i.id === active.id);
    const to = items.findIndex((i) => i.id === over.id);
    if (from === -1 || to === -1) return;
    onReorder(arrayMove(items, from, to));
  };

  if (items.length === 0) {
    return (
      <div className="rounded border border-dashed border-border p-8 text-center text-sm text-muted-foreground">
        No fields yet. Add one from the palette on the left.
      </div>
    );
  }

  let step = 1;
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      modifiers={[restrictToVerticalAxis]}
      onDragEnd={onDragEnd}
    >
      <SortableContext items={items.map((i) => i.id)} strategy={verticalListSortingStrategy}>
        <div className="flex flex-col gap-1.5">
          {items.map((item) => {
            const stepNumber =
              item.element.element === "page_break" ? ++step : null;
            return (
              <Row
                key={item.id}
                item={item}
                selected={item.id === selectedId}
                error={errors[item.id]}
                stepNumber={stepNumber}
                onSelect={() => onSelect(item.id)}
                onRemove={() => onRemove(item.id)}
              />
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );
}
