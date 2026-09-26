//! Frozen `typescript.production.v1` registry tuple validation and r2 read seam.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use serde_json::{Map, Value};
use thiserror::Error;

use crate::{ContentHash, canonical_json};

use super::reasons::{BindingReasonV1, BindingStageV1, CallStageV1, ResolutionOutcomeV1};

pub const TYPESCRIPT_REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r2";
pub const TYPESCRIPT_REGISTRY_HASH: &str =
    "sha256:4e4aa59c25c1c43257eb946de2aa0a99ad0ac193ad21b72a64f2163a1ec99a51";
pub const TYPESCRIPT_ARM_ID: &str = "typescript.production.v1.source@1";
pub const TYPESCRIPT_ARM_HASH: &str =
    "sha256:92623e9272fe929694ea063a51350740fada0879ee7ba1368af53608447cee11";
pub const TYPESCRIPT_EXTRACTOR_SET_HASH: &str =
    "sha256:f5722ef19c0a2a4a5b6ff583f95aa8fdc9d0cf4f3cbc0c5631c041770842b39f";
pub const TYPESCRIPT_RULE_SET_HASH: &str =
    "sha256:bcce4c84557970e6bc52a519b873ddd3c3eed33d518f49418da101f8c496c1d8";

const EMBEDDED_R2_REGISTRY: &[u8] = include_bytes!("registry.r2.json");
const EXPECTED_R2_REGISTRY_LENGTH: usize = 131_272;
const EXPECTED_TUPLE_HASH: &str =
    "sha256:30b5b7ed971d9bad50164563cef086488781f36870e0c12863aa023bcf186a55";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeScriptRegistryTuple {
    pub profile_id: String,
    pub profile_version: String,
    pub language: String,
    pub producer_id: String,
    pub extractor_set_hash: String,
    pub rule_set_hash: String,
    pub projection_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeScriptRegistryBinding {
    pub registry_id: String,
    pub registry_hash: String,
    pub arm_id: String,
    pub arm_hash: String,
    pub tuple: TypeScriptRegistryTuple,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("TypeScript registry tuple is not the frozen typescript.production.v1 arm")]
pub struct RegistryError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Error)]
pub enum TypeScriptRegistryAccessError {
    #[error("foreign TypeScript registry binding")]
    ForeignBinding,
    #[error("embedded TypeScript r2 registry violates its frozen identity or shape")]
    InvalidEmbeddedRegistry,
}

pub struct BindingStagePolicyRowV1 {
    stage: BindingStageV1,
    order: u8,
    prerequisite: Box<str>,
    observation: Box<str>,
}

impl BindingStagePolicyRowV1 {
    #[must_use]
    pub fn stage(&self) -> &BindingStageV1 {
        &self.stage
    }
    #[must_use]
    pub fn order(&self) -> u8 {
        self.order
    }
    #[must_use]
    pub fn prerequisite(&self) -> &str {
        &self.prerequisite
    }
    #[must_use]
    pub fn observation(&self) -> &str {
        &self.observation
    }
}

pub struct CallStagePolicyRowV1 {
    stage: CallStageV1,
    order: u8,
    prerequisite: Box<str>,
    observation: Box<str>,
}

impl CallStagePolicyRowV1 {
    #[must_use]
    pub fn stage(&self) -> &CallStageV1 {
        &self.stage
    }
    #[must_use]
    pub fn order(&self) -> u8 {
        self.order
    }
    #[must_use]
    pub fn prerequisite(&self) -> &str {
        &self.prerequisite
    }
    #[must_use]
    pub fn observation(&self) -> &str {
        &self.observation
    }
}

pub struct BindingReasonPolicyRowV1 {
    reason: BindingReasonV1,
    kind_ids: Box<[Box<str>]>,
    outcome: ResolutionOutcomeV1,
    required_stage: BindingStageV1,
    source_condition: Box<str>,
}

impl BindingReasonPolicyRowV1 {
    #[must_use]
    pub fn reason(&self) -> &BindingReasonV1 {
        &self.reason
    }
    pub fn kind_ids(&self) -> impl ExactSizeIterator<Item = &str> + '_ {
        self.kind_ids.iter().map(Box::as_ref)
    }
    #[must_use]
    pub fn outcome(&self) -> ResolutionOutcomeV1 {
        self.outcome
    }
    #[must_use]
    pub fn required_stage(&self) -> &BindingStageV1 {
        &self.required_stage
    }
    #[must_use]
    pub fn source_condition(&self) -> &str {
        &self.source_condition
    }
}

pub struct TypeScriptR2PolicyViewV1 {
    binding_stages: Box<[BindingStagePolicyRowV1]>,
    call_stages: Box<[CallStagePolicyRowV1]>,
    binding_reason_policy_id: Box<str>,
    binding_reason_rows: Box<[BindingReasonPolicyRowV1]>,
    binding_reason_precedence: Box<[BindingReasonV1]>,
}

impl TypeScriptR2PolicyViewV1 {
    #[must_use]
    pub fn binding_stages(&self) -> &[BindingStagePolicyRowV1] {
        &self.binding_stages
    }
    #[must_use]
    pub fn call_stages(&self) -> &[CallStagePolicyRowV1] {
        &self.call_stages
    }
    #[must_use]
    pub fn binding_reason_policy_id(&self) -> &str {
        &self.binding_reason_policy_id
    }
    #[must_use]
    pub fn binding_reason_rows(&self) -> &[BindingReasonPolicyRowV1] {
        &self.binding_reason_rows
    }
    #[must_use]
    pub fn binding_reason_precedence(&self) -> &[BindingReasonV1] {
        &self.binding_reason_precedence
    }
}

#[must_use]
pub fn typescript_registry_binding() -> TypeScriptRegistryBinding {
    TypeScriptRegistryBinding {
        registry_id: TYPESCRIPT_REGISTRY_ID.into(),
        registry_hash: TYPESCRIPT_REGISTRY_HASH.into(),
        arm_id: TYPESCRIPT_ARM_ID.into(),
        arm_hash: TYPESCRIPT_ARM_HASH.into(),
        tuple: TypeScriptRegistryTuple {
            profile_id: "typescript.production.v1".into(),
            profile_version: "1".into(),
            language: "typescript".into(),
            producer_id: "reviewgraphen.ingest.typescript_tree_sitter@1".into(),
            extractor_set_hash: TYPESCRIPT_EXTRACTOR_SET_HASH.into(),
            rule_set_hash: TYPESCRIPT_RULE_SET_HASH.into(),
            projection_id: "typescript.obligation_report@1".into(),
        },
    }
}

pub fn validate_typescript_tuple(tuple: &TypeScriptRegistryTuple) -> Result<(), RegistryError> {
    if tuple == &typescript_registry_binding().tuple {
        Ok(())
    } else {
        Err(RegistryError)
    }
}

pub fn validate_typescript_binding(
    binding: &TypeScriptRegistryBinding,
) -> Result<(), RegistryError> {
    if binding == &typescript_registry_binding() {
        Ok(())
    } else {
        Err(RegistryError)
    }
}

static R2_POLICY_VIEW: OnceLock<Result<TypeScriptR2PolicyViewV1, TypeScriptRegistryAccessError>> =
    OnceLock::new();

pub fn typescript_registry_definition_bytes(
    binding: &TypeScriptRegistryBinding,
) -> Result<&'static [u8], TypeScriptRegistryAccessError> {
    validated_r2_policy_view(binding)?;
    Ok(EMBEDDED_R2_REGISTRY)
}

pub fn typescript_r2_policy_view(
    binding: &TypeScriptRegistryBinding,
) -> Result<&'static TypeScriptR2PolicyViewV1, TypeScriptRegistryAccessError> {
    validated_r2_policy_view(binding)
}

fn validated_r2_policy_view(
    binding: &TypeScriptRegistryBinding,
) -> Result<&'static TypeScriptR2PolicyViewV1, TypeScriptRegistryAccessError> {
    validate_typescript_binding(binding)
        .map_err(|_| TypeScriptRegistryAccessError::ForeignBinding)?;
    match R2_POLICY_VIEW.get_or_init(load_embedded_r2) {
        Ok(view) => Ok(view),
        Err(error) => Err(*error),
    }
}

struct ExpectedStage {
    wire: &'static str,
    order: u8,
    prerequisite: &'static str,
    observation: &'static str,
}

const EXPECTED_BINDING_STAGES: [ExpectedStage; 7] = [
    ExpectedStage {
        wire: "b.form",
        order: 1,
        prerequisite: "caller import sourceがparsed",
        observation: "import form/type-only/bare/namespaceとlocal名・slot観測",
    },
    ExpectedStage {
        wire: "b.local_uniqueness",
        order: 2,
        prerequisite: "b.form",
        observation: "同file local名の宣言/import競合を調べる",
    },
    ExpectedStage {
        wire: "b.specifier",
        order: 3,
        prerequisite: "b.formでstatic相対stringが得られる",
        observation: "相対specifierの文法/正規化。type-onlyも観測可能",
    },
    ExpectedStage {
        wire: "b.candidates",
        order: 4,
        prerequisite: "b.specifierで正規化候補pathを作れる",
        observation: "全candidate/rejection witnessを先に走査。失敗理由を全収集",
    },
    ExpectedStage {
        wire: "b.export_binding",
        order: 5,
        prerequisite: "一意included正常parsed候補、runtime named/default",
        observation: "callee export tableを単一runtime callableへ照合",
    },
    ExpectedStage {
        wire: "b.writes",
        order: 6,
        prerequisite: "調べるlocal又はcallee元bindingがsourceで特定済み",
        observation: "観測可能な対象bindingのwriteを確認。未特定側は推測しない",
    },
    ExpectedStage {
        wire: "b.result",
        order: 7,
        prerequisite: "それまでの実評価witnessがある",
        observation: "resolved/unresolvedとresolved_function_idをまとめる",
    },
];
const EXPECTED_CALL_STAGES: [ExpectedStage; 10] = [
    ExpectedStage {
        wire: "c.syntax",
        order: 1,
        prerequisite: "正常parsed caller fileにcall occurrence",
        observation: "callee form、caller file eval/with等を観測",
    },
    ExpectedStage {
        wire: "c.caller",
        order: 2,
        prerequisite: "c.syntax",
        observation: "supported top-level callerを同定又は不能を記帳",
    },
    ExpectedStage {
        wire: "c.import_form",
        order: 3,
        prerequisite: "callee識別子又はmember base識別子をsourceで特定",
        observation: "import種別/local重複/bare/namespaceを観測。dynamic_dispatchでも独立評価可",
    },
    ExpectedStage {
        wire: "c.local_binding",
        order: 4,
        prerequisite: "bare identifier、importではないとsourceで判定",
        observation: "単一local callableを探索。名前未解決と非supported候補を区別して観測し、stageの観測内容は単一supported callableとの対応可否。理由の割当表はdescriptor外",
    },
    ExpectedStage {
        wire: "c.specifier",
        order: 5,
        prerequisite: "static relative import bindingを識別済み",
        observation: "相対specifier検査。path不能ならexport段階を推測しない",
    },
    ExpectedStage {
        wire: "c.candidates",
        order: 6,
        prerequisite: "正規化candidate集合を構成できる",
        observation: "全candidate/拒否witnessのentry・outcomeを評価",
    },
    ExpectedStage {
        wire: "c.export_binding",
        order: 7,
        prerequisite: "一意included正常parsed候補、runtime named/default",
        observation: "export slot→元callable bindingを照合",
    },
    ExpectedStage {
        wire: "c.shadow",
        order: 8,
        prerequisite: "caller subtreeと対象local/import名をsourceで特定",
        observation: "parameter/block/catch/destructuring等の同名bindingを調べる",
    },
    ExpectedStage {
        wire: "c.writes",
        order: 9,
        prerequisite: "local/import/callee bindingのうち検査対象を特定",
        observation: "評価可能なbindingのwrite検査。未解決側は理由を捏造しない",
    },
    ExpectedStage {
        wire: "c.resolution",
        order: 10,
        prerequisite: "実評価stageの結果とwitnessがある",
        observation: "2種のexact条件又は未解決をまとめる",
    },
];

const DEFAULT_AND_NAMED: &[&str] = &[
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
];
const ALL_IMPORT_FORMS: &[&str] = &[
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
    "reviewgraphen.typescript.kind.binding.star_as_import@1",
    "reviewgraphen.typescript.kind.binding.type_only_import@1",
];
const DEFAULT_NAMED_AND_TYPE_ONLY: &[&str] = &[
    "reviewgraphen.typescript.kind.binding.default_import@1",
    "reviewgraphen.typescript.kind.binding.named_import@1",
    "reviewgraphen.typescript.kind.binding.type_only_import@1",
];
const TYPE_ONLY: &[&str] = &["reviewgraphen.typescript.kind.binding.type_only_import@1"];
struct ExpectedBindingReason {
    wire: &'static str,
    kind_ids: &'static [&'static str],
    required_stage: &'static str,
    source_condition: &'static str,
}
const EXPECTED_BINDING_REASONS: [ExpectedBindingReason; 12] = [
    ExpectedBindingReason {
        wire: "export_binding_unsupported",
        kind_ids: DEFAULT_AND_NAMED,
        required_stage: "b.export_binding",
        source_condition: "一意正常calleeのexport slotが単一runtime callableでない",
    },
    ExpectedBindingReason {
        wire: "import_binding_ambiguous",
        kind_ids: ALL_IMPORT_FORMS,
        required_stage: "b.local_uniqueness",
        source_condition: "同local import重複/local宣言競合",
    },
    ExpectedBindingReason {
        wire: "import_resolution_unavailable",
        kind_ids: ALL_IMPORT_FORMS,
        required_stage: "b.form",
        source_condition: "bare/paths又はnamespace。type-onlyとの複合は評価できた時のみ",
    },
    ExpectedBindingReason {
        wire: "parse_failure",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "候補calleeがparse_failed",
    },
    ExpectedBindingReason {
        wire: "relative_specifier_unsupported",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.specifier",
        source_condition: "static相対specifierが規則外",
    },
    ExpectedBindingReason {
        wire: "relative_target_ambiguous",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "候補複数/実JS拒否witness",
    },
    ExpectedBindingReason {
        wire: "relative_target_excluded",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "existing候補のprofile除外",
    },
    ExpectedBindingReason {
        wire: "relative_target_missing",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "完全候補走査でTS/TSX候補0",
    },
    ExpectedBindingReason {
        wire: "relative_target_unread",
        kind_ids: DEFAULT_NAMED_AND_TYPE_ONLY,
        required_stage: "b.candidates",
        source_condition: "走査で証明した未読/不完全/parse failure等",
    },
    ExpectedBindingReason {
        wire: "type_only_binding",
        kind_ids: TYPE_ONLY,
        required_stage: "b.form",
        source_condition: "type-only importはruntime bindingにならない",
    },
    ExpectedBindingReason {
        wire: "unsupported_syntax",
        kind_ids: DEFAULT_AND_NAMED,
        required_stage: "b.export_binding",
        source_condition: "一意parsed calleeの未知binding構文/eval/with。caller-only evalはbindingへ転写しない",
    },
    ExpectedBindingReason {
        wire: "written_binding",
        kind_ids: DEFAULT_AND_NAMED,
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

fn load_embedded_r2() -> Result<TypeScriptR2PolicyViewV1, TypeScriptRegistryAccessError> {
    if EMBEDDED_R2_REGISTRY.len() != EXPECTED_R2_REGISTRY_LENGTH
        || ContentHash::sha256(EMBEDDED_R2_REGISTRY).to_string() != TYPESCRIPT_REGISTRY_HASH
    {
        return invalid();
    }
    let root: Value = serde_json::from_slice(EMBEDDED_R2_REGISTRY)
        .map_err(|_| TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
    let root_object = object(&root)?;
    if !has_exact_fields(root_object, &["arms", "format", "registry_id"])
        || string_field(root_object, "format")? != "reviewgraphen.source_review_registry@1"
        || string_field(root_object, "registry_id")? != TYPESCRIPT_REGISTRY_ID
    {
        return invalid();
    }
    let arms = array_field(root_object, "arms")?;
    let [arm_value] = arms.as_slice() else {
        return invalid();
    };
    if !canonical_hash_matches(arm_value, TYPESCRIPT_ARM_HASH) {
        return invalid();
    }
    let arm = object(arm_value)?;
    if string_field(arm, "arm_id")? != TYPESCRIPT_ARM_ID
        || string_field(arm, "extractor_set_hash")? != TYPESCRIPT_EXTRACTOR_SET_HASH
        || string_field(arm, "rule_set_hash")? != TYPESCRIPT_RULE_SET_HASH
    {
        return invalid();
    }
    validate_embedded_tuple(value_field(arm, "tuple")?)?;
    let rule_set = object(value_field(arm, "rule_set_definition")?)?;
    let policy_tuple = object(value_field(rule_set, "policy_tuple")?)?;
    let binding_stages = load_binding_stages(policy_tuple)?;
    let call_stages = load_call_stages(policy_tuple)?;
    let (binding_reason_policy_id, binding_reason_rows, binding_reason_precedence) =
        load_binding_reason_policy(policy_tuple)?;
    Ok(TypeScriptR2PolicyViewV1 {
        binding_stages,
        call_stages,
        binding_reason_policy_id,
        binding_reason_rows,
        binding_reason_precedence,
    })
}

fn invalid<T>() -> Result<T, TypeScriptRegistryAccessError> {
    Err(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)
}
fn object(value: &Value) -> Result<&Map<String, Value>, TypeScriptRegistryAccessError> {
    value
        .as_object()
        .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)
}
fn value_field<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Value, TypeScriptRegistryAccessError> {
    object
        .get(name)
        .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)
}
fn string_field<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a str, TypeScriptRegistryAccessError> {
    value_field(object, name)?
        .as_str()
        .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)
}
fn array_field<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Vec<Value>, TypeScriptRegistryAccessError> {
    value_field(object, name)?
        .as_array()
        .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)
}
fn has_exact_fields(object: &Map<String, Value>, expected: &[&str]) -> bool {
    object.len() == expected.len() && expected.iter().all(|field| object.contains_key(*field))
}
fn canonical_hash_matches(value: &Value, expected: &str) -> bool {
    canonical_json(value)
        .ok()
        .is_some_and(|bytes| ContentHash::sha256(&bytes).to_string() == expected)
}

fn validate_embedded_tuple(tuple_value: &Value) -> Result<(), TypeScriptRegistryAccessError> {
    if !canonical_hash_matches(tuple_value, EXPECTED_TUPLE_HASH) {
        return invalid();
    }
    let tuple = object(tuple_value)?;
    if !has_exact_fields(
        tuple,
        &[
            "extractor_set_hash",
            "language",
            "producer_id",
            "profile_id",
            "profile_version",
            "projection_id",
            "rule_set_hash",
        ],
    ) {
        return invalid();
    }
    let binding = typescript_registry_binding();
    if string_field(tuple, "profile_id")? != binding.tuple.profile_id
        || string_field(tuple, "profile_version")? != binding.tuple.profile_version
        || string_field(tuple, "language")? != binding.tuple.language
        || string_field(tuple, "producer_id")? != binding.tuple.producer_id
        || string_field(tuple, "extractor_set_hash")? != binding.tuple.extractor_set_hash
        || string_field(tuple, "rule_set_hash")? != binding.tuple.rule_set_hash
        || string_field(tuple, "projection_id")? != binding.tuple.projection_id
    {
        return invalid();
    }
    Ok(())
}

fn stage_rows_for_domain<'a>(
    policy_tuple: &'a Map<String, Value>,
    domain: &str,
) -> Result<Vec<&'a Map<String, Value>>, TypeScriptRegistryAccessError> {
    let mut rows = Vec::new();
    for value in array_field(policy_tuple, "stage_policies")? {
        let row = object(value)?;
        if string_field(row, "domain")? == domain {
            rows.push(row);
        }
    }
    Ok(rows)
}
fn validate_stage_row(
    row: &Map<String, Value>,
    domain: &str,
    expected: &ExpectedStage,
    seen_wires: &mut BTreeSet<String>,
    seen_orders: &mut BTreeSet<u8>,
) -> Result<(String, u8, String, String), TypeScriptRegistryAccessError> {
    if !has_exact_fields(
        row,
        &["domain", "name", "observation", "order", "prerequisite"],
    ) || string_field(row, "domain")? != domain
    {
        return invalid();
    }
    let wire = string_field(row, "name")?;
    let order = u8::try_from(
        value_field(row, "order")?
            .as_u64()
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?,
    )
    .map_err(|_| TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
    let prerequisite = string_field(row, "prerequisite")?;
    let observation = string_field(row, "observation")?;
    if wire != expected.wire
        || order != expected.order
        || prerequisite != expected.prerequisite
        || observation != expected.observation
        || prerequisite.is_empty()
        || observation.is_empty()
        || !seen_wires.insert(wire.to_owned())
        || !seen_orders.insert(order)
    {
        return invalid();
    }
    Ok((
        wire.to_owned(),
        order,
        prerequisite.to_owned(),
        observation.to_owned(),
    ))
}
fn load_binding_stages(
    policy_tuple: &Map<String, Value>,
) -> Result<Box<[BindingStagePolicyRowV1]>, TypeScriptRegistryAccessError> {
    let rows = stage_rows_for_domain(policy_tuple, "binding")?;
    if rows.len() != EXPECTED_BINDING_STAGES.len() {
        return invalid();
    }
    let (mut seen_wires, mut seen_orders, mut validated) = (
        BTreeSet::new(),
        BTreeSet::new(),
        Vec::with_capacity(rows.len()),
    );
    for (row, expected) in rows.iter().zip(EXPECTED_BINDING_STAGES.iter()) {
        let (wire, order, prerequisite, observation) =
            validate_stage_row(row, "binding", expected, &mut seen_wires, &mut seen_orders)?;
        let stage = BindingStageV1::from_validated_wire(&wire)
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
        validated.push(BindingStagePolicyRowV1 {
            stage,
            order,
            prerequisite: prerequisite.into_boxed_str(),
            observation: observation.into_boxed_str(),
        });
    }
    Ok(validated.into_boxed_slice())
}
fn load_call_stages(
    policy_tuple: &Map<String, Value>,
) -> Result<Box<[CallStagePolicyRowV1]>, TypeScriptRegistryAccessError> {
    let rows = stage_rows_for_domain(policy_tuple, "call")?;
    if rows.len() != EXPECTED_CALL_STAGES.len() {
        return invalid();
    }
    let (mut seen_wires, mut seen_orders, mut validated) = (
        BTreeSet::new(),
        BTreeSet::new(),
        Vec::with_capacity(rows.len()),
    );
    for (row, expected) in rows.iter().zip(EXPECTED_CALL_STAGES.iter()) {
        let (wire, order, prerequisite, observation) =
            validate_stage_row(row, "call", expected, &mut seen_wires, &mut seen_orders)?;
        let stage = CallStageV1::from_validated_wire(&wire)
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
        validated.push(CallStagePolicyRowV1 {
            stage,
            order,
            prerequisite: prerequisite.into_boxed_str(),
            observation: observation.into_boxed_str(),
        });
    }
    Ok(validated.into_boxed_slice())
}

/// Policy ID, allowed rows and precedence of the binding reason policy.
type BindingReasonPolicy = (
    Box<str>,
    Box<[BindingReasonPolicyRowV1]>,
    Box<[BindingReasonV1]>,
);

fn load_binding_reason_policy(
    policy_tuple: &Map<String, Value>,
) -> Result<BindingReasonPolicy, TypeScriptRegistryAccessError> {
    let domain_policies = object(value_field(policy_tuple, "domain_reason_policies")?)?;
    let binding = object(value_field(domain_policies, "binding")?)?;
    if !has_exact_fields(
        binding,
        &["allowed_reason_rows", "domain", "policy_id", "precedence"],
    ) || string_field(binding, "domain")? != "binding"
        || string_field(binding, "policy_id")? != "typescript.binding_reason_precedence@1"
    {
        return invalid();
    }
    let raw_rows = array_field(binding, "allowed_reason_rows")?;
    if raw_rows.len() != EXPECTED_BINDING_REASONS.len() {
        return invalid();
    }
    let mut seen_reasons = BTreeSet::new();
    let mut rows = Vec::with_capacity(raw_rows.len());
    for (raw, expected) in raw_rows.iter().zip(EXPECTED_BINDING_REASONS.iter()) {
        let row = object(raw)?;
        if !has_exact_fields(
            row,
            &[
                "kind_ids",
                "outcomes",
                "required_stage_expression",
                "source_condition",
                "target_domain",
                "wire_literal",
            ],
        ) || string_field(row, "wire_literal")? != expected.wire
            || string_field(row, "target_domain")? != "syntax"
            || string_field(row, "required_stage_expression")? != expected.required_stage
            || string_field(row, "source_condition")? != expected.source_condition
            || string_field(row, "source_condition")?.is_empty()
            || !seen_reasons.insert(string_field(row, "wire_literal")?.to_owned())
        {
            return invalid();
        }
        let raw_kinds = array_field(row, "kind_ids")?;
        if raw_kinds.is_empty() || raw_kinds.len() != expected.kind_ids.len() {
            return invalid();
        }
        let mut seen_kinds = BTreeSet::new();
        let mut kind_ids = Vec::with_capacity(raw_kinds.len());
        for (raw_kind, expected_kind) in raw_kinds.iter().zip(expected.kind_ids.iter()) {
            let kind = raw_kind
                .as_str()
                .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
            if kind.is_empty() || kind != *expected_kind || !seen_kinds.insert(kind.to_owned()) {
                return invalid();
            }
            kind_ids.push(kind.to_owned().into_boxed_str());
        }
        let outcomes = array_field(row, "outcomes")?;
        if outcomes.len() != 1 || outcomes[0].as_str() != Some("unresolved") {
            return invalid();
        }
        let reason = BindingReasonV1::from_validated_wire(expected.wire)
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
        let required_stage = BindingStageV1::from_validated_wire(expected.required_stage)
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
        rows.push(BindingReasonPolicyRowV1 {
            reason,
            kind_ids: kind_ids.into_boxed_slice(),
            outcome: ResolutionOutcomeV1::Unresolved,
            required_stage,
            source_condition: expected.source_condition.to_owned().into_boxed_str(),
        });
    }
    let raw_precedence = array_field(binding, "precedence")?;
    if raw_precedence.len() != EXPECTED_BINDING_PRECEDENCE.len() {
        return invalid();
    }
    let mut seen_precedence = BTreeSet::new();
    let mut precedence = Vec::with_capacity(raw_precedence.len());
    for (raw, expected) in raw_precedence
        .iter()
        .zip(EXPECTED_BINDING_PRECEDENCE.iter())
    {
        let wire = raw
            .as_str()
            .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?;
        if wire != *expected
            || !seen_precedence.insert(wire.to_owned())
            || !seen_reasons.contains(wire)
        {
            return invalid();
        }
        precedence.push(
            BindingReasonV1::from_validated_wire(wire)
                .ok_or(TypeScriptRegistryAccessError::InvalidEmbeddedRegistry)?,
        );
    }
    if seen_precedence != seen_reasons {
        return invalid();
    }
    Ok((
        string_field(binding, "policy_id")?
            .to_owned()
            .into_boxed_str(),
        rows.into_boxed_slice(),
        precedence.into_boxed_slice(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const FROZEN_R1_REGISTRY_JSON: &str =
        include_str!("../../../reviewgraphen-cli/tests/fixtures/typescript-v1/registry.r1.json");
    const FROZEN_R2_REGISTRY_HASHES: &str = include_str!(
        "../../../reviewgraphen-cli/tests/fixtures/typescript-v1/registry.r2.hashes.json"
    );
    const CURRENT_R2_REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r2";
    const FROZEN_R1_REGISTRY_ID: &str = "reviewgraphen.source_review_registry.r1";

    #[test]
    fn production_registry_constants_match_the_frozen_r2_fixture() {
        assert_eq!(TYPESCRIPT_REGISTRY_ID, CURRENT_R2_REGISTRY_ID);
        for literal in [
            TYPESCRIPT_REGISTRY_HASH,
            TYPESCRIPT_ARM_HASH,
            TYPESCRIPT_EXTRACTOR_SET_HASH,
            TYPESCRIPT_RULE_SET_HASH,
        ] {
            assert!(
                FROZEN_R2_REGISTRY_HASHES.contains(literal),
                "frozen hash fixture does not carry {literal}"
            );
        }
        assert!(
            FROZEN_R1_REGISTRY_JSON.contains(FROZEN_R1_REGISTRY_ID),
            "frozen r1 registry fixture does not carry its historical registry ID"
        );
        // The arm ID is intentionally shared by r1 and r2, so r1 remains a historical control.
        assert!(
            FROZEN_R1_REGISTRY_JSON.contains(TYPESCRIPT_ARM_ID),
            "frozen r1 registry fixture does not carry the shared historical arm ID"
        );
    }
}
