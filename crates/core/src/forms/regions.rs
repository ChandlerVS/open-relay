//! ISO 3166 country and subdivision catalogues.
//!
//! The tables are generated into `regions.json` by `scripts/gen-regions.mjs`,
//! which emits the byte-identical payload into
//! `packages/form-renderer/src/regions.data.ts` at the same time. Neither file
//! is hand-written: two hand-maintained copies of a 3,590-row table is the
//! drift hazard the module docs on [`super::visibility`] describe, and here it
//! is avoidable because the data has a single upstream source.
//!
//! # Packed format
//!
//! Countries are `"US:United States|CA:Canada"`, name-sorted. Subdivisions are
//! `"US=AL:Alabama|AK:Alaska~CA=AB:Alberta"` — `~` between countries, `=`
//! after the alpha-2, `|` between subdivisions, `:` between code and name. The
//! generator asserts no name contains a delimiter.
//!
//! # Top level only
//!
//! Only ISO 3166-2 entries *without* a parent are included. The nested tier is
//! a different kind of thing — the UK's 200-odd districts sit under England /
//! Scotland / Wales / Northern Ireland, France's departments under its regions
//! — and a "state / province" picker wants the top tier. One consequence worth
//! knowing: Ireland's top level is its four provinces, not its counties.
//!
//! # Stored value
//!
//! A country answer is the alpha-2 code (`US`); a subdivision answer is the
//! **bare** subdivision code (`CA`), not the full ISO 3166-2 code (`US-CA`).
//! The country is captured by the sibling field, and the bare form is what
//! CRMs expect — `backend::gohighlevel` forwards `state` verbatim.

use std::collections::HashMap;
use std::sync::LazyLock;

/// The generated tables, as emitted by `scripts/gen-regions.mjs`.
#[derive(serde::Deserialize)]
struct Packed {
    countries: String,
    subdivisions: String,
}

static PACKED: LazyLock<Packed> = LazyLock::new(|| {
    serde_json::from_str(include_str!("regions.json"))
        .expect("regions.json is generated and must parse")
});

/// The raw packed subdivision table, handed to the renderer verbatim on
/// [`super::PublicFormDto::regions`] so the embed bundle doesn't have to carry
/// it. See that field's docs for why it isn't always sent.
pub fn packed_subdivisions() -> &'static str {
    &PACKED.subdivisions
}

fn parse_pairs(packed: &'static str) -> Vec<(&'static str, &'static str)> {
    packed
        .split('|')
        .filter(|e| !e.is_empty())
        .map(|entry| {
            let (code, name) = entry
                .split_once(':')
                .expect("generated entry is always code:name");
            (code, name)
        })
        .collect()
}

static COUNTRIES: LazyLock<Vec<(&'static str, &'static str)>> =
    LazyLock::new(|| parse_pairs(&PACKED.countries));

static SUBDIVISIONS: LazyLock<HashMap<&'static str, Vec<(&'static str, &'static str)>>> =
    LazyLock::new(|| {
        PACKED
            .subdivisions
            .split('~')
            .filter(|e| !e.is_empty())
            .map(|entry| {
                let (country, list) = entry
                    .split_once('=')
                    .expect("generated entry is always CC=subdivisions");
                (country, parse_pairs(list))
            })
            .collect()
    });

/// Every ISO 3166-1 country as `(alpha-2, display name)`, name-sorted.
pub fn countries() -> &'static [(&'static str, &'static str)] {
    &COUNTRIES
}

/// Whether `code` is a known ISO 3166-1 alpha-2 code. Case-sensitive: callers
/// upper-case first, because that is the canonical stored form.
pub fn is_country_code(code: &str) -> bool {
    COUNTRIES.iter().any(|(c, _)| *c == code)
}

/// The top-level subdivisions of `country`, or `None` when it has none. 49 of
/// the 249 countries have none — a state field pointed at one of those renders
/// free text rather than an empty dropdown.
pub fn subdivisions(country: &str) -> Option<&'static [(&'static str, &'static str)]> {
    SUBDIVISIONS.get(country).map(Vec::as_slice)
}

/// Whether `code` is a top-level subdivision of `country`. Both are compared
/// in their canonical upper-case form.
pub fn is_subdivision_of(country: &str, code: &str) -> bool {
    subdivisions(country).is_some_and(|list| list.iter().any(|(c, _)| *c == code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_generated_tables_parse() {
        assert_eq!(countries().len(), 249);
        assert_eq!(SUBDIVISIONS.len(), 200);
        assert_eq!(
            SUBDIVISIONS.values().map(Vec::len).sum::<usize>(),
            3590,
            "total top-level subdivisions"
        );
    }

    #[test]
    fn countries_are_name_sorted_and_looked_up_by_code() {
        assert!(is_country_code("US"));
        assert!(is_country_code("NZ"));
        assert!(!is_country_code("us"), "lookups are canonical upper-case");
        assert!(!is_country_code("XX"));
        assert_eq!(countries().first().unwrap().1, "Afghanistan");
    }

    #[test]
    fn subdivisions_are_the_top_tier_only() {
        // 50 states + DC + 6 territories.
        assert_eq!(subdivisions("US").unwrap().len(), 57);
        assert_eq!(subdivisions("CA").unwrap().len(), 13);
        // Not the 200-odd districts nested beneath these four.
        assert_eq!(subdivisions("GB").unwrap().len(), 4);
        assert!(is_subdivision_of("US", "CA"));
        assert!(is_subdivision_of("CA", "BC"));
        assert!(!is_subdivision_of("CA", "TX"), "wrong country");
        assert!(!is_subdivision_of("US", "ca"));
    }

    #[test]
    fn a_country_without_subdivisions_reads_as_none() {
        // Hong Kong has no ISO 3166-2 entries; a state field bound to it
        // renders free text rather than an empty dropdown.
        assert!(subdivisions("HK").is_none());
        assert!(subdivisions("BM").is_none());
        assert!(subdivisions("XX").is_none());
    }

    #[test]
    fn display_names_have_their_bracketed_alternates_stripped() {
        let gb = subdivisions("GB").unwrap();
        let wales = gb.iter().find(|(c, _)| *c == "WLS").unwrap();
        assert_eq!(wales.1, "Wales", "not `Wales [Cymru GB-CYM]`");
    }
}
