use crate::program::{attribute_bool, attribute_string};
use crate::review::ObligationParts;
use crate::{
    CanonicalJson, CapabilityState, ContentHash, DomainError, IdRegistry, Obligation, ProgramSpace,
    Result, StableId, VersionTuple,
};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const CAPABILITY_GAP_RULE: &str = "capability_gap.origin_rule@1";
const CAPABILITY_GAP_PROPERTY: &str = "reviewgraphen.capability_gap";

/// Fixed descriptor for one M1 deterministic rule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RuleDescriptor {
    /// Stable rule ID and version.
    pub id: &'static str,
    /// Target category produced by the rule.
    pub target_kind: &'static str,
    /// Property covered by the rule.
    pub property_id: &'static str,
}

/// A transparent, retained reason a candidate target is outside the denominator.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExclusionRecord {
    /// Stable exclusion record ID.
    pub id: StableId,
    /// Rule candidate semantic key.
    pub candidate_key: String,
    /// Explicit policy or profile reason.
    pub reason: String,
    /// Source records affected by the exclusion.
    pub source_ids: BTreeSet<StableId>,
    /// Weight not included in eligible weighted coverage.
    pub excluded_weight: f64,
}

/// Versioned, explicit coverage denominator for one synthesized universe.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UniverseDescriptor {
    /// Deterministic universe ID.
    id: StableId,
    /// Snapshot the universe describes.
    snapshot_id: StableId,
    /// Normalized profile identity.
    profile_id: String,
    /// Rule pack hash supplied by ProgramSpace.
    rule_set_hash: ContentHash,
    /// Extractor/adaptor set hash.
    extractor_set_hash: ContentHash,
    /// Policy version supplied by ProgramSpace.
    policy_version: String,
    /// M1 rule pack implementation version.
    rule_pack_version: String,
    /// Eligible obligation IDs: the sole raw coverage denominator.
    pub(crate) obligation_ids: BTreeSet<StableId>,
    /// Explicit records excluded from that denominator.
    exclusions: Vec<ExclusionRecord>,
    /// Extraction limitations that qualify the universe.
    limitation_ids: BTreeSet<StableId>,
}

impl UniverseDescriptor {
    pub(crate) fn allocated_bytes(&self) -> usize {
        fn id_set(values: &BTreeSet<StableId>) -> usize {
            values
                .len()
                .saturating_mul(std::mem::size_of::<StableId>())
                .saturating_add(values.iter().map(StableId::allocated_bytes).sum::<usize>())
        }
        let exclusions = self.exclusions.iter().fold(
            self.exclusions
                .capacity()
                .saturating_mul(std::mem::size_of::<ExclusionRecord>()),
            |total, value| {
                total
                    .saturating_add(value.id.allocated_bytes())
                    .saturating_add(value.candidate_key.capacity())
                    .saturating_add(value.reason.capacity())
                    .saturating_add(id_set(&value.source_ids))
            },
        );
        self.id
            .allocated_bytes()
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.profile_id.capacity())
            .saturating_add(self.rule_set_hash.allocated_bytes())
            .saturating_add(self.extractor_set_hash.allocated_bytes())
            .saturating_add(self.policy_version.capacity())
            .saturating_add(self.rule_pack_version.capacity())
            .saturating_add(id_set(&self.obligation_ids))
            .saturating_add(exclusions)
            .saturating_add(id_set(&self.limitation_ids))
    }

    /// Deterministic universe identity.
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }

    /// Snapshot described by this denominator.
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }

    /// Normalized profile tuple recorded by this universe.
    #[must_use]
    pub fn profile_id(&self) -> &str {
        &self.profile_id
    }

    /// Rule-set hash bound into the universe.
    #[must_use]
    pub fn rule_set_hash(&self) -> &ContentHash {
        &self.rule_set_hash
    }

    /// Extractor-set hash bound into the universe.
    #[must_use]
    pub fn extractor_set_hash(&self) -> &ContentHash {
        &self.extractor_set_hash
    }

    /// Policy version bound into the universe.
    #[must_use]
    pub fn policy_version(&self) -> &str {
        &self.policy_version
    }

    /// Version of the deterministic rule-pack implementation that synthesized
    /// this explicit coverage denominator.
    #[must_use]
    pub fn rule_pack_version(&self) -> &str {
        &self.rule_pack_version
    }

    /// Explicit records excluded from the eligible denominator.
    #[must_use]
    pub fn exclusions(&self) -> &[ExclusionRecord] {
        &self.exclusions
    }

    /// Extraction limitations that qualify this universe.
    #[must_use]
    pub fn limitation_ids(&self) -> &BTreeSet<StableId> {
        &self.limitation_ids
    }
    /// Eligible raw obligation denominator.
    #[must_use]
    pub fn raw_denominator(&self) -> usize {
        self.obligation_ids.len()
    }

    /// Eligible IDs in deterministic order.
    #[must_use]
    pub fn obligation_ids(&self) -> &BTreeSet<StableId> {
        &self.obligation_ids
    }

    /// The total explicit excluded weight, kept outside eligible coverage.
    #[must_use]
    pub fn excluded_weight(&self) -> f64 {
        self.exclusions
            .iter()
            .map(|item| item.excluded_weight)
            .sum()
    }

    pub(crate) fn validate_against(
        &self,
        program: &ProgramSpace,
        obligations: &BTreeMap<StableId, Obligation>,
    ) -> Result<()> {
        let expected_obligation_ids = obligations.keys().cloned().collect::<BTreeSet<_>>();
        if self.obligation_ids != expected_obligation_ids {
            return Err(DomainError::Validation(
                "universe denominator IDs do not match obligation records".to_owned(),
            ));
        }
        if self.snapshot_id != *program.snapshot_id()
            || self.profile_id != program.profile_key()
            || self.rule_set_hash != *program.rule_set_hash()
            || self.extractor_set_hash != *program.extractor_set_hash()
            || self.policy_version != program.policy_version()
            || self.rule_pack_version != "m1.fixture@1"
        {
            return Err(DomainError::Validation(
                "universe identity and version tuple must match ProgramSpace".to_owned(),
            ));
        }
        let program_ids = program.known_ids();
        let mut limitation_ids = program
            .extraction()
            .limitations
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        limitation_ids.extend(
            obligations
                .values()
                .flat_map(|obligation| obligation.qualification_ids().iter().cloned()),
        );
        if self.limitation_ids != limitation_ids {
            return Err(DomainError::Validation(
                "universe limitation trace must match ProgramSpace extraction".to_owned(),
            ));
        }
        let expected_id = universe_id(
            program,
            &self.obligation_ids,
            &self.exclusions,
            &self.limitation_ids,
        )?;
        if self.id != expected_id {
            return Err(DomainError::Validation(
                "universe ID must bind its explicit denominator and qualification inputs"
                    .to_owned(),
            ));
        }
        if self
            .exclusions
            .windows(2)
            .any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(DomainError::Validation(
                "universe exclusions must be in deterministic ID order".to_owned(),
            ));
        }
        for exclusion in &self.exclusions {
            if !exclusion.excluded_weight.is_finite()
                || exclusion.excluded_weight <= 0.0
                || exclusion.reason.is_empty()
                || exclusion.source_ids.is_empty()
            {
                return Err(DomainError::Validation(
                    "universe exclusions require reason, source trace, and positive weight"
                        .to_owned(),
                ));
            }
            for source in &exclusion.source_ids {
                if !program_ids.contains(source) {
                    return Err(DomainError::DanglingReference {
                        owner: "universe exclusion",
                        owner_id: exclusion.id.clone(),
                        reference: source.clone(),
                    });
                }
            }
            let expected_id = StableId::derived(
                "exclusion",
                &BTreeMap::from([
                    (
                        "candidate".to_owned(),
                        Value::String(exclusion.candidate_key.clone()),
                    ),
                    (
                        "snapshot".to_owned(),
                        Value::String(program.snapshot_id().to_string()),
                    ),
                ]),
            )?;
            if exclusion.id != expected_id {
                return Err(DomainError::Validation(
                    "universe exclusion ID must bind its candidate and snapshot".to_owned(),
                ));
            }
        }
        for obligation in obligations.values() {
            if obligation.version().snapshot() != program.snapshot_id()
                || obligation.version().profile() != program.profile_key()
            {
                return Err(DomainError::Validation(
                    "universe obligations must retain the ProgramSpace version tuple".to_owned(),
                ));
            }
        }
        Ok(())
    }
}

/// The schema-compatible, canonical external obligation-bundle contract.
///
/// It intentionally contains no internal aggregate-only fields. Exclusions are
/// retained because they qualify the public coverage denominator. Its JSON
/// shape validates against `schemas/reviewgraphen.obligation.schema.json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ObligationContract {
    schema: &'static str,
    source: ContractSource,
    universe: ContractUniverse,
    obligations: Vec<ContractObligation>,
    #[serde(skip)]
    canonical: CanonicalJson,
}

impl ObligationContract {
    fn new(
        source: ContractSource,
        universe: ContractUniverse,
        obligations: Vec<ContractObligation>,
    ) -> Result<Self> {
        let placeholder = CanonicalJson::from_serializable(&Value::Null)?;
        let mut contract = Self {
            schema: "reviewgraphen.review_obligations.v2",
            source,
            universe,
            obligations,
            canonical: placeholder,
        };
        contract.canonical = CanonicalJson::from_serializable(&contract)?;
        Ok(contract)
    }

    /// Byte-stable canonical JSON for this exact schema contract.
    #[must_use]
    pub fn canonical(&self) -> &CanonicalJson {
        &self.canonical
    }

    /// Schema contract name.
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        self.schema
    }
}

/// Canonical result of a deterministic M1 synthesis run.
///
/// Serializing this type emits its [`ObligationContract`], never the internal
/// aggregate shape. That prevents accidental schema drift at the public boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ObligationBundle {
    universe: UniverseDescriptor,
    obligations: Vec<Obligation>,
    contract: ObligationContract,
}

impl Serialize for ObligationBundle {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.contract.serialize(serializer)
    }
}

impl ObligationBundle {
    /// Coverage universe retained by the in-memory aggregate.
    #[must_use]
    pub fn universe(&self) -> &UniverseDescriptor {
        &self.universe
    }

    /// Obligations in stable ID order.
    #[must_use]
    pub fn obligations(&self) -> &[Obligation] {
        &self.obligations
    }

    /// Explicit schema-compatible contract output.
    #[must_use]
    pub fn contract(&self) -> &ObligationContract {
        &self.contract
    }

    /// SHA-256 hash of the immutable schema contract bytes.
    #[must_use]
    pub fn canonical_hash(&self) -> &ContentHash {
        self.contract.canonical().hash()
    }

    /// Splits the internal bundle for construction of the event-sourced aggregate.
    #[must_use]
    pub fn into_parts(self) -> (UniverseDescriptor, Vec<Obligation>) {
        (self.universe, self.obligations)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractSource {
    kind: &'static str,
    locator: &'static str,
    revision: &'static str,
    content_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractUniverse {
    id: StableId,
    snapshot_id: StableId,
    profile_id: String,
    rule_set_hash: ContentHash,
    extractor_set_hash: ContentHash,
    policy_version: String,
    obligation_ids: Vec<StableId>,
    limitation_ids: Vec<StableId>,
    exclusions: Vec<ContractExclusion>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractExclusion {
    id: StableId,
    candidate_key: String,
    reason: String,
    source_ids: Vec<StableId>,
    excluded_weight: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractObligation {
    id: StableId,
    target: ContractTarget,
    property: ContractProperty,
    context_requirement: ContractContextRequirement,
    evidence_requirement: ContractEvidenceRequirement,
    risk: ContractRisk,
    applicability: ContractApplicability,
    version: VersionTuple,
    provenance: ContractProvenance,
    lifecycle: &'static str,
    depends_on_obligation_ids: Vec<StableId>,
    source_ids: Vec<StableId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractTarget {
    kind: String,
    refs: Vec<StableId>,
    semantic_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractProperty {
    id: String,
    version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractContextRequirement {
    context_ids: Vec<StableId>,
    required_capabilities: Vec<String>,
    include_relation_kinds: Vec<String>,
    max_relation_depth: u64,
    include_tests: bool,
    include_existing_evidence: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractEvidenceRequirement {
    required: bool,
    accepted_modes: Vec<String>,
    minimum_count: u64,
    policy: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractRisk {
    impact: &'static str,
    exposure: f64,
    uncertainty: f64,
    structural_reach: f64,
    weight: f64,
    rationale: &'static str,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractApplicability {
    status: String,
    reasons: Vec<String>,
    qualification_ids: Vec<StableId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct ContractProvenance {
    source: ContractSource,
    extraction_method: String,
    tool_version: &'static str,
    confidence: f64,
    review_status: &'static str,
    generator_ids: Vec<StableId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin_rule: Option<String>,
}

/// The intentionally narrow M1 rule pack for the double-submit ProgramSpace.
pub struct MvpRulePack;

impl MvpRulePack {
    /// Rules actually implemented by M1. Test execution and context-envelope
    /// rules are later milestones and are not represented as false obligations.
    #[must_use]
    pub const fn rules() -> &'static [RuleDescriptor] {
        &[
            RuleDescriptor {
                id: "node.changed_public_symbol@1",
                target_kind: "node",
                property_id: "async.concurrent_reentry",
            },
            RuleDescriptor {
                id: "relation.concurrent_reentry@1",
                target_kind: "relation",
                property_id: "async.concurrent_reentry",
            },
            RuleDescriptor {
                id: "relation.changed_call_contract@1",
                target_kind: "relation",
                property_id: "payment.idempotency_contract",
            },
            RuleDescriptor {
                id: "path.external_side_effect@1",
                target_kind: "path",
                property_id: "payment.at_most_once",
            },
            RuleDescriptor {
                id: "invariant.payment_at_most_once@1",
                target_kind: "invariant",
                property_id: "payment.at_most_once",
            },
            RuleDescriptor {
                id: CAPABILITY_GAP_RULE,
                target_kind: "subgraph",
                property_id: CAPABILITY_GAP_PROPERTY,
            },
        ]
    }

    /// Synthesizes the supported M1 slice with stable IDs and stable ordering.
    pub fn synthesize(program: &ProgramSpace) -> Result<ObligationBundle> {
        if program.profile_key() != "code-review@1"
            && program.profile_key() != crate::m5::DOUBLE_SUBMIT_PROFILE_ID
        {
            return Err(DomainError::Validation(
                "M1 manual adapter supports only the code-review@1 or fixed M5 fixture profile"
                    .to_owned(),
            ));
        }

        let mut exclusions = Vec::new();
        let mut obligations = Vec::new();
        let mut node_ids = BTreeMap::<StableId, StableId>::new();

        for artifact in program.artifacts() {
            if artifact.kind != "function"
                || !is_changed_public_symbol(program, &artifact.id)
                || !attribute_bool(&artifact.attributes, "public")
            {
                continue;
            }
            if let Some(reason) = attribute_string(&artifact.attributes, "reviewgraphen_excluded") {
                exclusions.push(exclusion(
                    program,
                    "node.changed_public_symbol@1",
                    artifact.id.clone(),
                    reason,
                    3.0,
                )?);
                continue;
            }
            let target_refs = vec![artifact.id.clone()];
            let node_obligation = materialize(
                program,
                ObligationSpec {
                    rule: "node.changed_public_symbol@1",
                    origin_rule: None,
                    target_kind: "node",
                    target_refs: target_refs.clone(),
                    property_id: "async.concurrent_reentry",
                    context_ids: contexts_for_node(program, &artifact.id),
                    required_capabilities: BTreeSet::from([
                        "ast".to_owned(),
                        "concurrency_model".to_owned(),
                    ]),
                    weight: 3.0,
                    depends_on: Vec::new(),
                    generator_ids: BTreeSet::from([artifact.id.clone()]),
                    additional_source_ids: Vec::new(),
                },
            )?;
            node_ids.insert(artifact.id.clone(), node_obligation.id().clone());
            obligations.push(node_obligation);
        }

        let mut reentry_ids = BTreeMap::<StableId, StableId>::new();
        for relation in program.relations() {
            if relation.kind != "handled_by"
                || attribute_string(&relation.attributes, "concurrency")
                    != Some("unbounded_reentry")
            {
                continue;
            }
            let dependencies = relation
                .target_ids
                .iter()
                .filter_map(|target| node_ids.get(target).cloned())
                .collect::<Vec<_>>();
            let target_refs = vec![relation.id.clone()];
            let reentry_obligation = materialize(
                program,
                ObligationSpec {
                    rule: "relation.concurrent_reentry@1",
                    origin_rule: None,
                    target_kind: "relation",
                    target_refs: target_refs.clone(),
                    property_id: "async.concurrent_reentry",
                    context_ids: contexts_containing_id(program, &relation.id),
                    required_capabilities: BTreeSet::from([
                        "direct_calls".to_owned(),
                        "concurrency_model".to_owned(),
                    ]),
                    weight: 3.5,
                    depends_on: dependencies,
                    generator_ids: BTreeSet::from([relation.id.clone()]),
                    additional_source_ids: Vec::new(),
                },
            )?;
            reentry_ids.insert(relation.id.clone(), reentry_obligation.id().clone());
            obligations.push(reentry_obligation);
        }

        let mut payment_contract_ids = BTreeMap::<StableId, StableId>::new();
        for relation in program.relations() {
            if relation.kind != "calls"
                || relation.attributes.get("idempotency_key_forwarded") != Some(&Value::Bool(false))
            {
                continue;
            }
            let reaches_external_effect = relation.target_ids.iter().any(|target| {
                program.artifact(target).is_some_and(|artifact| {
                    attribute_bool(&artifact.attributes, "external_side_effect")
                })
            });
            if !reaches_external_effect {
                continue;
            }
            let target_refs = vec![relation.id.clone()];
            let obligation = materialize(
                program,
                ObligationSpec {
                    rule: "relation.changed_call_contract@1",
                    origin_rule: None,
                    target_kind: "relation",
                    target_refs: target_refs.clone(),
                    property_id: "payment.idempotency_contract",
                    context_ids: contexts_containing_id(program, &relation.id),
                    required_capabilities: BTreeSet::from(["direct_calls".to_owned()]),
                    weight: 5.0,
                    depends_on: Vec::new(),
                    generator_ids: BTreeSet::from([relation.id.clone()]),
                    additional_source_ids: Vec::new(),
                },
            )?;
            payment_contract_ids.insert(relation.id.clone(), obligation.id().clone());
            obligations.push(obligation);
        }

        let reentry_relations = reentry_ids.keys().cloned().collect::<Vec<_>>();
        let mut invariant_indices = BTreeMap::<String, usize>::new();
        for entry_relation_id in reentry_relations {
            let Some(entry_relation) = program.relation(&entry_relation_id) else {
                continue;
            };
            for submit in &entry_relation.target_ids {
                for first_call in program
                    .relations()
                    .iter()
                    .filter(|relation| relation.kind == "calls" && relation.source_id == *submit)
                {
                    for payment in &first_call.target_ids {
                        for external_call in program.relations().iter().filter(|relation| {
                            relation.kind == "calls"
                                && relation.source_id == *payment
                                && payment_contract_ids.contains_key(&relation.id)
                        }) {
                            let dependencies = [
                                reentry_ids.get(&entry_relation_id).cloned(),
                                payment_contract_ids.get(&external_call.id).cloned(),
                            ]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>();
                            let target_refs = vec![
                                entry_relation_id.clone(),
                                first_call.id.clone(),
                                external_call.id.clone(),
                            ];
                            let path_generators = target_refs.iter().cloned().collect();
                            let path_context_ids = contexts_for_path(program, &target_refs);
                            let path_obligation = materialize(
                                program,
                                ObligationSpec {
                                    rule: "path.external_side_effect@1",
                                    origin_rule: None,
                                    target_kind: "path",
                                    context_ids: path_context_ids.clone(),
                                    target_refs,
                                    property_id: "payment.at_most_once",
                                    required_capabilities: BTreeSet::from([
                                        "direct_calls".to_owned(),
                                        "concurrency_model".to_owned(),
                                        "test_mapping".to_owned(),
                                    ]),
                                    weight: 6.0,
                                    depends_on: dependencies,
                                    generator_ids: path_generators,
                                    additional_source_ids: Vec::new(),
                                },
                            )?;
                            let path_id = path_obligation.id().clone();
                            obligations.push(path_obligation);
                            for invariant in program.invariants() {
                                if invariant.property_id != "payment.at_most_once" {
                                    continue;
                                }
                                let scoped_context_ids = invariant
                                    .scope_ids
                                    .iter()
                                    .filter(|scope_id| {
                                        program
                                            .contexts()
                                            .iter()
                                            .any(|context| context.id == **scope_id)
                                    })
                                    .cloned()
                                    .collect::<BTreeSet<_>>();
                                // Context scopes constrain applicability. They
                                // are not merely extra metadata on every path:
                                // an invariant scoped to a disjoint context is
                                // not applicable to this path.
                                if !scoped_context_ids
                                    .is_subset(&path_context_ids.iter().cloned().collect())
                                {
                                    continue;
                                }
                                let mut invariant_context_ids = path_context_ids.clone();
                                invariant_context_ids.extend(scoped_context_ids.iter().cloned());
                                invariant_context_ids.sort();
                                invariant_context_ids.dedup();
                                let normalized_context_key = normalized_ids(
                                    &invariant_context_ids,
                                    "invariant.context_ids",
                                )?
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("|");
                                let key = format!("{}|{}", invariant.id, normalized_context_key);
                                if let Some(index) = invariant_indices.get(&key) {
                                    obligations[*index].merge_dependencies([path_id.clone()]);
                                    obligations[*index].merge_generators(
                                        std::iter::once(path_id.clone()).chain(
                                            std::iter::once(invariant.id.clone())
                                                .chain(invariant.scope_ids.iter().cloned()),
                                        ),
                                    );
                                    continue;
                                }
                                let invariant_obligation = materialize(
                                    program,
                                    ObligationSpec {
                                        rule: "invariant.payment_at_most_once@1",
                                        origin_rule: None,
                                        target_kind: "invariant",
                                        target_refs: vec![invariant.id.clone()],
                                        property_id: "payment.at_most_once",
                                        context_ids: invariant_context_ids,
                                        required_capabilities: BTreeSet::from([
                                            "direct_calls".to_owned(),
                                            "concurrency_model".to_owned(),
                                            "test_mapping".to_owned(),
                                        ]),
                                        weight: 7.0,
                                        depends_on: vec![path_id.clone()],
                                        generator_ids: std::iter::once(path_id.clone())
                                            .chain(std::iter::once(invariant.id.clone()))
                                            .chain(invariant.scope_ids.iter().cloned())
                                            .collect(),
                                        additional_source_ids: invariant
                                            .scope_ids
                                            .iter()
                                            .cloned()
                                            .collect(),
                                    },
                                )?;
                                invariant_indices.insert(key, obligations.len());
                                obligations.push(invariant_obligation);
                            }
                        }
                    }
                }
            }
        }

        // A capability gap is not an exclusion. It has its own versioned rule
        // and target contract, so a `subgraph` root is never emitted under a
        // relation/path/invariant rule that declares a different target kind.
        // Emit it even when concrete facts exist: a partial extractor can show
        // some targets while still leaving the origin rule incompletely known.
        for descriptor in Self::rules()
            .iter()
            .filter(|descriptor| descriptor.id != CAPABILITY_GAP_RULE)
        {
            let rule = rule_contract(descriptor.id)?;
            let missing = rule
                .required_capabilities
                .iter()
                .filter(|capability| !capability_fully_available(program, capability))
                .cloned()
                .collect::<BTreeSet<_>>();
            if missing.is_empty() {
                continue;
            }
            let mut reasons = missing
                .iter()
                .map(|capability| capability_gap_reason(program, capability))
                .collect::<BTreeSet<_>>();
            reasons.insert(format!("origin_rule:{}", descriptor.id));
            let missing_owned = missing
                .iter()
                .map(|item| (*item).to_owned())
                .collect::<Vec<_>>();
            let qualification_ids = capability_qualification_ids(program, &missing_owned);
            let fallback = obligation(
                program,
                ObligationSpec {
                    rule: CAPABILITY_GAP_RULE,
                    origin_rule: Some(descriptor.id),
                    target_kind: "subgraph",
                    target_refs: vec![program.snapshot_id().clone()],
                    property_id: CAPABILITY_GAP_PROPERTY,
                    context_ids: Vec::new(),
                    required_capabilities: missing_owned.iter().cloned().collect(),
                    weight: fallback_weight(descriptor.id)?,
                    depends_on: Vec::new(),
                    generator_ids: BTreeSet::from([
                        program.repository_id().clone(),
                        program.snapshot_id().clone(),
                    ]),
                    additional_source_ids: vec![program.repository_id().clone()],
                },
                "unknown".to_owned(),
                reasons,
                qualification_ids,
            )?;
            obligations.push(fallback);
        }

        obligations.sort_by(|left, right| {
            rule_order(left.version().rule())
                .cmp(&rule_order(right.version().rule()))
                .then_with(|| left.id().cmp(right.id()))
        });
        let mut generated_ids = BTreeSet::new();
        let mut registry = IdRegistry::default();
        for obligation in &obligations {
            if !generated_ids.insert(obligation.id().clone()) {
                return Err(DomainError::IdCollision {
                    id: obligation.id().clone(),
                });
            }
            registry.reserve(obligation.id().clone(), obligation)?;
        }
        exclusions.sort_by(|left, right| left.id.cmp(&right.id));
        let obligation_ids = obligations.iter().map(|item| item.id().clone()).collect();
        let mut limitation_ids = program
            .extraction()
            .limitations
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>();
        limitation_ids.extend(
            obligations
                .iter()
                .flat_map(|item| item.qualification_ids().iter().cloned()),
        );
        let universe = universe(program, obligation_ids, exclusions, limitation_ids)?;
        let contract = contract(&universe, &obligations)?;
        Ok(ObligationBundle {
            universe,
            obligations,
            contract,
        })
    }
}

fn fallback_weight(rule: &str) -> Result<f64> {
    match rule {
        "node.changed_public_symbol@1" => Ok(3.0),
        "relation.concurrent_reentry@1" => Ok(3.5),
        "relation.changed_call_contract@1" => Ok(5.0),
        "path.external_side_effect@1" => Ok(6.0),
        "invariant.payment_at_most_once@1" => Ok(7.0),
        _ => Err(DomainError::Validation(format!(
            "M1 does not define a fallback weight for rule `{rule}`"
        ))),
    }
}

fn universe(
    program: &ProgramSpace,
    obligation_ids: BTreeSet<StableId>,
    exclusions: Vec<ExclusionRecord>,
    limitation_ids: BTreeSet<StableId>,
) -> Result<UniverseDescriptor> {
    Ok(UniverseDescriptor {
        id: universe_id(program, &obligation_ids, &exclusions, &limitation_ids)?,
        snapshot_id: program.snapshot_id().clone(),
        profile_id: program.profile_key(),
        rule_set_hash: program.rule_set_hash().clone(),
        extractor_set_hash: program.extractor_set_hash().clone(),
        policy_version: program.policy_version().to_owned(),
        rule_pack_version: "m1.fixture@1".to_owned(),
        obligation_ids,
        exclusions,
        limitation_ids,
    })
}

fn universe_id(
    program: &ProgramSpace,
    obligation_ids: &BTreeSet<StableId>,
    exclusions: &[ExclusionRecord],
    limitation_ids: &BTreeSet<StableId>,
) -> Result<StableId> {
    let bindings = BTreeMap::from([
        (
            "denominator_obligation_ids".to_owned(),
            Value::Array(
                obligation_ids
                    .iter()
                    .map(|item| Value::String(item.to_string()))
                    .collect(),
            ),
        ),
        (
            "denominator_exclusions".to_owned(),
            serde_json::to_value(exclusions)
                .map_err(|error| DomainError::CanonicalJson(error.to_string()))?,
        ),
        (
            "extractor_set".to_owned(),
            Value::String(program.extractor_set_hash().to_string()),
        ),
        (
            "extraction_adapters".to_owned(),
            serde_json::to_value(&program.extraction().adapters)
                .map_err(|error| DomainError::CanonicalJson(error.to_string()))?,
        ),
        (
            "extraction_capabilities".to_owned(),
            serde_json::to_value(&program.extraction().capabilities)
                .map_err(|error| DomainError::CanonicalJson(error.to_string()))?,
        ),
        (
            "extraction_limitations".to_owned(),
            serde_json::to_value(&program.extraction().limitations)
                .map_err(|error| DomainError::CanonicalJson(error.to_string()))?,
        ),
        (
            "limitation_ids".to_owned(),
            Value::Array(
                limitation_ids
                    .iter()
                    .map(|item| Value::String(item.to_string()))
                    .collect(),
            ),
        ),
        (
            "policy".to_owned(),
            Value::String(program.policy_version().to_owned()),
        ),
        ("profile".to_owned(), Value::String(program.profile_key())),
        (
            "rule_set".to_owned(),
            Value::String(program.rule_set_hash().to_string()),
        ),
        (
            "snapshot".to_owned(),
            Value::String(program.snapshot_id().to_string()),
        ),
        (
            "rule_pack".to_owned(),
            Value::String("m1.fixture@1".to_owned()),
        ),
    ]);
    StableId::derived("universe", &bindings)
}

fn exclusion(
    program: &ProgramSpace,
    rule: &str,
    source_id: StableId,
    reason: &str,
    excluded_weight: f64,
) -> Result<ExclusionRecord> {
    exclusion_for_targets(program, rule, &[source_id], reason, excluded_weight)
}

fn exclusion_for_targets(
    program: &ProgramSpace,
    rule: &str,
    source_ids: &[StableId],
    reason: &str,
    excluded_weight: f64,
) -> Result<ExclusionRecord> {
    if !excluded_weight.is_finite() || excluded_weight <= 0.0 {
        return Err(DomainError::Validation(
            "excluded candidate must retain a positive finite weight".to_owned(),
        ));
    }
    let source_ids = normalized_ids(source_ids, "exclusion.source_ids")?;
    let candidate_key = format!(
        "{rule}|{}",
        source_ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|")
    );
    let bindings = BTreeMap::from([
        ("candidate".to_owned(), Value::String(candidate_key.clone())),
        (
            "snapshot".to_owned(),
            Value::String(program.snapshot_id().to_string()),
        ),
    ]);
    Ok(ExclusionRecord {
        id: StableId::derived("exclusion", &bindings)?,
        candidate_key,
        reason: reason.to_owned(),
        source_ids,
        excluded_weight,
    })
}

struct ObligationSpec {
    rule: &'static str,
    origin_rule: Option<&'static str>,
    target_kind: &'static str,
    target_refs: Vec<StableId>,
    property_id: &'static str,
    context_ids: Vec<StableId>,
    required_capabilities: BTreeSet<String>,
    weight: f64,
    depends_on: Vec<StableId>,
    generator_ids: BTreeSet<StableId>,
    additional_source_ids: Vec<StableId>,
}

fn materialize(program: &ProgramSpace, spec: ObligationSpec) -> Result<Obligation> {
    let missing = spec
        .required_capabilities
        .iter()
        .filter(|capability| !capability_fully_available(program, capability))
        .cloned()
        .collect::<Vec<_>>();
    let (applicability_status, applicability_reasons, qualification_ids) = if missing.is_empty() {
        ("applicable".to_owned(), BTreeSet::new(), BTreeSet::new())
    } else {
        (
            "unknown".to_owned(),
            missing
                .iter()
                .map(|capability| capability_gap_reason(program, capability))
                .collect(),
            capability_qualification_ids(program, &missing),
        )
    };
    obligation(
        program,
        spec,
        applicability_status,
        applicability_reasons,
        qualification_ids,
    )
}

/// Only a `complete` capability fully satisfies a rule's completeness
/// requirement. `partial`, `missing`, and `unknown` must all leave a targeted
/// unknown obligation or a rule-level capability-gap obstruction rather than
/// being silently treated as if the capability were fully available; already
/// resolved facts still produce their (now `unknown`) obligation instead of
/// being dropped.
fn capability_fully_available(program: &ProgramSpace, capability: &str) -> bool {
    matches!(
        program
            .extraction()
            .capabilities
            .get(capability)
            .map(|declaration| declaration.state),
        Some(CapabilityState::Complete)
    )
}

/// A capability's gap reason distinguishes exactly why it did not satisfy a
/// rule's completeness requirement: `partial` (some facts resolved, some
/// not), `missing` (the adapter declared it entirely unavailable), `unknown`
/// (the adapter never established a completeness result), and undeclared
/// (no rule input ever named this capability at all). Collapsing any of
/// these into a shared tag would hide which of those four distinct
/// situations a reviewer is looking at.
fn capability_gap_reason(program: &ProgramSpace, capability: &str) -> String {
    let tag = match program
        .extraction()
        .capabilities
        .get(capability)
        .map(|declaration| declaration.state)
    {
        Some(CapabilityState::Partial) => "capability_partial",
        Some(CapabilityState::Missing) => "capability_missing",
        Some(CapabilityState::Unknown) => "capability_unknown",
        None => "capability_undeclared",
        // `capability_fully_available` already filters `Complete` out of the
        // `missing` set this function is called against.
        Some(CapabilityState::Complete) => "capability_missing",
    };
    format!("{tag}:{capability}")
}

/// Links an obligation targeted by an incomplete capability to the extraction
/// limitations that explain the incompleteness, when the adapter tagged them.
fn capability_qualification_ids(program: &ProgramSpace, missing: &[String]) -> BTreeSet<StableId> {
    program
        .extraction()
        .limitations
        .iter()
        .filter(|limitation| {
            missing
                .iter()
                .any(|capability| limitation.related_capabilities.contains(capability))
        })
        .map(|limitation| limitation.id.clone())
        .collect()
}

fn obligation(
    program: &ProgramSpace,
    spec: ObligationSpec,
    applicability_status: String,
    applicability_reasons: BTreeSet<String>,
    qualification_ids: BTreeSet<StableId>,
) -> Result<Obligation> {
    let normalized_target_refs = normalized_ids(&spec.target_refs, "obligation.target_refs")?;
    let normalized_context_ids = normalized_ids(&spec.context_ids, "obligation.context_ids")?;
    let normalized_depends_on = normalized_ids(&spec.depends_on, "obligation.depends_on")?;
    let semantic_key = format!(
        "{}|{}{}",
        spec.property_id,
        spec.target_refs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("|"),
        spec.origin_rule
            .map(|origin| format!("|origin_rule:{origin}"))
            .unwrap_or_default(),
    );
    let version = VersionTuple::new(
        program.profile_key(),
        spec.rule,
        program.extractor_set_hash().clone(),
        program.snapshot_id().clone(),
    )?;
    let contract = rule_contract(spec.rule)?;
    let mut bindings = BTreeMap::from([
        (
            "applicability_scope".to_owned(),
            Value::Array(
                normalized_context_ids
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        ),
        (
            "profile".to_owned(),
            Value::String(version.profile().to_owned()),
        ),
        (
            "property".to_owned(),
            Value::String(spec.property_id.to_owned()),
        ),
        ("rule".to_owned(), Value::String(spec.rule.to_owned())),
        (
            "normalized_target_refs".to_owned(),
            Value::Array(
                normalized_target_refs
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        ),
        (
            "snapshot_semantic_id".to_owned(),
            Value::String(version.snapshot().to_string()),
        ),
    ]);
    if spec.target_kind == "path" {
        bindings.insert(
            "ordered_path_target_refs".to_owned(),
            Value::Array(
                spec.target_refs
                    .iter()
                    .map(|id| Value::String(id.to_string()))
                    .collect(),
            ),
        );
    }
    if let Some(origin_rule) = spec.origin_rule {
        bindings.insert(
            "origin_rule".to_owned(),
            Value::String(origin_rule.to_owned()),
        );
    }
    if spec.rule == CAPABILITY_GAP_RULE {
        // A capability-gap obligation is explicitly versioned by the exact
        // capability-state reasons it is grounded in (ADR 0011 §9): unlike a
        // concrete rule obligation's ID, which is stable across a capability
        // state change, a gap obligation's identity must change when the
        // gap it represents changes (for example `partial` becoming
        // `missing`), so a stale gap can never silently keep the same ID as
        // a materially different one.
        let capability_gap_reasons = applicability_reasons
            .iter()
            .filter(|reason| !reason.starts_with("origin_rule:"))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        bindings.insert(
            "capability_gap_reasons".to_owned(),
            Value::Array(capability_gap_reasons),
        );
    }
    let mut source_ids = spec.target_refs.clone();
    source_ids.extend(spec.context_ids.iter().cloned());
    source_ids.extend(spec.additional_source_ids.iter().cloned());
    // Synthesized source provenance is a set assembled from several typed
    // facts (target, context, invariant scope). Deduplicate that derived set;
    // raw ProgramSpace adapters still reject duplicate user-supplied sets.
    let normalized_source_ids = source_ids.into_iter().collect::<BTreeSet<_>>();
    let source_ids = normalized_source_ids.iter().cloned().collect::<Vec<_>>();
    Obligation::new(ObligationParts {
        id: StableId::derived("obligation", &bindings)?,
        target_kind: spec.target_kind.to_owned(),
        target_refs: spec.target_refs,
        normalized_target_refs,
        semantic_key,
        property_id: spec.property_id.to_owned(),
        property_version: "1".to_owned(),
        context_ids: spec.context_ids,
        normalized_context_ids,
        required_capabilities: spec.required_capabilities,
        evidence_required: true,
        accepted_evidence_modes: contract
            .accepted_modes
            .iter()
            .map(|mode| (*mode).to_owned())
            .collect(),
        applicability_status,
        applicability_reasons,
        qualification_ids,
        weight: spec.weight,
        version,
        depends_on: spec.depends_on,
        normalized_depends_on,
        generator_ids: spec.generator_ids,
        source_ids,
        normalized_source_ids,
    })
}

fn contract(
    universe: &UniverseDescriptor,
    obligations: &[Obligation],
) -> Result<ObligationContract> {
    let source = ContractSource {
        kind: "tool",
        locator: "reviewgraphen obligation synthesizer",
        revision: "0.1.0-draft",
        content_hash: ContentHash::parse("sha256:8888888888888888")?,
    };
    let contract_universe = ContractUniverse {
        id: universe.id().clone(),
        snapshot_id: universe.snapshot_id().clone(),
        profile_id: universe.profile_id().to_owned(),
        rule_set_hash: universe.rule_set_hash.clone(),
        extractor_set_hash: universe.extractor_set_hash.clone(),
        policy_version: universe.policy_version.clone(),
        obligation_ids: obligations
            .iter()
            .map(|obligation| obligation.id().clone())
            .collect(),
        limitation_ids: universe.limitation_ids().iter().cloned().collect(),
        exclusions: universe
            .exclusions()
            .iter()
            .map(|item| ContractExclusion {
                id: item.id.clone(),
                candidate_key: item.candidate_key.clone(),
                reason: item.reason.clone(),
                source_ids: item.source_ids.iter().cloned().collect(),
                excluded_weight: item.excluded_weight,
            })
            .collect(),
    };
    let obligations = obligations
        .iter()
        .map(|obligation| contract_obligation(obligation, &source))
        .collect::<Result<Vec<_>>>()?;
    ObligationContract::new(source, contract_universe, obligations)
}

fn contract_obligation(
    obligation: &Obligation,
    source: &ContractSource,
) -> Result<ContractObligation> {
    let rule = rule_contract(obligation.version().rule())?;
    Ok(ContractObligation {
        id: obligation.id().clone(),
        target: ContractTarget {
            kind: obligation.target_kind().to_owned(),
            refs: obligation.target_refs().to_vec(),
            semantic_key: obligation.semantic_key().to_owned(),
        },
        property: ContractProperty {
            id: obligation.property_id().to_owned(),
            version: obligation.property_version().to_owned(),
        },
        context_requirement: ContractContextRequirement {
            context_ids: obligation.context_ids().to_vec(),
            required_capabilities: obligation.required_capabilities().iter().cloned().collect(),
            include_relation_kinds: rule
                .include_relation_kinds
                .iter()
                .map(|kind| (*kind).to_owned())
                .collect(),
            max_relation_depth: rule.max_relation_depth,
            include_tests: true,
            include_existing_evidence: true,
        },
        evidence_requirement: ContractEvidenceRequirement {
            required: obligation.evidence_required(),
            accepted_modes: rule
                .accepted_modes
                .iter()
                .map(|mode| (*mode).to_owned())
                .collect(),
            minimum_count: 1,
            policy: rule.evidence_policy,
        },
        risk: ContractRisk {
            impact: rule.impact,
            exposure: rule.exposure,
            uncertainty: rule.uncertainty,
            structural_reach: rule.structural_reach,
            weight: obligation.weight(),
            rationale: rule.rationale,
        },
        applicability: ContractApplicability {
            status: obligation.applicability_status().to_owned(),
            reasons: obligation.applicability_reasons().iter().cloned().collect(),
            qualification_ids: obligation.qualification_ids().iter().cloned().collect(),
        },
        version: obligation.version().clone(),
        provenance: ContractProvenance {
            source: source.clone(),
            extraction_method: obligation.version().rule().to_owned(),
            tool_version: "0.1.0-draft",
            confidence: 1.0,
            review_status: "accepted",
            generator_ids: obligation.generator_ids().iter().cloned().collect(),
            origin_rule: obligation
                .applicability_reasons()
                .iter()
                .find_map(|reason| reason.strip_prefix("origin_rule:").map(ToOwned::to_owned)),
        },
        lifecycle: obligation.lifecycle().as_str(),
        depends_on_obligation_ids: obligation.depends_on().to_vec(),
        source_ids: obligation.source_ids().to_vec(),
    })
}

struct RuleContract {
    required_capabilities: &'static [&'static str],
    include_relation_kinds: &'static [&'static str],
    max_relation_depth: u64,
    accepted_modes: &'static [&'static str],
    evidence_policy: &'static str,
    impact: &'static str,
    exposure: f64,
    uncertainty: f64,
    structural_reach: f64,
    rationale: &'static str,
}

fn rule_contract(rule: &str) -> Result<RuleContract> {
    let contract = match rule {
        "node.changed_public_symbol@1" => RuleContract {
            required_capabilities: &["ast", "concurrency_model"],
            include_relation_kinds: &["awaits", "writes", "handled_by"],
            max_relation_depth: 2,
            accepted_modes: &["source_inspection", "static_fact", "test"],
            evidence_policy: "default-evidence@1",
            impact: "high",
            exposure: 0.8,
            uncertainty: 0.4,
            structural_reach: 1.0,
            rationale: "External payment side effect is reachable from a repeatable UI event.",
        },
        "relation.concurrent_reentry@1" => RuleContract {
            required_capabilities: &["direct_calls", "concurrency_model"],
            include_relation_kinds: &["handled_by", "awaits", "writes"],
            max_relation_depth: 2,
            accepted_modes: &["static_fact", "test"],
            evidence_policy: "default-evidence@1",
            impact: "high",
            exposure: 0.8,
            uncertainty: 0.4,
            structural_reach: 1.0,
            rationale: "External payment side effect is reachable from a repeatable UI event.",
        },
        "relation.changed_call_contract@1" => RuleContract {
            required_capabilities: &["direct_calls"],
            include_relation_kinds: &["calls"],
            max_relation_depth: 2,
            accepted_modes: &["source_inspection", "api_contract", "test"],
            evidence_policy: "critical-requires-machine-or-human-witness@1",
            impact: "critical",
            exposure: 1.0,
            uncertainty: 0.4,
            structural_reach: 1.0,
            rationale: "External payment side effect is reachable from a repeatable UI event.",
        },
        "path.external_side_effect@1" => RuleContract {
            required_capabilities: &["direct_calls", "concurrency_model", "test_mapping"],
            include_relation_kinds: &["handled_by", "calls", "covers"],
            max_relation_depth: 4,
            accepted_modes: &["counterexample_path", "test"],
            evidence_policy: "critical-requires-machine-or-human-witness@1",
            impact: "critical",
            exposure: 1.0,
            uncertainty: 0.4,
            structural_reach: 3.0,
            rationale: "External payment side effect is reachable from a repeatable UI event.",
        },
        "invariant.payment_at_most_once@1" => RuleContract {
            required_capabilities: &["direct_calls", "concurrency_model", "test_mapping"],
            include_relation_kinds: &["handled_by", "calls", "covers", "constrains"],
            max_relation_depth: 4,
            accepted_modes: &["counterexample_path", "test", "human_decision"],
            evidence_policy: "critical-requires-machine-or-human-witness@1",
            impact: "critical",
            exposure: 1.0,
            uncertainty: 0.4,
            structural_reach: 3.0,
            rationale: "External payment side effect is reachable from a repeatable UI event.",
        },
        CAPABILITY_GAP_RULE => RuleContract {
            required_capabilities: &[],
            include_relation_kinds: &[],
            max_relation_depth: 0,
            accepted_modes: &[],
            evidence_policy: "capability-gap-obstruction@1",
            impact: "unknown",
            exposure: 0.0,
            uncertainty: 1.0,
            structural_reach: 0.0,
            rationale: "The origin rule cannot be fully assessed because a required extraction capability is unavailable.",
        },
        _ => {
            return Err(DomainError::Validation(format!(
                "M1 does not define a contract for rule `{rule}`"
            )));
        }
    };
    Ok(contract)
}

fn rule_order(rule: &str) -> usize {
    MvpRulePack::rules()
        .iter()
        .position(|descriptor| descriptor.id == rule)
        .unwrap_or(usize::MAX)
}

fn normalized_ids(values: &[StableId], field: &'static str) -> Result<BTreeSet<StableId>> {
    let mut normalized = BTreeSet::new();
    for value in values {
        if !normalized.insert(value.clone()) {
            return Err(DomainError::Validation(format!(
                "{field} must not contain duplicate IDs"
            )));
        }
    }
    Ok(normalized)
}

fn contexts_containing_id(program: &ProgramSpace, source_id: &StableId) -> Vec<StableId> {
    program
        .contexts()
        .iter()
        .filter(|context| context.member_ids.contains(source_id))
        .map(|context| context.id.clone())
        .collect()
}

fn contexts_for_node(program: &ProgramSpace, artifact_id: &StableId) -> Vec<StableId> {
    let mut contexts = Vec::new();
    for relation in program.relations().iter().filter(|relation| {
        relation.kind == "handled_by" && relation.target_ids.contains(artifact_id)
    }) {
        extend_unique(&mut contexts, contexts_containing_id(program, &relation.id));
    }
    contexts
}

fn contexts_for_path(program: &ProgramSpace, ordered_path: &[StableId]) -> Vec<StableId> {
    let mut contexts = Vec::new();
    for relation_id in ordered_path {
        extend_unique(&mut contexts, contexts_containing_id(program, relation_id));
    }

    let normalized_path = ordered_path.iter().cloned().collect::<BTreeSet<_>>();
    for cover in program.relations().iter().filter(|relation| {
        relation.kind == "covers" && relation.target_ids.is_superset(&normalized_path)
    }) {
        extend_unique(&mut contexts, contexts_containing_id(program, &cover.id));
    }
    contexts
}

fn extend_unique(target: &mut Vec<StableId>, candidates: Vec<StableId>) {
    for candidate in candidates {
        if !target.contains(&candidate) {
            target.push(candidate);
        }
    }
}

fn is_changed_public_symbol(program: &ProgramSpace, artifact_id: &StableId) -> bool {
    program.artifact(artifact_id).is_some_and(|artifact| {
        attribute_bool(&artifact.attributes, "changed")
            || program.relations().iter().any(|relation| {
                relation.kind == "contains"
                    && relation.target_ids.contains(artifact_id)
                    && program
                        .artifact(&relation.source_id)
                        .is_some_and(|container| attribute_bool(&container.attributes, "changed"))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_target_order_is_an_identity_input_while_context_set_order_is_not() {
        let program = ProgramSpace::from_json_slice(include_bytes!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .expect("reference fixture");
        let target_refs = vec![
            StableId::parse("relation:tap-handled-by-submit").expect("id"),
            StableId::parse("relation:submit-calls-payment").expect("id"),
            StableId::parse("relation:payment-calls-stripe").expect("id"),
        ];
        let contexts = vec![
            StableId::parse("context:ui-event").expect("id"),
            StableId::parse("context:payment").expect("id"),
            StableId::parse("context:test").expect("id"),
        ];
        let first = obligation(
            &program,
            ObligationSpec {
                rule: "path.external_side_effect@1",
                origin_rule: None,
                target_kind: "path",
                target_refs: target_refs.clone(),
                property_id: "payment.at_most_once",
                context_ids: contexts.clone(),
                required_capabilities: BTreeSet::new(),
                weight: 1.0,
                depends_on: Vec::new(),
                generator_ids: BTreeSet::new(),
                additional_source_ids: Vec::new(),
            },
            "applicable".to_owned(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .expect("path obligation");
        let mut reversed_targets = target_refs;
        reversed_targets.reverse();
        let mut reversed_contexts = contexts;
        reversed_contexts.reverse();
        let second = obligation(
            &program,
            ObligationSpec {
                rule: "path.external_side_effect@1",
                origin_rule: None,
                target_kind: "path",
                target_refs: reversed_targets,
                property_id: "payment.at_most_once",
                context_ids: reversed_contexts,
                required_capabilities: BTreeSet::new(),
                weight: 1.0,
                depends_on: Vec::new(),
                generator_ids: BTreeSet::new(),
                additional_source_ids: Vec::new(),
            },
            "applicable".to_owned(),
            BTreeSet::new(),
            BTreeSet::new(),
        )
        .expect("path obligation");
        assert_ne!(first.id(), second.id());
        assert_eq!(
            first.normalized_context_ids(),
            second.normalized_context_ids()
        );
    }
}
