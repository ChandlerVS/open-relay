import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
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
import {
  elementRule,
  elementTitle,
  isRowMarker,
  normalizeRows,
  type BuilderElement,
} from "./model";
import type { LayoutErrors } from "./validate";

export interface SelectMods {
  shift: boolean;
  meta: boolean;
}

export interface CanvasProps {
  items: BuilderElement[];
  selectedIds: ReadonlySet<string>;
  errors: LayoutErrors;
  onSelect: (id: string, mods: SelectMods) => void;
  onRemove: (id: string) => void;
  onReorder: (next: BuilderElement[]) => void;
  /**
   * Whether a drag is in flight. Lifted out because Space/Enter on the grip
   * starts a *keyboard* drag while the grip — which is inside the canvas — holds
   * focus, and the page's Delete/select-all shortcuts have to stand down for it.
   */
  onDragActiveChange?: (active: boolean) => void;
  /**
   * Viewing without `forms:write`. Selection stays live — the Inspector is
   * still worth reading — but reordering and removal are gone. A `fieldset`
   * can't express this the way it does for the Palette and Inspector: the
   * drag handle and the row body are buttons that must stay half-alive.
   */
  readOnly?: boolean;
}

const KIND_LABEL: Record<BuilderElement["element"]["element"], string> = {
  standard: "Standard",
  custom: "Custom",
  heading: "Heading",
  paragraph: "Text",
  rich_text: "Rich text",
  divider: "Divider",
  page_break: "Page break",
  row_start: "Row",
  row_end: "Row",
};

function ElementRow({
  item,
  selected,
  error,
  stepNumber,
  inRow,
  readOnly,
  onSelect,
  onRemove,
}: {
  item: BuilderElement;
  selected: boolean;
  error: string | undefined;
  stepNumber: number | null;
  /** Sits between a `row_start` and its `row_end`, so it is drawn indented. */
  inRow: boolean;
  readOnly: boolean;
  onSelect: (mods: SelectMods) => void;
  onRemove: () => void;
}) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: item.id,
    disabled: readOnly,
  });
  const el = item.element;
  const isBreak = el.element === "page_break";
  const marker = isRowMarker(el);
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
        (isBreak || marker) && "bg-muted/60 border-dashed",
        // Indentation is the only cue that a field is in a row — the list is
        // flat, so there is no container to draw around it.
        inRow && "ml-5 border-l-2 border-l-primary/40",
      )}
    >
      {readOnly ? (
        // A spacer, so read-only rows keep the same left edge as editable ones.
        <span className="w-4 shrink-0" aria-hidden="true" />
      ) : (
        <button
          type="button"
          className="cursor-grab text-muted-foreground hover:text-foreground touch-none"
          aria-label={`Reorder ${elementTitle(el)}`}
          // Selecting on the grip too, so the left edge of a row isn't a dead
          // zone. dnd-kit's own listeners run first and swallow this once an
          // actual drag starts.
          onClick={(e) => onSelect({ shift: e.shiftKey, meta: e.metaKey || e.ctrlKey })}
          {...attributes}
          {...listeners}
        >
          <GripVertical className="h-4 w-4" />
        </button>
      )}

      <button
        type="button"
        // Shift-click otherwise drags a native text selection across the list,
        // which would then also trip the copy handler's "a text selection is
        // live, leave it alone" guard and make Cmd+C silently do nothing.
        onMouseDown={(e) => {
          if (e.shiftKey) e.preventDefault();
        }}
        onClick={(e) => onSelect({ shift: e.shiftKey, meta: e.metaKey || e.ctrlKey })}
        // `aria-pressed` rather than `aria-selected`: the latter needs
        // `role="option"`, which can't hold the nested buttons this row has.
        aria-pressed={selected}
        className="flex-1 min-w-0 text-left"
      >
        <div className="flex items-center gap-1.5">
          <span className="truncate text-sm font-medium">
            {isBreak && stepNumber !== null
              ? `Page break — step ${stepNumber} starts here`
              : el.element === "row_start"
                ? `${elementTitle(el)} — fields below sit on one line`
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

      {!readOnly && (
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
      )}
    </div>
  );
}

/**
 * One flat sortable list. Pages are a visual grouping computed from the
 * `page_break` elements rather than a nested structure, which keeps
 * drag-and-drop across a page boundary free.
 *
 * Rows work the same way, and for the same payoff: a `row_start`/`row_end`
 * marker pair rather than a container with children, so putting a field in a
 * row is an ordinary vertical reorder. No second `SortableContext`, no
 * cross-container drag handling, and `restrictToVerticalAxis` stays on. The
 * price is that a drop can leave the markers in a nonsensical order, which
 * `normalizeRows` repairs afterwards rather than preventing.
 */
export function Canvas({
  items,
  selectedIds,
  errors,
  onSelect,
  onRemove,
  onReorder,
  onDragActiveChange,
  readOnly = false,
}: CanvasProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const onDragStart = (e: DragStartEvent) => {
    onDragActiveChange?.(true);
    // Dragging something outside the selection makes it the selection, so the
    // highlight never claims a block is moving when only one element is. Moving
    // a whole block is cut-and-paste; multi-drag would mean teaching dnd-kit
    // about groups, which is the nesting this flat list exists to avoid.
    const id = String(e.active.id);
    if (!selectedIds.has(id)) onSelect(id, { shift: false, meta: false });
  };

  const onDragEnd = (e: DragEndEvent) => {
    onDragActiveChange?.(false);
    const { active, over } = e;
    if (!over || active.id === over.id) return;
    const from = items.findIndex((i) => i.id === active.id);
    const to = items.findIndex((i) => i.id === over.id);
    if (from === -1 || to === -1) return;
    // Repair rather than restrict: a flat list lets a drag invert or strand a
    // row marker, and tidying up after is what keeps the drag itself simple.
    onReorder(normalizeRows(arrayMove(items, from, to)));
  };

  if (items.length === 0) {
    return (
      <div className="rounded border border-dashed border-border p-8 text-center text-sm text-muted-foreground">
        {readOnly
          ? "This form has no fields yet."
          : "No fields yet. Add one from the palette on the left."}
      </div>
    );
  }

  let step = 1;
  // Depth is derived on the fly from the markers, exactly like the step
  // counter — neither is stored on the element.
  let inRow = false;
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      modifiers={[restrictToVerticalAxis]}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      // Escape during a keyboard drag ends it without an `onDragEnd`, and a
      // guard that stuck on would disable every shortcut for the session.
      onDragCancel={() => onDragActiveChange?.(false)}
    >
      <SortableContext items={items.map((i) => i.id)} strategy={verticalListSortingStrategy}>
        {/* `select-none` so a shift-click range doesn't smear a text
            selection across the list as it grows. */}
        <div className="flex flex-col gap-1.5 select-none">
          {items.map((item) => {
            const kind = item.element.element;
            const stepNumber = kind === "page_break" ? ++step : null;
            if (kind === "row_end") inRow = false;
            const indented = inRow;
            if (kind === "row_start") inRow = true;
            return (
              <ElementRow
                key={item.id}
                item={item}
                selected={selectedIds.has(item.id)}
                error={errors[item.id]}
                stepNumber={stepNumber}
                inRow={indented}
                readOnly={readOnly}
                onSelect={(mods) => onSelect(item.id, mods)}
                onRemove={() => onRemove(item.id)}
              />
            );
          })}
        </div>
      </SortableContext>
    </DndContext>
  );
}
