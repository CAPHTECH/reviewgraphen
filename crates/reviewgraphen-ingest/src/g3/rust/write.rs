use super::containment::directed;
use super::declaration::{Decision, accepted_location, select};
use super::types::{
    RawKey, RustFactRefV1, RustFileBindingV1, RustG3ReasonV1 as Reason,
    RustInclusiveLineColumnRange as Range, RustLhsKindV1,
};
use reviewgraphen_core::ProgramSpace;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn directed_write(
    program: &ProgramSpace,
    origin: &reviewgraphen_core::StableId,
    state: &reviewgraphen_core::StableId,
    line: u64,
) -> Decision {
    let targets = BTreeSet::from([state.clone()]);
    let matches = program
        .relations()
        .iter()
        .filter(|relation| {
            relation.kind == "writes"
                && relation.directed
                && &relation.source_id == origin
                && relation.target_ids == targets
                && relation.attributes.get("line") == Some(&json!(line))
        })
        .collect::<Vec<_>>();
    let relation = match matches.as_slice() {
        [] => {
            return Err((
                Reason::MissingAcceptedFact,
                json!({"expected_fact_kind":"writes", "line":line}),
            ));
        }
        [only] => *only,
        _ => {
            let mut ids = matches
                .iter()
                .map(|relation| relation.id.as_str().to_owned())
                .collect::<Vec<_>>();
            ids.sort();
            return Err((Reason::AmbiguousAcceptedFact, json!({"candidate_ids":ids})));
        }
    };
    if program.relation(&relation.id) != Some(relation) {
        return Err((
            Reason::MissingAcceptedFact,
            json!({"expected_fact_kind":"writes", "line":line}),
        ));
    }
    Ok(RustFactRefV1::Relation {
        id: relation.id.clone(),
        source_id: origin.clone(),
        target_ids: targets,
    })
}

pub(super) fn observe(
    key: &RawKey,
    full: Option<Range>,
    first: &BTreeMap<String, Range>,
    source: &RustFileBindingV1,
    program: &ProgramSpace,
) -> Decision {
    let RawKey::Assignment {
        occurrence,
        lhs_kind,
        lhs_name,
        owner_logical_name,
        ..
    } = key
    else {
        unreachable!("assignment key")
    };
    if *lhs_kind != RustLhsKindV1::Identifier {
        return Err((
            Reason::UnsupportedAssignmentLhs,
            json!({"lhs_kind":lhs_kind.label()}),
        ));
    }
    let Some(owner) = owner_logical_name else {
        return Err((Reason::UnsupportedOwner, Value::Null));
    };
    let full = full.ok_or((Reason::UnsupportedOwner, Value::Null))?;
    let function = select(program, source, "function", owner, Some(full))?;
    let (module, short) = owner
        .rsplit_once("::")
        .ok_or((Reason::UnsupportedOwner, Value::Null))?;
    let is_test = function
        .attributes
        .get("test_function")
        .and_then(Value::as_bool)
        .ok_or((
            Reason::MissingAcceptedFact,
            json!({"expected_fact_kind":"test_function_attribute"}),
        ))?;
    let source_id = if is_test {
        let test = select(program, source, "test", short, Some(full))?;
        let placeholder = Range::new(1, 1, 1, 1).expect("fixed placeholder");
        let module = select(program, source, "module", module, Some(placeholder))?;
        directed(program, "contains", &module.id, &test.id)?;
        test.id.clone()
    } else {
        function.id.clone()
    };
    let state_label = lhs_name.as_ref().ok_or((
        Reason::UnsupportedAssignmentLhs,
        json!({"lhs_kind":lhs_kind.label()}),
    ))?;
    let expected = first.get(state_label).copied().ok_or((
        Reason::MissingAcceptedFact,
        json!({"expected_fact_kind":"first_state_write"}),
    ))?;
    let state = select(program, source, "state", state_label, None)?;
    if !accepted_location(state.location.as_ref(), source.canonical_path(), expected) {
        return Err((
            Reason::AcceptedLocationMismatch,
            json!({"expected":expected.value(),
            "actual":state.location.as_ref().map(|loc|json!({"start_line":loc.start_line,
                "start_column":loc.start_column,"end_line":loc.end_line,"end_column":loc.end_column}))}),
        ));
    }
    directed_write(program, &source_id, &state.id, occurrence.start_line())
}

#[cfg(test)]
mod line_tests {
    use super::super::partition::test_fixture;
    use super::super::types::{RustFactRefV1, RustG3OutcomeV1};
    use serde_json::json;

    #[test]
    fn repeated_endpoint_writes_resolve_by_line_and_same_line_may_reuse_relation() {
        let source = "fn caller() {\n    let mut value = 0;\n    value = 1;\n    value = 2;\n}\n";
        let result = test_fixture::committed(source).ingest();
        let accepted = result
            .program_space
            .relations()
            .iter()
            .filter(|relation| {
                relation.kind == "writes"
                    && matches!(relation.attributes.get("line"),
                Some(line) if line == &json!(3) || line == &json!(4))
            })
            .collect::<Vec<_>>();
        assert_eq!(
            accepted.len(),
            2,
            "legacy producer has two distinct line-bearing relations"
        );
        let batch = result.g3_observations().expect("actual Git-bound batch");
        assert_eq!(batch.writes().len(), 2);
        for line in [3, 4] {
            let expected = accepted
                .iter()
                .find(|relation| relation.attributes["line"] == json!(line))
                .expect("accepted write for independent literal line");
            let row = batch
                .writes()
                .iter()
                .find(|row| row.occurrence().start_line() == line)
                .expect("source-observed occurrence on this line");
            assert!(
                matches!(row.outcome(), RustG3OutcomeV1::ExistingFact(
                RustFactRefV1::Relation { id, source_id, target_ids })
                if id == &expected.id && source_id == &expected.source_id
                    && target_ids == &expected.target_ids),
                "write on line {line} must choose the line-specific existing relation"
            );
        }

        let same = "fn caller() { let mut value = 0; value = 1; value = 2; }\n";
        let result = test_fixture::committed(same).ingest();
        let accepted = result
            .program_space
            .relations()
            .iter()
            .filter(|r| r.kind == "writes")
            .collect::<Vec<_>>();
        assert_eq!(accepted.len(), 1, "same-line producer relation collapsed");
        let batch = result.g3_observations().expect("Git-bound same-line batch");
        assert_eq!(batch.writes().len(), 2, "distinct occurrence denominator");
        assert!(
            batch.writes().iter().all(|row| matches!(row.outcome(),
            RustG3OutcomeV1::ExistingFact(RustFactRefV1::Relation { id, .. })
                if id == &accepted[0].id)),
            "one accepted same-line relation may be reused"
        );
    }

    #[test]
    fn first_legacy_state_write_in_method_or_closure_precedes_later_free_write() {
        let cases = [
            (
                "method",
                "struct Holder;\nimpl Holder { fn method(&self) {\n    let mut value = 0;\n    value = 1;\n} }\nfn caller() {\n    let mut value = 0;\n    value = 2;\n}\n",
                4,
                8,
            ),
            (
                "closure",
                "fn holder() {\n    let mut value = 0;\n    let action = || { value = 1; };\n}\nfn caller() {\n    let mut value = 0;\n    value = 2;\n}\n",
                3,
                7,
            ),
            (
                "compound",
                "fn holder() {\n    let mut value = 0;\n    value += 1;\n}\nfn caller() {\n    let mut value = 0;\n    value = 2;\n}\n",
                3,
                7,
            ),
        ];
        for (kind, source, first_line, later_line) in cases {
            let result = test_fixture::committed(source).ingest();
            let program = &result.program_space;
            let state = program
                .artifacts()
                .iter()
                .find(|a| a.kind == "state" && a.label == "value")
                .expect("legacy file-wide state deduplicated by label");
            assert_eq!(
                state.location.as_ref().and_then(|loc| loc.start_line),
                Some(first_line),
                "{kind}: legacy state location derives from first producer write"
            );
            let caller = program
                .artifacts()
                .iter()
                .find(|a| a.kind == "function" && a.label == "crate::structural::caller")
                .expect("later accepted free function");
            let accepted = program
                .relations()
                .iter()
                .find(|r| {
                    r.kind == "writes"
                        && r.source_id == caller.id
                        && r.target_ids.contains(&state.id)
                        && r.attributes.get("line") == Some(&json!(later_line))
                })
                .expect("later accepted directed caller write with independent line");
            let batch = result.g3_observations().expect("committed-source G3 batch");
            let row = batch
                .writes()
                .iter()
                .find(|row| row.occurrence().start_line() == later_line)
                .expect("later source-observed assignment");
            assert!(
                matches!(row.outcome(),RustG3OutcomeV1::ExistingFact(
                RustFactRefV1::Relation { id,.. }) if id == &accepted.id),
                "{kind}: first-state location must honor preceding producer write, not only eligible free ExprAssign"
            );
        }
    }
}
