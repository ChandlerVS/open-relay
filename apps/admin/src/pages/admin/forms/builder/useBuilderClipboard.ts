import { useEffect, useRef, type RefObject } from "react";
import { serializeElements, parseElements, writeLocalClipboard } from "./clipboard";
import type { FormElement } from "./model";

export interface ClipboardActions {
  /** Elements to put on the clipboard, or `null` when there's nothing selected. */
  copy: () => FormElement[] | null;
  /** Same, but also deletes them. Only called when editing is allowed. */
  cut: () => FormElement[] | null;
  paste: (elements: FormElement[]) => void;
  duplicate: () => void;
  remove: () => void;
  selectAll: () => void;
  clear: () => void;
  canEdit: boolean;
}

/**
 * The element the user actually acted on.
 *
 * `composedPath()[0]`, not `e.target`: the builder renders the live preview
 * inside a **shadow root** (`ShadowForm`), and an event from inside one is
 * retargeted at the host element by the time it reaches `document`. Reading
 * `e.target` would see a `<div>` where the user was typing in an `<input>`, so
 * a Cmd+A in the preview would sail past the editable-target guard below and
 * select every element on the canvas instead of the text in the field.
 */
function source(e: Event): Element | null {
  const path = typeof e.composedPath === "function" ? e.composedPath() : [];
  const first = path.length > 0 ? path[0] : e.target;
  return first instanceof Element ? first : null;
}

function isEditable(el: Element | null): boolean {
  if (!el) return false;
  const tag = el.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return el instanceof HTMLElement && el.isContentEditable;
}

/** Radix unmounts a closed dialog, so presence is as good as an open check. */
function dialogOpen(): boolean {
  return document.querySelector('[role="dialog"], [role="alertdialog"]') !== null;
}

function hasTextSelection(): boolean {
  return (window.getSelection()?.toString() ?? "").trim().length > 0;
}

/**
 * Keyboard and system-clipboard wiring for the canvas.
 *
 * Uses the `copy`/`cut`/`paste` DOM events rather than `navigator.clipboard`,
 * because `ClipboardEvent.clipboardData` needs no permission and no secure
 * context. The catch is that **Firefox and Safari only fire `paste` on an
 * editable focus context**, and a canvas card is a `<button>` — so on those
 * browsers the keyboard paste never arrives and the toolbar's Paste button is
 * the only route. That is why it is a button and not a hint.
 *
 * The listeners are registered once and read the latest actions through a ref:
 * `items` changes on every keystroke in the Inspector, and re-binding three
 * document listeners per character would be silly.
 */
export function useBuilderClipboard(
  containerRef: RefObject<HTMLElement | null>,
  actions: ClipboardActions,
  dragActive: boolean,
): void {
  const latest = useRef(actions);
  latest.current = actions;
  const dragging = useRef(dragActive);
  dragging.current = dragActive;

  useEffect(() => {
    const inCanvas = (e: Event): boolean => {
      const src = source(e);
      // Note this is correctly `false` for anything inside the preview's shadow
      // root: its nodes are not descendants of the canvas in the light tree.
      return src !== null && containerRef.current !== null && containerRef.current.contains(src);
    };

    /** Guards every handler shares: never steal a keystroke meant for a field. */
    const blocked = (e: Event): boolean =>
      dragging.current || isEditable(source(e)) || dialogOpen();

    const onCopy = (e: ClipboardEvent, cut: boolean) => {
      if (blocked(e)) return;
      // Copying a label out of an element card is a reasonable thing to want,
      // and it looks exactly like this with a selection also live.
      if (hasTextSelection()) return;
      if (cut && !latest.current.canEdit) return;
      const elements = cut ? latest.current.cut() : latest.current.copy();
      if (!elements || elements.length === 0) return;
      const payload = serializeElements(elements);
      e.clipboardData?.setData("text/plain", payload);
      writeLocalClipboard(payload);
      e.preventDefault();
    };

    const handleCopy = (e: ClipboardEvent) => onCopy(e, false);
    const handleCut = (e: ClipboardEvent) => onCopy(e, true);

    const handlePaste = (e: ClipboardEvent) => {
      if (blocked(e) || !latest.current.canEdit) return;
      const text = e.clipboardData?.getData("text/plain") ?? "";
      const elements = parseElements(text);
      // No fallback to the localStorage mirror here, deliberately: the user's
      // real clipboard holds something else, and pasting a stale block instead
      // of nothing would be worse than doing nothing. The button falls back.
      if (!elements) return;
      e.preventDefault();
      latest.current.paste(elements);
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (blocked(e)) return;
      const mod = e.metaKey || e.ctrlKey;

      // Escape drops the selection. Unscoped on purpose — it is the one key a
      // user reaches for to mean "never mind", from wherever they are. The
      // drag guard above already keeps it from stealing dnd-kit's own cancel.
      if (e.key === "Escape") {
        latest.current.clear();
        return;
      }
      if (mod && e.key.toLowerCase() === "d") {
        if (!latest.current.canEdit) return;
        e.preventDefault();
        latest.current.duplicate();
        return;
      }
      // Select-all and delete are scoped to the canvas: an unscoped Cmd+A would
      // fight the browser's own, and an unscoped Backspace deleting form fields
      // from wherever the user happened to be would be alarming.
      if (!inCanvas(e)) return;
      if (mod && e.key.toLowerCase() === "a") {
        e.preventDefault();
        latest.current.selectAll();
        return;
      }
      if (e.key === "Delete" || e.key === "Backspace") {
        if (!latest.current.canEdit) return;
        e.preventDefault();
        latest.current.remove();
      }
    };

    document.addEventListener("copy", handleCopy);
    document.addEventListener("cut", handleCut);
    document.addEventListener("paste", handlePaste);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("copy", handleCopy);
      document.removeEventListener("cut", handleCut);
      document.removeEventListener("paste", handlePaste);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [containerRef]);
}
