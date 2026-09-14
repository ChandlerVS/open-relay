import { test } from "node:test";
import assert from "node:assert/strict";

import { computeVisibility, type Values } from "./visibility.ts";
import type { ConditionOp, FormElement } from "./schema.ts";

/**
 * The array half of the visibility spec. `crates/core/src/forms/visibility.rs`
 * runs the same cases in `an_array_answer_is_read_as_a_set` — the two copies
 * must agree exactly, or a visitor sees one form and the server validates
 * another.
 */

function layout(op: ConditionOp, value?: string): FormElement[] {
  return [
    {
      element: "custom",
      config: {
        key: "gear",
        label: "Gear",
        type: "checkboxes",
        options: ["Mobile Computers", "Label Printers"],
        position: 0,
      },
    },
    {
      element: "custom",
      config: {
        key: "city",
        label: "City",
        type: "text",
        position: 1,
        visible_when: { conditions: [{ field: "gear", op, ...(value ? { value } : {}) }] },
      },
    },
  ];
}

const shown = (op: ConditionOp, value: string | undefined, values: Values) =>
  computeVisibility(layout(op, value), values).visible[1];

const ticked: Values = { gear: ["Mobile Computers", " Label Printers "] };
const none: Values = { gear: [] };

test("equals means 'has ticked', case-insensitively and trimmed", () => {
  assert.equal(shown("equals", "label printers", ticked), true);
  assert.equal(shown("equals", "Barcode Scanners", ticked), false);
  assert.equal(shown("equals", "Label Printers", none), false);
});

test("not_equals means 'has not ticked'", () => {
  assert.equal(shown("not_equals", "Label Printers", ticked), false);
  assert.equal(shown("not_equals", "Barcode Scanners", ticked), true);
});

test("contains matches inside any element", () => {
  assert.equal(shown("contains", "printer", ticked), true);
});

test("emptiness asks whether anything is ticked", () => {
  assert.equal(shown("is_not_empty", undefined, ticked), true);
  assert.equal(shown("is_empty", undefined, ticked), false);
  assert.equal(shown("is_empty", undefined, none), true);
  assert.equal(shown("is_empty", undefined, {}), true, "unanswered reads like an empty set");
});
