import { useMemo, type ReactNode } from "react";
import { parseMarkdown, type MdBlock, type MdInline } from "./markdown";
import type { RichTextTone } from "./schema";
import { isHttpUrl } from "./url";

/**
 * Renders a markdown source as React elements.
 *
 * No HTML string is ever constructed — see the spec in `markdown.ts` for why
 * that is the load-bearing property and what subset is supported.
 */

function renderInline(nodes: MdInline[], prefix: string | number): ReactNode[] {
  return nodes.map((n, i) => {
    const k = `${prefix}-${i}`;
    switch (n.kind) {
      // A bare string child: React escapes it. This one line is the entire
      // reason no sanitiser is needed in this package.
      case "text":
        return n.value;
      case "break":
        return <br key={k} />;
      case "code":
        return <code key={k}>{n.value}</code>;
      case "strong":
        return <strong key={k}>{renderInline(n.children, k)}</strong>;
      case "em":
        return <em key={k}>{renderInline(n.children, k)}</em>;
      case "link":
        return (
          <a
            key={k}
            // Re-checked at the point it becomes a DOM attribute, for exactly
            // the reason `Form.tsx` re-checks the redirect URL: React does not
            // sanitise `href`, a `javascript:` one is live, and this draws on a
            // page we don't own. The parser already refused it; this costs a
            // few bytes and removes the need to trust that it did.
            href={isHttpUrl(n.href) ? n.href : undefined}
            // The visitor is mid-form and nothing here persists a draft, so
            // navigating in place would destroy everything they have typed.
            // `nofollow ugc` because the destination is author-supplied content
            // on a page whose owner is often a different party.
            target="_blank"
            rel="noopener noreferrer nofollow ugc"
          >
            {renderInline(n.children, k)}
          </a>
        );
      default: {
        const unhandled: never = n;
        void unhandled;
        return null;
      }
    }
  });
}

function renderItems(items: MdInline[][], prefix: number): ReactNode[] {
  return items.map((kids, i) => <li key={i}>{renderInline(kids, `${prefix}-${i}`)}</li>);
}

// Index keys are correct here, unusually: the AST is a pure function of
// `source`, so an element cannot move without a full re-parse.
function renderBlock(b: MdBlock, i: number): ReactNode {
  switch (b.kind) {
    case "paragraph":
      return <p key={i}>{renderInline(b.children, i)}</p>;
    case "heading": {
      const Tag = (b.level === 2 ? "h2" : "h3") as "h2";
      return <Tag key={i}>{renderInline(b.children, i)}</Tag>;
    }
    case "bullet":
      return <ul key={i}>{renderItems(b.items, i)}</ul>;
    case "ordered":
      return (
        <ol key={i} start={b.start === 1 ? undefined : b.start}>
          {renderItems(b.items, i)}
        </ol>
      );
    case "thematic_break":
      return <hr key={i} />;
    default: {
      const unhandled: never = b;
      void unhandled;
      return null;
    }
  }
}

/**
 * `normal` gets no modifier — it is the default presentation, so leaving the
 * class off keeps the markup identical to what an untoned block produces. The
 * same call `fieldClass` makes for `full`.
 */
export function richTextClass(tone: RichTextTone | null | undefined): string {
  return tone && tone !== "normal" ? `or-richtext or-richtext--${tone}` : "or-richtext";
}

export function Markdown({
  source,
  className,
}: {
  source: string;
  className?: string;
}) {
  // The builder preview re-parses on every keystroke. Cheap, but not free, and
  // a stable AST also keeps React from rebuilding the subtree.
  const blocks = useMemo(() => parseMarkdown(source), [source]);
  if (blocks.length === 0) return null;
  return <div className={className}>{blocks.map(renderBlock)}</div>;
}
