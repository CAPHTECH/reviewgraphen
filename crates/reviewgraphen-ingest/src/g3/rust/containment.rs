use super::declaration::{Decision, select};
use super::types::{
    RawKey, RustFactRefV1, RustFileBindingV1, RustG3ReasonV1 as Reason,
    RustInclusiveLineColumnRange as Range,
};
use reviewgraphen_core::{ProgramSpace, StableId};
use serde_json::json;
use std::collections::BTreeSet;

pub(super) fn directed(
    program: &ProgramSpace,
    kind: &'static str,
    parent: &StableId,
    child: &StableId,
) -> Decision {
    let targets = BTreeSet::from([child.clone()]);
    let matches = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == kind
                && relation.directed
                && &relation.source_id == parent
                && relation.target_ids == targets
        })
        .collect::<Vec<_>>();
    let relation = match matches.as_slice() {
        [] => {
            return Err((
                Reason::MissingAcceptedFact,
                json!({"expected_fact_kind":kind}),
            ));
        }
        [one] => *one,
        _ => {
            let mut ids = matches
                .iter()
                .map(|r| r.id.as_str().to_owned())
                .collect::<Vec<_>>();
            ids.sort();
            return Err((Reason::AmbiguousAcceptedFact, json!({"candidate_ids":ids})));
        }
    };
    if program.relation(&relation.id) != Some(relation) {
        return Err((
            Reason::MissingAcceptedFact,
            json!({"expected_fact_kind":kind}),
        ));
    }
    Ok(RustFactRefV1::Relation {
        id: relation.id.clone(),
        source_id: parent.clone(),
        target_ids: targets,
    })
}

pub(super) fn observe(
    key: &RawKey,
    full: Option<Range>,
    source: &RustFileBindingV1,
    program: &ProgramSpace,
) -> Decision {
    let RawKey::Containment {
        parent_logical_name,
        child_logical_name,
        ..
    } = key
    else {
        unreachable!("containment key")
    };
    let placeholder = Range::new(1, 1, 1, 1).expect("fixed placeholder is valid");
    let parent = select(
        program,
        source,
        "module",
        parent_logical_name,
        Some(placeholder),
    )?;
    let child = select(program, source, "function", child_logical_name, full)?;
    directed(program, "contains", &parent.id, &child.id)
}
