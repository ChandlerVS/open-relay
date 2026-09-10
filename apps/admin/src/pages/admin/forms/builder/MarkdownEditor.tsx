import { useDeferredValue, useRef } from "react";
import { Markdown, richTextClass, type RichTextTone } from "@open-relay/form-renderer";
// The renderer's own stylesheet, so the preview is styled by the same rules
// the embed uses. Its `:host` rule matches nothing outside a shadow root,
// and every other selector is `.or-*`-prefixed, so this is inert elsewhere.
import "@open-relay/form-renderer/styles.css";
import { useTheme } from "../../../../lib/theme/useTheme";
import {
  Bold,
  Code,
  Heading2,
  Italic,
  Link as LinkIcon,
  List,
  ListOrdered,
  Minus,
} from "lucide-react";

/**
 * A markdown textarea with a formatting toolbar and a live preview.
 *
 * Deliberately not a contenteditable WYSIWYG: markdown is the stored source of
 * truth, and a rich editor would have to serialise back to it on every
 * keystroke — which both invites drift from what the renderer actually parses
 * and, because the builder's dirty check is a `JSON.stringify` comparison,
 * would make a form read as edited the moment it loaded.
 *
 * The preview is the *real* renderer (`Markdown` from `@open-relay/form-renderer`),
 * not a lookalike, so what the author sees is what a visitor gets.
 */

/** Any block marker, so one block command cleanly replaces another. */
const BLOCK_MARKER = /^\s*(?:#{1,6} +|[-*+] +|\d{1,9}[.)] +)/;

/**
 * Replace the textarea's selection with `text`, then select `[from, to]`.
 *
 * `document.execCommand("insertText")` rather than assigning `el.value`. It is
 * deprecated with no replacement, and it is still the only way to mutate an
 * input that pushes an entry onto the *browser's own* undo stack — a direct
 * assignment wipes it, so one Bold click would cost the author every Ctrl-Z
 * they had banked. It also dispatches a real `input` event, which is what
 * React's value tracker listens for, so `onChange` fires once with the edit.
 */
function replaceSelection(
  el: HTMLTextAreaElement,
  text: string,
  from: number,
  to: number,
) {
  el.focus();
  if (!document.execCommand("insertText", false, text)) {
    // Fallback so a browser that finally drops execCommand degrades to a
    // working button with a lost undo entry rather than a dead one. The native
    // setter is required: React's tracker swallows a plain assignment.
    const { selectionStart: s, selectionEnd: e, value } = el;
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )?.set;
    setter?.call(el, value.slice(0, s) + text + value.slice(e));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }
  el.setSelectionRange(from, to);
}

/** Wrap the selection in `marker`, or unwrap if it already is. */
function wrap(el: HTMLTextAreaElement, marker: string, placeholder: string) {
  const { selectionStart: s, selectionEnd: e, value } = el;
  const sel = value.slice(s, e);
  const n = marker.length;

  if (sel.length >= 2 * n && sel.startsWith(marker) && sel.endsWith(marker)) {
    const inner = sel.slice(n, -n);
    replaceSelection(el, inner, s, s + inner.length);
    return;
  }
  // Markers just outside the selection: swallow them and unwrap. This is what
  // makes double-clicking the *word* toggle bold back off.
  if (value.slice(s - n, s) === marker && value.slice(e, e + n) === marker) {
    el.setSelectionRange(s - n, e + n);
    replaceSelection(el, sel, s - n, s - n + sel.length);
    return;
  }
  const body = sel || placeholder;
  // Select the body, not the markers, so an empty-selection click leaves the
  // placeholder highlighted and the author types straight over it.
  replaceSelection(el, marker + body + marker, s + n, s + n + body.length);
}

/** Apply (or remove) a line prefix across every line the selection touches. */
function prefixLines(
  el: HTMLTextAreaElement,
  make: (i: number) => string,
  is: RegExp,
) {
  const { value } = el;
  // Expand to whole lines: a block marker applies to the line, not the chars.
  const from = value.lastIndexOf("\n", el.selectionStart - 1) + 1;
  const nl = value.indexOf("\n", el.selectionEnd);
  const to = nl === -1 ? value.length : nl;
  const lines = value.slice(from, to).split("\n");
  const body = lines.filter((l) => l.trim());
  const on = body.length > 0 && body.every((l) => is.test(l));
  const next = lines
    .map((l, i) => (on ? l.replace(BLOCK_MARKER, "") : make(i) + l.replace(BLOCK_MARKER, "")))
    .join("\n");
  el.setSelectionRange(from, to);
  replaceSelection(el, next, from, from + next.length);
}

function insertLink(el: HTMLTextAreaElement) {
  const { selectionStart: s, selectionEnd: e, value } = el;
  const sel = value.slice(s, e);
  // A selected URL becomes the destination; anything else becomes the text.
  const isUrl = /^https?:\/\/\S+$/i.test(sel);
  const text = isUrl ? "link text" : sel || "link text";
  const href = isUrl ? sel : "https://";
  // Leave selected whichever half still needs filling in.
  const at = isUrl ? s + 1 : s + text.length + 3;
  const len = isUrl ? text.length : href.length;
  replaceSelection(el, `[${text}](${href})`, at, at + len);
}

interface ToolProps {
  label: string;
  icon: React.ReactNode;
  onRun: (el: HTMLTextAreaElement) => void;
  target: React.RefObject<HTMLTextAreaElement | null>;
}

function Tool({ label, icon, onRun, target }: ToolProps) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      className="flex h-7 w-7 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground disabled:opacity-40"
      // Without this the textarea blurs before the click lands and
      // execCommand has no editing host to act on.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => {
        const el = target.current;
        if (el) onRun(el);
      }}
    >
      {icon}
    </button>
  );
}

export interface MarkdownEditorProps {
  value: string;
  tone: RichTextTone;
  onChange: (next: string) => void;
}

export function MarkdownEditor({ value, tone, onChange }: MarkdownEditorProps) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const { resolved: theme } = useTheme();
  // Parsing is cheap, but `onChange` already rewrites the whole `items` array
  // and re-validates the layout on every keystroke. Deferring the preview keeps
  // the typed character ahead of the render without desyncing the textarea,
  // which is what debouncing `value` would do.
  const preview = useDeferredValue(value);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (!(e.metaKey || e.ctrlKey)) return;
    const el = e.currentTarget;
    const key = e.key.toLowerCase();
    if (key === "b") {
      e.preventDefault();
      wrap(el, "**", "bold text");
    } else if (key === "i") {
      e.preventDefault();
      wrap(el, "_", "italic text");
    } else if (key === "k") {
      e.preventDefault();
      insertLink(el);
    }
  };

  const icon = "h-3.5 w-3.5";

  return (
    <div className="space-y-2">
      <div className="flex flex-wrap items-center gap-0.5 rounded border border-border bg-muted/40 p-1">
        <Tool label="Bold" target={ref} icon={<Bold className={icon} />} onRun={(el) => wrap(el, "**", "bold text")} />
        <Tool label="Italic" target={ref} icon={<Italic className={icon} />} onRun={(el) => wrap(el, "_", "italic text")} />
        <Tool label="Code" target={ref} icon={<Code className={icon} />} onRun={(el) => wrap(el, "`", "code")} />
        <Tool label="Link" target={ref} icon={<LinkIcon className={icon} />} onRun={insertLink} />
        <span className="mx-1 h-4 w-px bg-border" />
        <Tool
          label="Bulleted list"
          target={ref}
          icon={<List className={icon} />}
          onRun={(el) => prefixLines(el, () => "- ", /^\s*[-*+] +/)}
        />
        <Tool
          label="Numbered list"
          target={ref}
          icon={<ListOrdered className={icon} />}
          onRun={(el) => prefixLines(el, (i) => `${i + 1}. `, /^\s*\d{1,9}[.)] +/)}
        />
        <Tool
          label="Heading"
          target={ref}
          icon={<Heading2 className={icon} />}
          onRun={(el) => prefixLines(el, () => "## ", /^\s*#{1,6} +/)}
        />
        <Tool
          label="Divider"
          target={ref}
          icon={<Minus className={icon} />}
          onRun={(el) => {
            const at = el.selectionEnd;
            replaceSelection(el, "\n\n---\n\n", at + 7, at + 7);
          }}
        />
      </div>

      <textarea
        ref={ref}
        className="w-full min-h-48 rounded border border-border bg-background p-2 font-mono text-sm"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        // Converge on what `service::normalize_layout` will store, so a form
        // that saved cleanly stops reading as dirty on the next load.
        onBlur={(e) => {
          const next = e.target.value.replace(/\r\n?/g, "\n").trim();
          if (next !== value) onChange(next);
        }}
      />

      <div>
        <p className="mb-1 text-xs text-muted-foreground">Preview</p>
        {/*
          `.or-form` is what defines the theme tokens the `.or-richtext` rules
          read, so wrapping in it makes this fragment self-sufficient outside
          the shadow root. The stylesheet's `:host` rule matches nothing here,
          so importing it globally into the admin is inert.
        */}
        <div className="or-form" data-theme={theme}>
          <Markdown source={preview} className={richTextClass(tone)} />
        </div>
      </div>
    </div>
  );
}
