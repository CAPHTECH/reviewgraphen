//! Compile-red acceptance for the frozen S0 r2 public registry read seam.
//! Expected values are independently owned literals from S0-DEFINITIONS.round8.json.

use reviewgraphen_core::ContentHash;
use reviewgraphen_core::source_review::reasons::{
    BindingReasonV1, BindingStageV1, ResolutionOutcomeV1,
};
use reviewgraphen_core::source_review::registry::{
    TypeScriptRegistryAccessError, typescript_r2_policy_view, typescript_registry_binding,
    typescript_registry_definition_bytes,
};

const EXPECTED_DEFINITION_LENGTH: usize = 131_272;
const EXPECTED_DEFINITION_SHA256: &str =
    "sha256:4e4aa59c25c1c43257eb946de2aa0a99ad0ac193ad21b72a64f2163a1ec99a51";
const EXPECTED_REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r2";

struct StageExpectation {
    wire: &'static str,
    order: u8,
    prerequisite: &'static str,
    observation: &'static str,
}

const EXPECTED_BINDING_STAGES: [StageExpectation; 7] = [
    StageExpectation {
        wire: "b.form",
        order: 1,
        prerequisite: "caller import sourceがparsed",
        observation: "import form/type-only/bare/namespaceとlocal名・slot観測",
    },
    StageExpectation {
        wire: "b.local_uniqueness",
        order: 2,
        prerequisite: "b.form",
        observation: "同file local名の宣言/import競合を調べる",
    },
    StageExpectation {
        wire: "b.specifier",
        order: 3,
        prerequisite: "b.formでstatic相対stringが得られる",
        observation: "相対specifierの文法/正規化。type-onlyも観測可能",
    },
    StageExpectation {
        wire: "b.candidates",
        order: 4,
        prerequisite: "b.specifierで正規化候補pathを作れる",
        observation: "全candidate/rejection witnessを先に走査。失敗理由を全収集",
    },
    StageExpectation {
        wire: "b.export_binding",
        order: 5,
        prerequisite: "一意included正常parsed候補、runtime named/default",
        observation: "callee export tableを単一runtime callableへ照合",
    },
    StageExpectation {
        wire: "b.writes",
        order: 6,
        prerequisite: "調べるlocal又はcallee元bindingがsourceで特定済み",
        observation: "観測可能な対象bindingのwriteを確認。未特定側は推測しない",
    },
    StageExpectation {
        wire: "b.result",
        order: 7,
        prerequisite: "それまでの実評価witnessがある",
        observation: "resolved/unresolvedとresolved_function_idをまとめる",
    },
];

const EXPECTED_CALL_STAGES: [StageExpectation; 10] = [
    StageExpectation {
        wire: "c.syntax",
        order: 1,
        prerequisite: "正常parsed caller fileにcall occurrence",
        observation: "callee form、caller file eval/with等を観測",
    },
    StageExpectation {
        wire: "c.caller",
        order: 2,
        prerequisite: "c.syntax",
        observation: "supported top-level callerを同定又は不能を記帳",
    },
    StageExpectation {
        wire: "c.import_form",
        order: 3,
        prerequisite: "callee識別子又はmember base識別子をsourceで特定",
        observation: "import種別/local重複/bare/namespaceを観測。dynamic_dispatchでも独立評価可",
    },
    StageExpectation {
        wire: "c.local_binding",
        order: 4,
        prerequisite: "bare identifier、importではないとsourceで判定",
        observation: "単一local callableを探索。名前未解決と非supported候補を区別して観測し、stageの観測内容は単一supported callableとの対応可否。理由の割当表はdescriptor外",
    },
    StageExpectation {
        wire: "c.specifier",
        order: 5,
        prerequisite: "static relative import bindingを識別済み",
        observation: "相対specifier検査。path不能ならexport段階を推測しない",
    },
    StageExpectation {
        wire: "c.candidates",
        order: 6,
        prerequisite: "正規化candidate集合を構成できる",
        observation: "全candidate/拒否witnessのentry・outcomeを評価",
    },
    StageExpectation {
        wire: "c.export_binding",
        order: 7,
        prerequisite: "一意included正常parsed候補、runtime named/default",
        observation: "export slot→元callable bindingを照合",
    },
    StageExpectation {
        wire: "c.shadow",
        order: 8,
        prerequisite: "caller subtreeと対象local/import名をsourceで特定",
        observation: "parameter/block/catch/destructuring等の同名bindingを調べる",
    },
    StageExpectation {
        wire: "c.writes",
        order: 9,
        prerequisite: "local/import/callee bindingのうち検査対象を特定",
        observation: "評価可能なbindingのwrite検査。未解決側は理由を捏造しない",
    },
    StageExpectation {
        wire: "c.resolution",
        order: 10,
        prerequisite: "実評価stageの結果とwitnessがある",
        observation: "2種のexact条件又は未解決をまとめる",
    },
];

struct BindingReasonExpectation {
    wire: &'static str,
    kind_ids: &'static [&'static str],
    required_stage: &'static str,
    source_condition: &'static str,
}

const DEFAULT_AND_NAMED: [&str; 2] = [
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
];
const ALL_IMPORT_FORMS: [&str; 4] = [
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
    "reviewgraphen.typescript.kind.binding.star_as_import@1",
    "reviewgraphen.typescript.kind.binding.type_only_import@1",
];
const DEFAULT_NAMED_AND_TYPE_ONLY: [&str; 3] = [
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
    "reviewgraphen.typescript.kind.binding.type_only_import@1",
];
const TYPE_ONLY: [&str; 1] = ["reviewgraphen.typescript.kind.binding.type_only_import@1"];

const EXPECTED_BINDING_REASONS: [BindingReasonExpectation; 12] = [
    BindingReasonExpectation {
        wire: "export_binding_unsupported",
        kind_ids: &DEFAULT_AND_NAMED,
        required_stage: "b.export_binding",
        source_condition: "一意正常calleeのexport slotが単一runtime callableでない",
    },
    BindingReasonExpectation {
        wire: "import_binding_ambiguous",
        kind_ids: &ALL_IMPORT_FORMS,
        required_stage: "b.local_uniqueness",
        source_condition: "同local import重複/local宣言競合",
    },
    BindingReasonExpectation {
        wire: "import_resolution_unavailable",
        kind_ids: &ALL_IMPORT_FORMS,
        required_stage: "b.form",
        source_condition: "bare/paths又はnamespace。type-onlyとの複合は評価できた時のみ",
    },
    BindingReasonExpectation {
        wire: "parse_failure",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "候補calleeがparse_failed",
    },
    BindingReasonExpectation {
        wire: "relative_specifier_unsupported",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.specifier",
        source_condition: "static相対specifierが規則外",
    },
    BindingReasonExpectation {
        wire: "relative_target_ambiguous",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "候補複数/実JS拒否witness",
    },
    BindingReasonExpectation {
        wire: "relative_target_excluded",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "existing候補のprofile除外",
    },
    BindingReasonExpectation {
        wire: "relative_target_missing",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "完全候補走査でTS/TSX候補0",
    },
    BindingReasonExpectation {
        wire: "relative_target_unread",
        kind_ids: &DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "走査で証明した未読/不完全/parse failure等",
    },
    BindingReasonExpectation {
        wire: "type_only_binding",
        kind_ids: &TYPE_ONLY,
        required_stage: "b.form",
        source_condition: "type-only importはruntime bindingにならない",
    },
    BindingReasonExpectation {
        wire: "unsupported_syntax",
        kind_ids: &DEFAULT_AND_NAMED,
        required_stage: "b.export_binding",
        source_condition: "一意parsed calleeの未知binding構文/eval/with。caller-only evalはbindingへ転写しない",
    },
    BindingReasonExpectation {
        wire: "written_binding",
        kind_ids: &DEFAULT_AND_NAMED,
        required_stage: "b.writes",
        source_condition: "import名又はcallee元bindingへのwrite。caller subtree shadowはbinding行に載せない",
    },
];

const EXPECTED_BINDING_PRECEDENCE: [&str; 12] = [
    "parse_failure",
    "unsupported_syntax",
    "relative_specifier_unsupported",
    "relative_target_unread",
    "relative_target_ambiguous",
    "relative_target_excluded",
    "relative_target_missing",
    "import_binding_ambiguous",
    "type_only_binding",
    "export_binding_unsupported",
    "import_resolution_unavailable",
    "written_binding",
];

#[test]
fn r2_definition_identity_and_binding_before_read_are_fixed() {
    let binding = typescript_registry_binding();
    let mut foreign = binding.clone();
    foreign.registry_id = "reviewgraphen.source_review_registry.foreign".into();

    assert_eq!(
        typescript_registry_definition_bytes(&foreign),
        Err(TypeScriptRegistryAccessError::ForeignBinding)
    );
    assert!(matches!(
        typescript_r2_policy_view(&foreign),
        Err(TypeScriptRegistryAccessError::ForeignBinding)
    ));

    let (bytes, view) = match (
        typescript_registry_definition_bytes(&binding),
        typescript_r2_policy_view(&binding),
    ) {
        (Ok(bytes), Ok(view)) => (bytes, view),
        (
            Err(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry),
            Err(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry),
        ) => panic!("both accessors rejected the correct binding as InvalidEmbeddedRegistry"),
        _ => panic!("raw-bytes and typed policy accessors diverged for the correct binding"),
    };

    assert_eq!(bytes.len(), EXPECTED_DEFINITION_LENGTH);
    assert_eq!(
        ContentHash::sha256(bytes).to_string(),
        EXPECTED_DEFINITION_SHA256
    );
    let parsed: serde_json::Value =
        serde_json::from_slice(bytes).expect("validated embedded bytes must be JSON");
    assert_eq!(parsed["registry_id"], EXPECTED_REGISTRY_ID);
    assert_eq!(
        parsed["arms"][0]["rule_set_definition"]["policy_tuple"]["domain_reason_policies"]["binding"]
            ["domain"],
        "binding"
    );
    let raw_binding_reason_rows = parsed["arms"][0]["rule_set_definition"]["policy_tuple"]
        ["domain_reason_policies"]["binding"]["allowed_reason_rows"]
        .as_array()
        .expect("validated embedded binding reason policy must be an array");
    assert_eq!(
        raw_binding_reason_rows.len(),
        EXPECTED_BINDING_REASONS.len()
    );
    for (raw, expected) in raw_binding_reason_rows.iter().zip(EXPECTED_BINDING_REASONS) {
        assert_eq!(raw["wire_literal"], expected.wire);
        assert_eq!(raw["target_domain"], "syntax");
        assert_eq!(raw["outcomes"], serde_json::json!(["unresolved"]));
    }

    assert_eq!(view.binding_stages().len(), EXPECTED_BINDING_STAGES.len());
    for (actual, expected) in view.binding_stages().iter().zip(EXPECTED_BINDING_STAGES) {
        assert_eq!(actual.stage().wire_literal(), expected.wire);
        assert_eq!(actual.order(), expected.order);
        assert_eq!(actual.prerequisite(), expected.prerequisite);
        assert_eq!(actual.observation(), expected.observation);
    }
    assert_eq!(view.call_stages().len(), EXPECTED_CALL_STAGES.len());
    for (actual, expected) in view.call_stages().iter().zip(EXPECTED_CALL_STAGES) {
        assert_eq!(actual.stage().wire_literal(), expected.wire);
        assert_eq!(actual.order(), expected.order);
        assert_eq!(actual.prerequisite(), expected.prerequisite);
        assert_eq!(actual.observation(), expected.observation);
    }
}

#[test]
fn r2_binding_reason_policy_and_raw_rejections_are_fixed() {
    let binding = typescript_registry_binding();
    let view = typescript_r2_policy_view(&binding)
        .expect("correct binding must reach the shared validated embedded registry state");

    assert_eq!(
        view.binding_reason_policy_id(),
        "typescript.binding_reason_precedence@1"
    );
    assert_eq!(
        view.binding_reason_rows().len(),
        EXPECTED_BINDING_REASONS.len()
    );
    for (actual, expected) in view
        .binding_reason_rows()
        .iter()
        .zip(EXPECTED_BINDING_REASONS)
    {
        assert_eq!(actual.reason().wire_literal(), expected.wire);
        assert_eq!(actual.kind_ids().collect::<Vec<_>>(), expected.kind_ids);
        assert_eq!(actual.outcome(), ResolutionOutcomeV1::Unresolved);
        assert_eq!(
            actual.required_stage().wire_literal(),
            expected.required_stage
        );
        assert_eq!(actual.source_condition(), expected.source_condition);
    }
    assert_eq!(
        view.binding_reason_precedence()
            .iter()
            .map(BindingReasonV1::wire_literal)
            .collect::<Vec<_>>(),
        EXPECTED_BINDING_PRECEDENCE
    );

    for rejected_wire in ["binding-stage\0foreign", "b.form@2"] {
        let error = BindingStageV1::parse_wire(&binding, rejected_wire)
            .expect_err("unknown Binding stage must be rejected");
        assert_eq!(error.rejected_wire.as_bytes(), rejected_wire.as_bytes());
    }
    for rejected_wire in ["binding-reason\0foreign", "binding_reason@2"] {
        let error = BindingReasonV1::parse_wire(&binding, rejected_wire)
            .expect_err("unknown Binding reason must be rejected");
        assert_eq!(error.rejected_wire.as_bytes(), rejected_wire.as_bytes());
    }
}
