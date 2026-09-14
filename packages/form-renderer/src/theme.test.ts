import { test } from "node:test";
import assert from "node:assert/strict";

import { isHexColor, isSafeFontFamily, themeStyle } from "./theme.ts";
import type { ThemeSettings } from "./schema.ts";

/**
 * `themeStyle` writes API-supplied values into CSS on a host page we don't
 * own. These pin the containment half of the contract: whatever the server
 * would accept renders, and anything that could make a request, reference
 * another variable or escape its declaration is dropped.
 */

test("hex colours are the only colours", () => {
  for (const ok of ["#fff", "#FFFF", "#4f46e5", "#4f46e580"]) {
    assert.ok(isHexColor(ok), ok);
  }
  for (const bad of [
    "fff",
    "#ffff0",
    "red",
    "url(https://x.test/a.png)",
    "var(--x)",
    "#fff;color:blue",
    "red;color:blue",
    "",
    42,
  ]) {
    assert.ok(!isHexColor(bad), String(bad));
  }
});

test("font families are a fixed character set with balanced quotes", () => {
  for (const ok of ["Inter", '"Helvetica Neue", Arial, sans-serif', "'IBM Plex Sans'"]) {
    assert.ok(isSafeFontFamily(ok), ok);
  }
  for (const bad of ["Inter; color: red", "url(x)", "var(--x)", '"Unbalanced', "a\\62 c", ""]) {
    assert.ok(!isSafeFontFamily(bad), bad);
  }
});

test("a valid theme maps to theme-tier tokens only", () => {
  const style = themeStyle({
    colors: { accent: "#4f46e5", input_background: "#ffffff" },
    radius: 12,
    font_family: "Inter, sans-serif",
    font_size: "large",
    density: "compact",
  }) as Record<string, string>;
  assert.deepEqual(style, {
    "--or-theme-accent": "#4f46e5",
    "--or-theme-input-background": "#ffffff",
    "--or-theme-radius": "12px",
    "--or-theme-font": "Inter, sans-serif",
    "--or-theme-font-scale": "1.125",
    "--or-theme-space-scale": "0.75",
  });
  for (const key of Object.keys(style)) {
    assert.ok(key.startsWith("--or-theme-"), key);
  }
});

test("hostile or malformed values are dropped, not passed through", () => {
  const hostile = {
    colors: { accent: "url(https://x.test)", text: "var(--x)", nonsense: "#fff" },
    radius: 99,
    font_family: "x; background: url(https://x.test)",
    font_size: "constructor",
    density: "__proto__",
  } as unknown as ThemeSettings;
  assert.equal(themeStyle(hostile), undefined);
  assert.equal(themeStyle({ radius: 4.5 }), undefined);
  assert.equal(themeStyle({ radius: -1 }), undefined);
});

test("the empty and default themes add nothing", () => {
  assert.equal(themeStyle(undefined), undefined);
  assert.equal(themeStyle(null), undefined);
  assert.equal(themeStyle({}), undefined);
  assert.equal(themeStyle({ font_size: "medium", density: "comfortable" }), undefined);
});
