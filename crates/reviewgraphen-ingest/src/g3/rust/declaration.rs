use super::types::{
    RawKey, RustFactRefV1, RustFileBindingV1, RustG3ReasonV1 as Reason,
    RustInclusiveLineColumnRange as Range,
};
use reviewgraphen_core::{Artifact, Location, ProgramSpace};
use serde_json::{Value, json};

pub(super) type Decision = Result<RustFactRefV1, (Reason, Value)>;

pub(super) fn accepted_location(location: Option<&Location>, path: &str, expected: Range) -> bool {
    location.is_some_and(|loc| {
        loc.path == path
            && loc.start_line == Some(expected.start_line())
            && loc.start_column == Some(expected.start_column())
            && loc.end_line == Some(expected.end_line())
            && loc.end_column == Some(expected.end_column())
    })
}

pub(super) fn select<'a>(
    program: &'a ProgramSpace,
    source: &RustFileBindingV1,
    kind: &'static str,
    label: &str,
    expected: Option<Range>,
) -> Result<&'a Artifact, (Reason, Value)> {
    let found = program
        .artifacts()
        .iter()
        .filter(|artifact| {
            artifact.kind == kind
                && artifact.language.as_deref() == Some("rust")
                && artifact.label == label
                && artifact.content_hash.as_ref() == Some(source.source_hash())
                && artifact
                    .location
                    .as_ref()
                    .is_some_and(|loc| loc.path == source.canonical_path())
        })
        .collect::<Vec<_>>();
    let candidate = match found.as_slice() {
        [] => {
            return Err((
                Reason::MissingAcceptedFact,
                json!({"expected_fact_kind":kind}),
            ));
        }
        [one] => *one,
        _ => {
            let mut ids = found
                .iter()
                .map(|item| item.id.as_str().to_owned())
                .collect::<Vec<_>>();
            ids.sort();
            return Err((Reason::AmbiguousAcceptedFact, json!({"candidate_ids":ids})));
        }
    };
    if let Some(expected) = expected
        && !accepted_location(
            candidate.location.as_ref(),
            source.canonical_path(),
            expected,
        )
    {
        return Err((
            Reason::AcceptedLocationMismatch,
            json!({"expected":expected.value(),
            "actual":candidate.location.as_ref().map(|loc|json!({"start_line":loc.start_line,
                "start_column":loc.start_column,"end_line":loc.end_line,"end_column":loc.end_column}))}),
        ));
    }
    if program.artifact(&candidate.id) != Some(candidate) {
        return Err((
            Reason::MissingAcceptedFact,
            json!({"expected_fact_kind":kind}),
        ));
    }
    Ok(candidate)
}

pub(super) fn observe(
    key: &RawKey,
    full: Option<Range>,
    source: &RustFileBindingV1,
    program: &ProgramSpace,
) -> Decision {
    let RawKey::Declaration { logical_name, .. } = key else {
        unreachable!("declaration key")
    };
    let full = full.ok_or((Reason::AcceptedLocationMismatch, Value::Null))?;
    let artifact = select(program, source, "function", logical_name, Some(full))?;
    Ok(RustFactRefV1::Artifact {
        id: artifact.id.clone(),
    })
}
