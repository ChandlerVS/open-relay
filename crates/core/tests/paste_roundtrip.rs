//! The admin builder's paste transform must emit a layout this crate accepts.
//!
//! An explicit `layout` write gets no server-side repair — `strip_dangling_rules`
//! and `strip_dangling_country_refs` run only on the legacy write paths — so the
//! builder repairs a pasted block itself (`apps/admin/src/pages/admin/forms/
//! builder/blockOps.ts`). This pins the result of that against the real
//! validator, which is the half of the contract TypeScript cannot check.
//!
//! The fixture is a billing block pasted **twice into the same form**: the second
//! copy has every custom key re-keyed (`billing_city` → `billing_city_2`) and its
//! rules and country binding repointed at the copy's own keys. Without that
//! repair this layout is a 400, which makes it the case worth pinning.
//!
//! To regenerate after changing the transform, run `prepareForPaste` twice over
//! an empty form and write `stripIds(outcome.items)` here.
//!
//! DB-free, so it runs in the default `cargo test` rather than behind `--ignored`.
use open_relay_core::forms::FormElement;

#[test]
fn twice_pasted_block_is_a_layout_the_server_accepts() {
    let raw = include_str!("fixtures/pasted_layout.json");
    let layout: Vec<FormElement> = serde_json::from_str(raw).expect("deserialises");
    assert_eq!(layout.len(), 14, "7 elements, pasted twice");

    // The copy is independent: it names its own keys, not the original's.
    let keys: Vec<&str> = layout.iter().filter_map(|e| e.field_key()).collect();
    assert!(keys.contains(&"billing_city"), "the original survives");
    assert!(keys.contains(&"billing_city_2"), "the copy was re-keyed");

    open_relay_core::forms::service::validate_layout(&layout).expect("validates");
}
