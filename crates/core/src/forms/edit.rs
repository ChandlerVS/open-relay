//! Positional edits to a `layout`, for callers that hold one element rather
//! than the whole document.
//!
//! # Why these are pure, and why they validate nothing
//!
//! Every function here is `Vec<FormElement> -> CoreResult<Vec<FormElement>>`
//! with no database and no registry. The caller is expected to hand the result
//! straight back to [`super::service::update_form`], which runs
//! `normalize_layout`, `validate_layout` and `legacy_from_layout` exactly as it
//! would for a whole-document write. That ordering is the safety property: an
//! edit here cannot produce a layout the REST API would have rejected, cannot
//! skip the backwards-pointing checks on `visible_when` / `country_field`, and
//! cannot let the legacy columns go stale.
//!
//! So these functions deliberately do **not** re-check rules, keys, row balance
//! or page-break placement. Duplicating `validate_layout` here would be a second
//! transcription of a spec that has one owner, and the two would drift.
//!
//! The errors they *do* raise are the ones `validate_layout` cannot phrase
//! usefully: "there is no element with that key", "index 9 in a layout of 4".
//! Those are addressing mistakes, and the caller needs them named before the
//! edit rather than as a downstream structural complaint.
//!
//! # Rows move as a unit
//!
//! Rows are a flat `RowStart`/`RowEnd` marker pair (see the module docs on
//! [`super::FormElement`]), which means a naive splice can invert or strand a
//! marker. Rather than repair that afterwards the way the builder's
//! `normalizeRows` does, the two operations that could cause it — remove and
//! move — treat a marker as standing for its whole row. Half a pair is never
//! constructed in the first place.

use crate::error::{CoreError, CoreResult};
use crate::forms::FormElement;

/// How a caller names the element to act on.
///
/// `Key` is the form an agent will almost always want: field keys are stable
/// across edits, whereas every insertion renumbers the indices after it.
/// `Index` exists for the decorations — headings, dividers, row markers — which
/// register no key and so cannot be addressed any other way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElementRef {
    Key(String),
    Index(usize),
}

impl ElementRef {
    /// Resolve to a position in `layout`.
    fn resolve(&self, layout: &[FormElement]) -> CoreResult<usize> {
        match self {
            ElementRef::Index(i) => {
                if *i >= layout.len() {
                    return Err(CoreError::BadRequest(format!(
                        "no element at index {i}: the layout has {} element(s)",
                        layout.len()
                    )));
                }
                Ok(*i)
            }
            ElementRef::Key(key) => layout
                .iter()
                .position(|el| el.field_key() == Some(key.as_str()))
                .ok_or_else(|| {
                    CoreError::BadRequest(format!("no field with key {key:?} in this form"))
                }),
        }
    }
}

/// The half-open span an element occupies for the purposes of remove and move.
///
/// One element for everything except a row marker, which stands for the whole
/// `RowStart..=RowEnd` run so a pair can never be split.
fn span(layout: &[FormElement], i: usize) -> CoreResult<(usize, usize)> {
    match &layout[i] {
        FormElement::RowStart(_) => {
            let end = layout
                .iter()
                .skip(i + 1)
                .position(|e| matches!(e, FormElement::RowEnd))
                .map(|off| i + 1 + off)
                .ok_or_else(|| {
                    // Only reachable if a malformed layout was stored before
                    // `validate_layout` existed in its current form.
                    CoreError::BadRequest(
                        "this form has a row that is never closed; send a full layout to repair it"
                            .into(),
                    )
                })?;
            Ok((i, end + 1))
        }
        FormElement::RowEnd => {
            let start = layout[..i]
                .iter()
                .rposition(|e| matches!(e, FormElement::RowStart(_)))
                .ok_or_else(|| {
                    CoreError::BadRequest(
                        "this form has a row end with no start; send a full layout to repair it"
                            .into(),
                    )
                })?;
            Ok((start, i + 1))
        }
        _ => Ok((i, i + 1)),
    }
}

fn check_insert_at(layout: &[FormElement], at: usize) -> CoreResult<()> {
    // `len` is legal — that is an append.
    if at > layout.len() {
        return Err(CoreError::BadRequest(format!(
            "cannot insert at index {at}: the layout has {} element(s)",
            layout.len()
        )));
    }
    Ok(())
}

/// Insert `element` at `at`, or append when `at` is `None`.
///
/// An index that lands strictly inside a row is *not* repaired: the caller named
/// a position explicitly, so if the element cannot sit in a row `validate_layout`
/// will say so by name. That is the same split the legacy write paths take the
/// other side of — they repair, because their callers cannot see rows at all.
pub fn add_element(
    mut layout: Vec<FormElement>,
    element: FormElement,
    at: Option<usize>,
) -> CoreResult<Vec<FormElement>> {
    let at = match at {
        Some(at) => {
            check_insert_at(&layout, at)?;
            at
        }
        None => layout.len(),
    };
    layout.insert(at, element);
    Ok(layout)
}

/// Replace the element named by `target` with `element`.
///
/// Replacing a row marker is refused. A marker carries only its label, so the
/// only coherent "replacement" is an edit to that label — and swapping a marker
/// for a field would silently unbalance the pair.
pub fn update_element(
    mut layout: Vec<FormElement>,
    target: &ElementRef,
    element: FormElement,
) -> CoreResult<Vec<FormElement>> {
    let i = target.resolve(&layout)?;
    if matches!(
        layout[i],
        FormElement::RowStart(_) | FormElement::RowEnd
    ) && !matches!(element, FormElement::RowStart(_))
    {
        return Err(CoreError::BadRequest(
            "cannot replace a row marker with another element; remove the row or edit its label"
                .into(),
        ));
    }
    layout[i] = element;
    Ok(layout)
}

/// Remove the element named by `target`.
///
/// Naming either row marker removes the whole row, contents included. Deleting
/// one marker alone would leave the other stranded, which is a 400 — so the
/// operation that would produce it simply does not exist.
pub fn remove_element(
    mut layout: Vec<FormElement>,
    target: &ElementRef,
) -> CoreResult<Vec<FormElement>> {
    let i = target.resolve(&layout)?;
    let (start, end) = span(&layout, i)?;
    layout.drain(start..end);
    Ok(layout)
}

/// Move the element named by `target` so it lands at index `to`.
///
/// `to` is read against the layout *with the element removed*, which is the
/// reading that makes "move it to the end" mean `to == len - 1` rather than
/// something that depends on which direction the element travelled. A row moves
/// as a block, and its `to` is where the `RowStart` ends up.
pub fn move_element(
    mut layout: Vec<FormElement>,
    target: &ElementRef,
    to: usize,
) -> CoreResult<Vec<FormElement>> {
    let i = target.resolve(&layout)?;
    let (start, end) = span(&layout, i)?;
    let block: Vec<FormElement> = layout.drain(start..end).collect();
    check_insert_at(&layout, to)?;
    layout.splice(to..to, block);
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms::{
        CustomField, CustomFieldType, FieldWidth, HeadingElement, RowStartElement, StandardElement,
    };

    fn custom(key: &str) -> FormElement {
        FormElement::Custom(CustomField {
            key: key.into(),
            label: key.into(),
            kind: CustomFieldType::Text,
            required: false,
            placeholder: None,
            help_text: None,
            position: 0,
            width: FieldWidth::Full,
            default_value: None,
            visible_when: None,
        })
    }

    fn standard(key: &str) -> FormElement {
        FormElement::Standard(StandardElement {
            key: key.into(),
            required: false,
            label: None,
            placeholder: None,
            help_text: None,
            width: FieldWidth::Full,
            default_value: None,
            input_override: None,
            visible_when: None,
        })
    }

    fn heading() -> FormElement {
        FormElement::Heading(HeadingElement {
            text: "Hi".into(),
            level: 2,
            visible_when: None,
        })
    }

    /// Compact rendering, so an assertion reads as the shape it is checking.
    fn shape(layout: &[FormElement]) -> Vec<String> {
        layout
            .iter()
            .map(|el| match el {
                FormElement::Standard(s) => format!("std:{}", s.key),
                FormElement::Custom(c) => format!("cus:{}", c.key),
                FormElement::Heading(_) => "heading".into(),
                FormElement::Paragraph(_) => "paragraph".into(),
                FormElement::RichText(_) => "rich_text".into(),
                FormElement::Divider => "divider".into(),
                FormElement::PageBreak(_) => "page_break".into(),
                FormElement::RowStart(_) => "row_start".into(),
                FormElement::RowEnd => "row_end".into(),
            })
            .collect()
    }

    fn row_of(keys: &[&str]) -> Vec<FormElement> {
        let mut v = vec![FormElement::RowStart(RowStartElement { label: None })];
        v.extend(keys.iter().map(|k| custom(k)));
        v.push(FormElement::RowEnd);
        v
    }

    #[test]
    fn add_appends_by_default_and_inserts_at_an_index() {
        let layout = vec![custom("a"), custom("b")];
        let appended = add_element(layout.clone(), custom("c"), None).unwrap();
        assert_eq!(shape(&appended), ["cus:a", "cus:b", "cus:c"]);

        let inserted = add_element(layout, custom("c"), Some(1)).unwrap();
        assert_eq!(shape(&inserted), ["cus:a", "cus:c", "cus:b"]);
    }

    #[test]
    fn an_out_of_range_index_is_named_rather_than_clamped() {
        // Silently appending would let an agent that miscounted believe it had
        // inserted somewhere specific.
        let layout = vec![custom("a")];
        let err = add_element(layout.clone(), custom("b"), Some(5)).unwrap_err();
        assert!(format!("{err}").contains("1 element"), "{err}");
        // `len` itself is an append, not an error.
        assert!(add_element(layout, custom("b"), Some(1)).is_ok());
    }

    #[test]
    fn elements_are_addressable_by_key_and_a_missing_key_says_so() {
        let layout = vec![standard("email"), custom("nickname")];
        let out = remove_element(layout.clone(), &ElementRef::Key("email".into())).unwrap();
        assert_eq!(shape(&out), ["cus:nickname"]);

        let err = remove_element(layout, &ElementRef::Key("nope".into())).unwrap_err();
        assert!(format!("{err}").contains("nope"), "{err}");
    }

    #[test]
    fn removing_either_row_marker_takes_the_whole_row() {
        let mut layout = vec![custom("before")];
        layout.extend(row_of(&["city", "state"]));
        layout.push(custom("after"));

        // Naming the start.
        let out = remove_element(layout.clone(), &ElementRef::Index(1)).unwrap();
        assert_eq!(shape(&out), ["cus:before", "cus:after"]);

        // Naming the end — same result, no stranded partner.
        let end = layout.iter().position(|e| matches!(e, FormElement::RowEnd)).unwrap();
        let out = remove_element(layout, &ElementRef::Index(end)).unwrap();
        assert_eq!(shape(&out), ["cus:before", "cus:after"]);
    }

    #[test]
    fn removing_a_field_inside_a_row_leaves_the_row_intact() {
        let layout = row_of(&["city", "state"]);
        let out = remove_element(layout, &ElementRef::Key("city".into())).unwrap();
        assert_eq!(shape(&out), ["row_start", "cus:state", "row_end"]);
    }

    #[test]
    fn move_reads_its_target_against_the_layout_without_the_element() {
        let layout = vec![custom("a"), custom("b"), custom("c")];

        // Forwards to the end.
        let out = move_element(layout.clone(), &ElementRef::Key("a".into()), 2).unwrap();
        assert_eq!(shape(&out), ["cus:b", "cus:c", "cus:a"]);

        // Backwards to the front. Same index arithmetic in both directions.
        let out = move_element(layout, &ElementRef::Key("c".into()), 0).unwrap();
        assert_eq!(shape(&out), ["cus:c", "cus:a", "cus:b"]);
    }

    #[test]
    fn moving_a_row_moves_the_block() {
        let mut layout = vec![custom("first")];
        layout.extend(row_of(&["city", "state"]));

        let out = move_element(layout, &ElementRef::Index(1), 0).unwrap();
        assert_eq!(
            shape(&out),
            ["row_start", "cus:city", "cus:state", "row_end", "cus:first"]
        );
    }

    #[test]
    fn a_row_marker_cannot_be_replaced_by_a_field() {
        let layout = row_of(&["city"]);
        let err = update_element(layout.clone(), &ElementRef::Index(0), heading()).unwrap_err();
        assert!(format!("{err}").contains("row marker"), "{err}");

        // Editing the label — a row_start for a row_start — is allowed.
        let out = update_element(
            layout,
            &ElementRef::Index(0),
            FormElement::RowStart(RowStartElement {
                label: Some("Address".into()),
            }),
        )
        .unwrap();
        assert!(matches!(&out[0], FormElement::RowStart(r) if r.label.as_deref() == Some("Address")));
    }

    #[test]
    fn update_replaces_in_place_without_reordering() {
        let layout = vec![custom("a"), custom("b"), custom("c")];
        let out = update_element(layout, &ElementRef::Key("b".into()), heading()).unwrap();
        assert_eq!(shape(&out), ["cus:a", "heading", "cus:c"]);
    }
}
