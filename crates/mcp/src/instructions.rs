//! The `instructions` string handed to a client at initialize.
//!
//! This is the only place an agent learns the rules that a JSON Schema cannot
//! state: which references may point where, which elements may sit inside
//! which, and what the server will reject. Every constraint below is enforced
//! by `forms::service::validate_layout`, so this text is a summary of that
//! function, not a second specification — when it changes, this changes.

pub const INSTRUCTIONS: &str = r#"
OpenRelay form authoring.

A form's shape is its `layout`: a FLAT, ordered list of elements. Call
`describe_form_schema` for the exact JSON Schema of every element type.

Elements are adjacently tagged: {"element": "<kind>", "config": {...}}.
`divider` and `row_end` are the two that take no config: {"element": "divider"}.

RULES THE SERVER ENFORCES (a violation is an error, not a warning):

1. References point strictly BACKWARDS. A `visible_when` condition may only
   name a field that appears EARLIER in the layout, and a `state` field's
   `country_field` may only name an earlier country field. This is what makes
   the form resolvable in one pass; it also rules out cycles and
   self-references.

2. Rows are a flat marker PAIR, not a container. Put fields side by side by
   surrounding them with {"element":"row_start","config":{}} and
   {"element":"row_end"}. Rows may not nest, may hold at most 4 elements, and
   may hold ONLY `standard` and `custom` fields — no headings, page breaks or
   rich text. Inside a row, `width` is a weight rather than a column span.

3. Pages are separators, not nesting. A form is multi-step when its layout
   contains `page_break` elements. A break may not be first, may not be last,
   and two may not be adjacent; at most 20 pages.

4. Redirect URLs must be absolute http(s). A relative, scheme-relative or
   `javascript:` URL is rejected — this is a security boundary, since the
   value is handed to the browser on a third-party page.

5. Send `layout` OR the legacy `standard_fields`/`custom_fields` pair, never
   both. Prefer `layout`: it is the source of truth, and the legacy pair is
   recomputed from it on every write.

STANDARD VS CUSTOM FIELDS
  Standard fields are a fixed catalogue (first_name, last_name, email, phone,
  company, job_title, website, message, address_line_1, address_line_2, city,
  state, postal_code, country) and each may appear AT MOST ONCE. If a form
  needs two addresses, use `custom` fields of type `country`/`state` for the
  second — that is exactly why those custom types exist.

EDITING
  To change one element, prefer `add_element` / `update_element` /
  `remove_element` / `move_element` over rewriting the whole layout. They take
  a `key` (a field's key) or an `index`, edit in place, and re-run the full
  validation. Naming either marker of a row removes or moves the WHOLE row.

  `update_form` with a `layout` REPLACES the layout entirely. Read the form
  first if you mean to preserve what is there.

COMPATIBILITY
  Forms are embedded on third-party pages by a cached script that may predate
  a feature. An old bundle renders `rich_text` and `file` fields as NOTHING,
  and stacks rows instead of laying them out. Do not put legally load-bearing
  copy in a rich-text block, and be careful making a `file` field required on
  a widely-embedded form.
"#;
