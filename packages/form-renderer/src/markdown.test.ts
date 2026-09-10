import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown, type MdBlock, type MdInline } from "./markdown.ts";
import { isHttpUrl } from "./url.ts";

/**
 * The executable form of the spec in `markdown.ts`.
 *
 * Written as a fixture table on purpose: the doc comment there makes a dozen
 * behavioural promises a typechecker cannot see, and this file is where they
 * are pinned. `flat()` renders an AST back to a compact string so a fixture
 * reads as "source in, shape out" rather than as nested object literals.
 */

function flatInline(nodes: MdInline[]): string {
  return nodes
    .map((n) => {
      switch (n.kind) {
        case "text":
          return n.value;
        case "break":
          return "<br>";
        case "code":
          return "`" + n.value + "`";
        case "strong":
          return `strong(${flatInline(n.children)})`;
        case "em":
          return `em(${flatInline(n.children)})`;
        case "link":
          return `a[${n.href}](${flatInline(n.children)})`;
      }
    })
    .join("");
}

function flat(blocks: MdBlock[]): string {
  return blocks
    .map((b) => {
      switch (b.kind) {
        case "paragraph":
          return `P ${flatInline(b.children)}`;
        case "heading":
          return `H${b.level} ${flatInline(b.children)}`;
        case "bullet":
          return `UL ${b.items.map((i) => `[${flatInline(i)}]`).join("")}`;
        case "ordered":
          return `OL@${b.start} ${b.items.map((i) => `[${flatInline(i)}]`).join("")}`;
        case "thematic_break":
          return "HR";
      }
    })
    .join(" / ");
}

const CASES: [string, string][] = [
  // --- inline basics --------------------------------------------------
  ["**bold** and *it*", "P strong(bold) and em(it)"],
  ["a `co de` b", "P a `co de` b"],
  ["***both***", "P strong(em(both))"],
  ["**a *b* c**", "P strong(a em(b) c)"],
  ["__strong__ and _em_", "P strong(strong) and em(em)"],
  // `_` refuses to sit against a word character, so identifiers survive.
  ["snake_case_name stays", "P snake_case_name stays"],
  // `*` has no such guard, matching CommonMark.
  ["a*b*c", "P aem(b)c"],
  ["unclosed *em here", "P unclosed *em here"],
  ["esc \\*not em\\* done", "P esc *not em* done"],

  // --- links ----------------------------------------------------------
  ["see [terms](https://e.com/t) now", "P see a[https://e.com/t](terms) now"],
  ["[**bold** link](https://e.com)", "P a[https://e.com](strong(bold) link)"],
  ["[a [b] c](https://e.com)", "P a[https://e.com](a [b] c)"],
  ["[x](https://en.w.org/wiki/Foo_(bar))", "P a[https://en.w.org/wiki/Foo_(bar)](x)"],
  // Unsafe or unsupported destinations fall back to literal text.
  ["[click](javascript:alert(1))", "P [click](javascript:alert(1))"],
  ["[click](/relative)", "P [click](/relative)"],
  ['[click](https://e.com "Title")', 'P [click](https://e.com "Title")'],
  ["[unclosed](https://e.com", "P [unclosed](https://e.com"],

  // --- things that must render literally -------------------------------
  ["![alt](https://x/i.png)", "P ![alt](https://x/i.png)"],
  ["<b>hi</b> & <script>x</script>", "P <b>hi</b> & <script>x</script>"],
  ["| a | b |\n|---|---|\n| 1 | 2 |", "P | a | b | |---|---| | 1 | 2 |"],
  ["> quoted", "P > quoted"],

  // --- blocks -----------------------------------------------------------
  ["## Head\n### Sub\n#### Deep", "H2 Head / H3 Sub / H3 Deep"],
  ["# One", "H2 One"],
  ["text\n---\nmore", "P text / HR / P more"],
  ["***", "HR"],
  ["- one\n- two", "UL [one][two]"],
  // A blank line between items of the same kind keeps one list.
  ["- one\n\n- two", "UL [one][two]"],
  ["- one\n\ntext", "UL [one] / P text"],
  ["3. start\n4. next", "OL@3 [start][next]"],
  ["1. a\n- b", "OL@1 [a] / UL [b]"],
  ["- item one\n  continued", "UL [item one continued]"],

  // --- breaks -----------------------------------------------------------
  ["line1  \nline2", "P line1<br>line2"],
  ["line1\nline2", "P line1 line2"],
  ["line1\\\nline2", "P line1<br>line2"],

  // --- known limitation, pinned deliberately ---------------------------
  // A short emphasis pass finds its closer lazily. CommonMark's delimiter-run
  // stack costs more than every other feature here combined. If someone
  // improves emphasis, this fixture is how they find out they changed it.
  ["*a **b** c*", "P em(a **b)* c*"],
];

test("markdown fixtures", () => {
  for (const [src, want] of CASES) {
    assert.equal(flat(parseMarkdown(src)), want, JSON.stringify(src));
  }
});

test("empty and whitespace sources produce no blocks", () => {
  assert.deepEqual(parseMarkdown(""), []);
  assert.deepEqual(parseMarkdown("   \n\n  "), []);
});

test("CRLF is folded", () => {
  assert.equal(flat(parseMarkdown("a\r\n\r\nb")), "P a / P b");
});

/**
 * The one test that must never go red. Walks every AST this suite can produce,
 * plus a hostile list, and asserts no link node survived with a destination
 * that `isHttpUrl` rejects.
 */
test("no link node ever carries an unsafe href", () => {
  const hostile = [
    "javascript:alert(1)",
    "JaVaScRiPt:alert(1)",
    "java\nscript:alert(1)",
    "java\tscript:alert(1)",
    "data:text/html,<script>x</script>",
    "vbscript:msgbox(1)",
    "//evil.example/x",
    "/relative",
    "#frag",
    " https://e.com",
    "https://e.com ",
    "https:///path",
    "https:/x",
    "",
  ];
  const sources = [
    ...CASES.map(([src]) => src),
    ...hostile.map((h) => `[click](${h})`),
    ...hostile.map((h) => `text before [a **b**](${h}) text after`),
  ];

  const walk = (nodes: MdInline[]) => {
    for (const n of nodes) {
      if (n.kind === "link") {
        assert.ok(isHttpUrl(n.href), `unsafe href survived parsing: ${n.href}`);
        walk(n.children);
      } else if (n.kind === "strong" || n.kind === "em") {
        walk(n.children);
      }
    }
  };

  for (const src of sources) {
    for (const b of parseMarkdown(src)) {
      if (b.kind === "bullet" || b.kind === "ordered") b.items.forEach(walk);
      else if (b.kind !== "thematic_break") walk(b.children);
    }
  }
});

/**
 * "Nothing vanishes silently", machine-checked. Every alphanumeric run in the
 * source has to survive into some text or code node. This is the property most
 * likely to regress when a construct is added.
 */
test("no visible text is ever dropped", () => {
  const collect = (nodes: MdInline[], out: string[]) => {
    for (const n of nodes) {
      if (n.kind === "text" || n.kind === "code") out.push(n.value);
      else if (n.kind !== "break") collect(n.children, out);
    }
  };

  for (const [src] of CASES) {
    const out: string[] = [];
    for (const b of parseMarkdown(src)) {
      if (b.kind === "bullet" || b.kind === "ordered") {
        b.items.forEach((i) => collect(i, out));
      } else if (b.kind !== "thematic_break") {
        collect(b.children, out);
      }
    }
    const seen = out.join(" ");
    // Markup doesn't count as visible text: a link destination is not drawn
    // (`[terms](https://x)` renders "terms"), and a list marker becomes the
    // list itself (`3.` becomes `<ol start="3">`).
    const visible = src
      .replace(/\]\([^)]*\)/g, "]")
      .split("\n")
      .map((l) => l.replace(/^ {0,3}(?:#{1,6} +|[-*+] +|\d{1,9}[.)] +)/, ""))
      .join("\n");
    for (const word of visible.match(/[A-Za-z0-9]+/g) ?? []) {
      assert.ok(seen.includes(word), `"${word}" vanished from ${JSON.stringify(src)}`);
    }
  }
});
