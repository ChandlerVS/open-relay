# Vendored ISO 3166 data

`iso_3166-1.json` (countries) and `iso_3166-2.json` (subdivisions) are taken
verbatim from the Debian **iso-codes** project:

<https://salsa.debian.org/iso-codes-team/iso-codes> — `data/`

iso-codes is distributed under the **GNU LGPL v2.1**; see `LICENSE-LGPL-2.1`
in this directory. ISO does not freely license its own lists, which is why this
project uses the Debian compilation rather than the standard's own tables.

These files are inputs, not outputs. `scripts/gen-regions.mjs` reads them and
emits the two checked-in catalogues the code actually loads:

- `crates/core/src/forms/regions.json`
- `packages/form-renderer/src/regions.data.ts`

To refresh, replace both files from upstream and run `pnpm gen:regions`.
`pnpm gen:regions:check` asserts the generated pair is current.
