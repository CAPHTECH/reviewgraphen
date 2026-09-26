//! Non-authority, complete-key multiset accounting for one parsed file.

use super::G3AccountingError;
use reviewgraphen_ingest::typescript::g3_syntax::{RawTypeScriptFileV1, RawTypeScriptKeyV1};
use std::collections::BTreeMap;

fn counts(
    keys: impl IntoIterator<Item = RawTypeScriptKeyV1>,
) -> BTreeMap<RawTypeScriptKeyV1, usize> {
    let mut counts = BTreeMap::new();
    for key in keys {
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

/// Independently enumerated AST keys equal emitted candidate keys, with
/// multiplicity preserved. No BTreeSet can hide a repeated occurrence.
pub(super) fn verify_raw(raw: &RawTypeScriptFileV1) -> Result<(), G3AccountingError> {
    let observed = counts(raw.census.iter().cloned());
    let emitted = counts(
        raw.containment
            .iter()
            .cloned()
            .map(RawTypeScriptKeyV1::Containment)
            .chain(
                raw.assignments
                    .iter()
                    .cloned()
                    .map(RawTypeScriptKeyV1::Assignment),
            )
            .chain(raw.calls.iter().cloned().map(RawTypeScriptKeyV1::Call)),
    );
    if observed.values().any(|count| *count != 1) || emitted.values().any(|count| *count != 1) {
        return Err(G3AccountingError::DuplicateOccurrence);
    }
    if observed.keys().any(|key| !emitted.contains_key(key)) {
        return Err(G3AccountingError::MissingOccurrence);
    }
    if emitted.keys().any(|key| !observed.contains_key(key)) {
        return Err(G3AccountingError::UnexpectedOccurrence);
    }
    Ok(())
}

/// Compare exact supported keys with final dispositions, including both arms.
pub(super) fn verify_bound(
    raw: &RawTypeScriptFileV1,
    success: &[RawTypeScriptKeyV1],
    obstructed: &[RawTypeScriptKeyV1],
) -> Result<(), G3AccountingError> {
    let expected = counts(raw.census.iter().cloned());
    let successes = counts(success.iter().cloned());
    let obstructions = counts(obstructed.iter().cloned());
    if successes.values().any(|count| *count > 1) || obstructions.values().any(|count| *count > 1) {
        return Err(G3AccountingError::DuplicateOccurrence);
    }
    if successes.keys().any(|key| obstructions.contains_key(key)) {
        return Err(G3AccountingError::DoubleDisposition);
    }
    if expected
        .keys()
        .any(|key| !successes.contains_key(key) && !obstructions.contains_key(key))
    {
        return Err(G3AccountingError::MissingOccurrence);
    }
    if successes
        .keys()
        .chain(obstructions.keys())
        .any(|key| !expected.contains_key(key))
    {
        return Err(G3AccountingError::UnexpectedOccurrence);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reviewgraphen_ingest::typescript::g3_syntax::parse_g3_syntax;

    #[test]
    fn duplicated_and_disappearing_dispositions_are_accounting_errors() {
        let raw = parse_g3_syntax(b"let a = 0; a = 1;\n").unwrap();
        verify_raw(&raw).unwrap();
        assert_eq!(
            verify_bound(&raw, &[], &[]),
            Err(G3AccountingError::MissingOccurrence)
        );
        let key = raw.census[0].clone();
        assert_eq!(
            verify_bound(&raw, std::slice::from_ref(&key), std::slice::from_ref(&key)),
            Err(G3AccountingError::DoubleDisposition)
        );
        assert_eq!(
            verify_bound(&raw, &[key.clone(), key], &[]),
            Err(G3AccountingError::DuplicateOccurrence)
        );
    }
}
