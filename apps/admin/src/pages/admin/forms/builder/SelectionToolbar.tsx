import { useState } from "react";
import { Copy, ClipboardPaste, Scissors, CopyPlus, Trash2, X } from "lucide-react";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@open-relay/ui";
import { parseElements, readLocalClipboard } from "./clipboard";
import type { FormElement } from "./model";

export interface SelectionToolbarProps {
  count: number;
  canEdit: boolean;
  onCopy: () => void;
  onCut: () => void;
  onDuplicate: () => void;
  onDelete: () => void;
  onPaste: (elements: FormElement[]) => void;
  onClear: () => void;
}

const MOD = typeof navigator !== "undefined" && /Mac|iP(hone|ad)/.test(navigator.platform)
  ? "⌘"
  : "Ctrl+";

function Action({
  icon: Icon,
  label,
  hint,
  onClick,
  disabled,
}: {
  icon: typeof Copy;
  label: string;
  hint: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <Button
      type="button"
      variant="outline"
      size="sm"
      className="h-7 text-xs"
      disabled={disabled}
      title={hint}
      onClick={onClick}
    >
      <Icon className="h-3.5 w-3.5 mr-1" />
      {label}
    </Button>
  );
}

/**
 * Bulk actions for the canvas selection.
 *
 * Paste is a button rather than only a shortcut because the keyboard route
 * genuinely does not exist on some browsers: Firefox and Safari fire `paste`
 * only at an editable focus context, and a canvas card is a `<button>`. Its
 * click handler walks three rungs — the async Clipboard API, then the
 * `localStorage` mirror, then a textarea the user can paste into by hand —
 * because each of the first two is blocked somewhere. Firefox does not expose
 * `readText()` to page content at all, which is what the third rung is for.
 */
export function SelectionToolbar({
  count,
  canEdit,
  onCopy,
  onCut,
  onDuplicate,
  onDelete,
  onPaste,
  onClear,
}: SelectionToolbarProps) {
  const [manualOpen, setManualOpen] = useState(false);
  const [manualText, setManualText] = useState("");
  const [manualError, setManualError] = useState<string | null>(null);
  const has = count > 0;

  const tryPaste = async () => {
    try {
      const text = await navigator.clipboard.readText();
      const elements = parseElements(text);
      if (elements) {
        onPaste(elements);
        return;
      }
    } catch {
      // Not a secure context, no permission, or Firefox — fall through.
    }
    const mirrored = readLocalClipboard();
    if (mirrored) {
      onPaste(mirrored);
      return;
    }
    setManualText("");
    setManualError(null);
    setManualOpen(true);
  };

  const submitManual = () => {
    const elements = parseElements(manualText);
    if (!elements) {
      setManualError("That doesn't look like copied form elements.");
      return;
    }
    setManualOpen(false);
    onPaste(elements);
  };

  return (
    <div className="mb-2 flex flex-wrap items-center gap-1.5 border-b border-border pb-2">
      <span
        className="text-xs text-muted-foreground mr-1"
        // Announced so a screen-reader user hears the count change; the
        // highlight alone is a purely visual cue.
        aria-live="polite"
      >
        {has ? `${count} selected` : "Nothing selected"}
      </span>

      <Action
        icon={Copy}
        label="Copy"
        hint={`Copy the selection (${MOD}C) — paste it into any form`}
        onClick={onCopy}
        disabled={!has}
      />
      {canEdit && (
        <>
          <Action
            icon={Scissors}
            label="Cut"
            hint={`Cut the selection (${MOD}X)`}
            onClick={onCut}
            disabled={!has}
          />
          <Action
            icon={ClipboardPaste}
            label="Paste"
            hint={`Paste (${MOD}V) after the selection, or at the end`}
            onClick={() => void tryPaste()}
          />
          <Action
            icon={CopyPlus}
            label="Duplicate"
            hint={`Duplicate the selection in place (${MOD}D)`}
            onClick={onDuplicate}
            disabled={!has}
          />
          <Action
            icon={Trash2}
            label="Delete"
            hint="Delete the selection (Del)"
            onClick={onDelete}
            disabled={!has}
          />
        </>
      )}
      {has && (
        <Action icon={X} label="Clear" hint="Clear the selection (Esc)" onClick={onClear} />
      )}

      <p className="basis-full text-xs text-muted-foreground">
        Click to select; Shift-click for a range, {MOD}click to add or remove one.
      </p>

      <Dialog open={manualOpen} onOpenChange={setManualOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>Paste here</DialogTitle>
            <DialogDescription>
              This browser won't let a page read the clipboard directly. Click
              in the box and press {MOD}V.
            </DialogDescription>
          </DialogHeader>
          <textarea
            className="w-full min-h-32 rounded border border-border bg-background p-2 text-sm font-mono"
            autoFocus
            value={manualText}
            onChange={(e) => {
              setManualText(e.target.value);
              setManualError(null);
            }}
          />
          {manualError && <p className="text-xs text-destructive">{manualError}</p>}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setManualOpen(false)}>
              Cancel
            </Button>
            <Button type="button" disabled={!manualText.trim()} onClick={submitManual}>
              Paste
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
