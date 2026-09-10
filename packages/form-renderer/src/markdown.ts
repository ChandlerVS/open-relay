// Extension-qualified so `node --test` can resolve this without a bundler;
// `allowImportingTsExtensions` is already on in `tsconfig.base.json` and
// Vite resolves it identically.
import { isHttpUrl } from "./url.ts";

/**
 * A deliberately small markdown subset, parsed to an AST of plain objects.
 *
 * This is **not** CommonMark and is not trying to be. It ships inside the embed
 * bundle, which lands in third-party host pages, so every feature costs bytes
 * on somebody else's page forever. A real markdown library is 12-40 KB before
 * a sanitiser; this is ~2 KB gzipped including the renderer and the CSS.
 *
 * Two invariants make the size affordable and the result safe:
 *
 * 1. **There is no HTML string anywhere.** `parseMarkdown` returns data;
 *    `Markdown.tsx` turns that data into React elements. Every leaf is a React
 *    text child (escaped by React) or an attribute from a fixed allowlist.
 *    Nothing in this package touches `innerHTML` or `dangerouslySetInnerHTML`,
 *    which is why no sanitiser is needed and why raw HTML in the source is
 *    simply text.
 * 2. **Unsupported syntax renders as literal text; nothing is ever dropped
 *    silently.** An author who pastes a table, an image, a blockquote or an
 *    HTML tag sees their own characters on screen. That is the only failure
 *    mode we are willing to have — a block that quietly renders half of what
 *    was typed is worse than one that renders it verbatim.
 *
 * ## Blocks (line-based, single pass, no nesting)
 *
 * Input is normalised `\r\n?` to `\n` and split on `\n`. Each line is tested in
 * this order; the first match wins:
 *
 * - **Blank** — closes the open paragraph. It closes an open list too, *unless*
 *   the next non-blank line starts an item of the same list kind, in which case
 *   the blank is swallowed and the list continues. Authors double-space between
 *   bullets constantly, and splitting that into two `<ul>`s looks broken.
 * - **Thematic break** — three or more `-`, `*` or `_`, optionally spaced.
 *   Checked *before* paragraph continuation, so `---` under a line of text is a
 *   rule, never a setext heading. Setext headings are not supported at all, and
 *   this ordering is what makes that unambiguous. A `|---|---|` table delimiter
 *   contains pipes and so is not a break — a pasted table survives as prose.
 * - **ATX heading** — one or two `#` gives `h2`, three or more gives `h3`.
 *   Clamped on purpose: an `h1` emitted into a host page hijacks that page's
 *   document outline, and anything below `h3` is invisible in a form.
 * - **Bullet item** — `-`, `*` or `+`. Interchangeable: switching marker
 *   mid-list does not start a new list, because one marker style is drawn
 *   regardless.
 * - **Ordered item** — `1.` or `1)`. The first item's number becomes
 *   `<ol start>`; later numbers are ignored, as in every markdown renderer.
 *   Switching between bullet and ordered *does* start a new list.
 * - **Lazy continuation** — any other non-blank line while a list is open
 *   appends to the current item. Indentation is not significant, which is
 *   exactly why nested lists are not supported: there is no second level to
 *   indent into.
 * - **Anything else** — a paragraph line.
 *
 * Nested lists, blockquotes, fenced and indented code blocks, tables, reference
 * links, footnotes and HTML blocks are **not** block constructs here. Their
 * source lines fall through to the paragraph branch and render as text.
 *
 * ## Inline (single left-to-right scan, recursive)
 *
 * Precedence, tightest first: escape, hard break, code span, image guard, link,
 * emphasis. Anything unmatched accumulates into a text node.
 *
 * - **Escape** — a backslash before ASCII punctuation emits that character
 *   literally. A backslash before anything else is a literal backslash.
 * - **Soft vs hard break** — a single newline joins with a space, per
 *   CommonMark. Two trailing spaces or a trailing backslash is a hard break.
 *   Note this differs from `MessageAction` copy, which uses `pre-line` so every
 *   newline survives; there the text is never parsed, here it is.
 * - **Code span** — a run of N backticks closes on the next run of exactly N.
 *   Content is verbatim: no emphasis, no links, no escapes inside. An unclosed
 *   run is literal backticks.
 * - **Image guard** — an unescaped `!` immediately before `[` consumes `![` as
 *   literal text and resumes *inside* the brackets, so `![alt](url)` renders
 *   character-for-character. Without this the `[alt](url)` tail would quietly
 *   become a link to the image, which is the "renders half of what was typed"
 *   failure the whole design is trying to avoid.
 * - **Link** — `[text](dest)` only. `text` is parsed recursively with nested
 *   links flattened; `dest` is raw, with balanced parens tracked so
 *   `https://en.wikipedia.org/wiki/Foo_(bar)` survives. Titles, reference links
 *   and autolinks are not supported and render literally. `dest` must satisfy
 *   `isHttpUrl`; if it does not, or the construct never closes, the entire
 *   `[text](dest)` source is emitted as literal text — so a bad URL is visible
 *   to the author in the builder preview rather than silently degrading in a
 *   visitor's browser.
 * - **Emphasis** — `***x***` gives strong+em, `**x**` strong, `*x*` em, tried
 *   in that order. An opener must be followed by a non-space and a closer
 *   preceded by one. `_` additionally refuses to open or close touching a
 *   letter or digit, so `snake_case_name` stays literal; `*` has no such guard,
 *   so `a*b*c` emphasises, matching CommonMark. Unmatched delimiters are
 *   literal.
 *
 *   The closer is found lazily, which handles `**bold with *em* inside**` but
 *   not the reverse: `*a **b** c*` finds its closer too early and renders with
 *   stray asterisks. That is an accepted limitation of a short emphasis pass —
 *   CommonMark's delimiter-run stack costs more than every other feature here
 *   combined — and it is pinned by a test so nobody changes it by accident.
 *
 * `<`, `>` and `|` carry no meaning. `<b>hi</b>` and `<script>x</script>` are
 * text nodes and React escapes them. Entity references are not decoded.
 *
 * ## Not supported, on purpose
 *
 * Tables, images, raw HTML, blockquotes, nested lists, task lists,
 * strikethrough, code blocks, setext headings, autolinks, reference links,
 * footnotes, entity references, heading IDs.
 */

export type MdInline =
  | { kind: "text"; value: string }
  | { kind: "break" }
  | { kind: "code"; value: string }
  | { kind: "strong"; children: MdInline[] }
  | { kind: "em"; children: MdInline[] }
  /** `href` has already passed `isHttpUrl`; re-checked at render anyway. */
  | { kind: "link"; href: string; children: MdInline[] };

export type MdBlock =
  | { kind: "paragraph"; children: MdInline[] }
  | { kind: "heading"; level: 2 | 3; children: MdInline[] }
  | { kind: "bullet"; items: MdInline[][] }
  | { kind: "ordered"; start: number; items: MdInline[][] }
  | { kind: "thematic_break" };

const RE_BREAK = /^ {0,3}(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$/;
const RE_HEADING = /^ {0,3}(#{1,6}) +(.*)$/;
const RE_BULLET = /^ {0,3}[-*+] +(.*)$/;
const RE_ORDERED = /^ {0,3}(\d{1,9})[.)] +(.*)$/;
const PUNCT = /[!-\/:-@\[-`{-~]/;

/** Word characters that `_` refuses to sit against, so snake_case survives. */
function isWordChar(c: string): boolean {
  return c !== "" && /[\p{L}\p{N}]/u.test(c);
}

/** Parse a markdown source into blocks. Pure; safe to memoise on `source`. */
export function parseMarkdown(source: string): MdBlock[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const out: MdBlock[] = [];

  // Open state. At most one of `para` / `list` is non-null at a time.
  let para: string[] | null = null;
  let list: { kind: "bullet" | "ordered"; start: number; items: string[] } | null = null;

  const flushPara = () => {
    if (para && para.join("\n").trim() !== "") {
      out.push({ kind: "paragraph", children: parseInline(para.join("\n")) });
    }
    para = null;
  };
  const flushList = () => {
    if (list) {
      const items = list.items.map((t) => parseInline(t));
      out.push(
        list.kind === "bullet"
          ? { kind: "bullet", items }
          : { kind: "ordered", start: list.start, items },
      );
    }
    list = null;
  };
  const flush = () => {
    flushPara();
    flushList();
  };

  for (let i = 0; i < lines.length; i += 1) {
    const ln = lines[i] ?? "";

    if (ln.trim() === "") {
      flushPara();
      // A blank line inside a list is only a separator if what follows isn't
      // another item of the same kind. Authors double-space bullets constantly.
      if (list) {
        let j = i + 1;
        while (j < lines.length && (lines[j] ?? "").trim() === "") j += 1;
        const next = lines[j] ?? "";
        const sameKind =
          list.kind === "bullet" ? RE_BULLET.test(next) : RE_ORDERED.test(next);
        if (!sameKind) flushList();
      }
      continue;
    }

    if (RE_BREAK.test(ln)) {
      flush();
      out.push({ kind: "thematic_break" });
      continue;
    }

    const heading = RE_HEADING.exec(ln);
    if (heading) {
      flush();
      const hashes = (heading[1] ?? "#").length;
      out.push({
        kind: "heading",
        level: hashes <= 2 ? 2 : 3,
        children: parseInline(heading[2] ?? ""),
      });
      continue;
    }

    const bullet = RE_BULLET.exec(ln);
    if (bullet) {
      flushPara();
      if (list && list.kind !== "bullet") flushList();
      if (!list) list = { kind: "bullet", start: 1, items: [] };
      list.items.push(bullet[1] ?? "");
      continue;
    }

    const ordered = RE_ORDERED.exec(ln);
    if (ordered) {
      flushPara();
      if (list && list.kind !== "ordered") flushList();
      if (!list) {
        list = { kind: "ordered", start: Number(ordered[1] ?? "1") || 1, items: [] };
      }
      list.items.push(ordered[2] ?? "");
      continue;
    }

    // Lazy continuation of the current list item.
    if (list && list.items.length > 0) {
      list.items[list.items.length - 1] += "\n" + ln.trim();
      continue;
    }

    if (!para) para = [];
    para.push(ln);
  }

  flush();
  return out;
}

/** Parse inline markup. Exported for tests; `parseMarkdown` is the entry point. */
export function parseInline(src: string): MdInline[] {
  const out: MdInline[] = [];
  let buf = "";
  const push = (n: MdInline) => {
    if (buf !== "") {
      out.push({ kind: "text", value: buf });
      buf = "";
    }
    out.push(n);
  };

  let i = 0;
  while (i < src.length) {
    const c = src.charAt(i);

    // --- escape ---------------------------------------------------------
    if (c === "\\") {
      const next = src.charAt(i + 1);
      if (next === "\n") {
        push({ kind: "break" });
        i += 2;
        continue;
      }
      if (next !== "" && PUNCT.test(next)) {
        buf += next;
        i += 2;
        continue;
      }
      buf += "\\";
      i += 1;
      continue;
    }

    // --- breaks ---------------------------------------------------------
    if (c === "\n") {
      // Two or more trailing spaces before the newline is a hard break.
      if (/ {2,}$/.test(buf)) {
        buf = buf.replace(/ +$/, "");
        push({ kind: "break" });
      } else {
        buf = buf.replace(/ +$/, "") + " ";
      }
      i += 1;
      while (src.charAt(i) === " ") i += 1;
      continue;
    }

    // --- code span ------------------------------------------------------
    if (c === "`") {
      let n = 0;
      while (src.charAt(i + n) === "`") n += 1;
      const fence = "`".repeat(n);
      // Find a run of exactly n backticks.
      let j = i + n;
      let close = -1;
      while (j < src.length) {
        if (src.charAt(j) === "`") {
          let m = 0;
          while (src.charAt(j + m) === "`") m += 1;
          if (m === n) {
            close = j;
            break;
          }
          j += m;
          continue;
        }
        j += 1;
      }
      if (close === -1) {
        buf += fence;
        i += n;
        continue;
      }
      push({ kind: "code", value: src.slice(i + n, close).replace(/\n/g, " ").trim() });
      i = close + n;
      continue;
    }

    // --- image guard ----------------------------------------------------
    // `![alt](url)` must render literally, not degrade into a link.
    if (c === "!" && src.charAt(i + 1) === "[") {
      buf += "![";
      i += 2;
      continue;
    }

    // --- link -----------------------------------------------------------
    if (c === "[") {
      const link = matchLink(src, i);
      if (link) {
        push({
          kind: "link",
          href: link.href,
          children: flattenLinks(parseInline(link.text)),
        });
        i = link.end;
        continue;
      }
      buf += "[";
      i += 1;
      continue;
    }

    // --- emphasis -------------------------------------------------------
    if (c === "*" || c === "_") {
      const em = matchEmphasis(src, i, c);
      if (em) {
        push(em.node);
        i = em.end;
        continue;
      }
      buf += c;
      i += 1;
      continue;
    }

    buf += c;
    i += 1;
  }

  if (buf !== "") out.push({ kind: "text", value: buf });
  return out;
}

/** `[text](dest)` starting at `start`, or null if it doesn't close cleanly. */
function matchLink(
  src: string,
  start: number,
): { text: string; href: string; end: number } | null {
  // Bracket matching, so `[a [b] c](url)` works.
  let depth = 0;
  let i = start;
  let textEnd = -1;
  for (; i < src.length; i += 1) {
    const c = src.charAt(i);
    if (c === "\\") {
      i += 1;
      continue;
    }
    if (c === "[") depth += 1;
    else if (c === "]") {
      depth -= 1;
      if (depth === 0) {
        textEnd = i;
        break;
      }
    }
  }
  if (textEnd === -1 || src.charAt(textEnd + 1) !== "(") return null;

  // Destination, tracking balanced parens so `.../Foo_(bar)` survives.
  let paren = 1;
  let j = textEnd + 2;
  for (; j < src.length; j += 1) {
    const c = src.charAt(j);
    if (c === "\\") {
      j += 1;
      continue;
    }
    if (c === "(") paren += 1;
    else if (c === ")") {
      paren -= 1;
      if (paren === 0) break;
    }
  }
  if (paren !== 0) return null;

  const href = src.slice(textEnd + 2, j);
  // An unsafe or unsupported destination falls back to literal text, so the
  // author sees exactly what they typed instead of a silently dead link.
  if (!isHttpUrl(href)) return null;
  return { text: src.slice(start + 1, textEnd), href, end: j + 1 };
}

/** Links cannot nest; an inner one contributes only its text. */
function flattenLinks(nodes: MdInline[]): MdInline[] {
  return nodes.flatMap((n) => {
    if (n.kind === "link") return flattenLinks(n.children);
    if (n.kind === "strong" || n.kind === "em") {
      return [{ ...n, children: flattenLinks(n.children) }];
    }
    return [n];
  });
}

/** `***x***`, `**x**` or `*x*` (or the `_` forms) starting at `start`. */
function matchEmphasis(
  src: string,
  start: number,
  marker: string,
): { node: MdInline; end: number } | null {
  let run = 0;
  while (src.charAt(start + run) === marker) run += 1;
  // `_` must not open against a word character, so snake_case stays literal.
  if (marker === "_" && isWordChar(src.charAt(start - 1))) return null;

  for (const n of run >= 3 ? [3, 2, 1] : run === 2 ? [2, 1] : [1]) {
    const open = start + n;
    if (open >= src.length || /\s/.test(src.charAt(open))) continue;
    const close = findCloser(src, open, marker, n);
    // `close === open` is an empty span. Matching it would consume the
    // delimiters and emit nothing, which is the one failure mode this parser
    // refuses: characters the author typed would vanish. Fall through to the
    // shorter run, and ultimately to literal text.
    if (close === -1 || close === open) continue;
    const inner = parseInline(src.slice(open, close));
    const node: MdInline =
      n === 3
        ? { kind: "strong", children: [{ kind: "em", children: inner }] }
        : n === 2
          ? { kind: "strong", children: inner }
          : { kind: "em", children: inner };
    return { node, end: close + n };
  }
  return null;
}

/** Index of a closing run of exactly `n` markers, or -1. */
function findCloser(src: string, from: number, marker: string, n: number): number {
  let i = from;
  while (i < src.length) {
    const c = src.charAt(i);
    if (c === "\\") {
      i += 2;
      continue;
    }
    if (c === "`") {
      // Don't close inside a code span.
      let m = 0;
      while (src.charAt(i + m) === "`") m += 1;
      const next = src.indexOf("`".repeat(m), i + m);
      i = next === -1 ? i + m : next + m;
      continue;
    }
    if (c === marker) {
      let run = 0;
      while (src.charAt(i + run) === marker) run += 1;
      const prev = src.charAt(i - 1);
      const closes =
        run >= n &&
        prev !== "" &&
        !/\s/.test(prev) &&
        (marker !== "_" || !isWordChar(src.charAt(i + run)));
      if (closes) return i;
      i += run;
      continue;
    }
    i += 1;
  }
  return -1;
}
