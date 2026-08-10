use crate::{
    ContentHash, DomainError, Obligation, ObligationLifecycle, PlanningError, Result,
    ReviewAggregate, Severity, StableId,
};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};

/// Fixed D1 planner policy. Its fields are deliberately not caller configurable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannerPolicyV1;

impl PlannerPolicyV1 {
    pub const VERSION: &'static str = "scheduler.baseline@1";
    pub const MAX_OBLIGATIONS: usize = 2_048;
    pub const MAX_EDGES: usize = 65_536;
    pub const MAX_WAVES: usize = 1_024;
    pub const MAX_OBLIGATIONS_PER_WAVE: usize = 2_048;
    pub const MAX_STRING_BYTES: usize = 16_384;
    pub const MAX_CANONICAL_INPUT_BYTES: usize = 786_432;
    pub const MAX_CANONICAL_PLAN_BYTES: usize = 786_432;

    #[must_use]
    pub const fn baseline() -> Self {
        Self
    }

    /// Canonical bytes of the planner sub-policy, without using an unbounded
    /// serde value tree.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let mut out = BoundedJson::new(Self::MAX_CANONICAL_INPUT_BYTES);
        out.push("{\"canonical_input_bytes\":786432,\"canonical_plan_bytes\":786432,")?;
        out.push("\"deferral_reasons\":[\"budget_exhausted\",\"prerequisite_deferred\"],")?;
        out.push("\"empty_universe\":\"allowed\",\"initial_lifecycle\":\"generated\",")?;
        out.push("\"max_edges\":65536,\"max_obligations\":2048,")?;
        out.push(
            "\"max_obligations_per_wave\":2048,\"max_string_bytes\":16384,\"max_waves\":1024,",
        )?;
        out.push("\"ordering\":[\"impact_desc\",\"prerequisite_depth_desc\",\"stable_id_asc\"],")?;
        out.push("\"rules\":[\"all_obligations_generated\",\"bounded_canonical_serialization\",\"dependency_cycle_domain_failure\",\"dependency_dangling_domain_failure\",\"kahn_boundary_ready_only\",\"likelihood_medium\",\"max_string_all_canonical_text\",\"plan_contains_planner_input_hash\",\"planner_input_bounded_writer\",\"reasons_closed_tags_only\",\"risk_rationale_absent\",\"schedule_wave_reason_absent\",\"wave_id_plan_index_ids\"],")?;
        out.push("\"thresholds\":[[\"0\",\"0.25\",\"info\"],[\"0.25\",\"0.5\",\"low\"],[\"0.5\",\"1\",\"medium\"],[\"1\",\"2\",\"high\"],[\"2\",\"infinity\",\"critical\"]],\"version\":\"scheduler.baseline@1\"}")?;
        out.finish()
    }

    pub fn hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }
}

/// Fixed wave-count and per-wave capacity request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanBudget {
    max_waves: u32,
    max_obligations_per_wave: u32,
}

impl PlanBudget {
    pub fn new(max_waves: u32, max_obligations_per_wave: u32) -> Result<Self> {
        if max_waves == 0 || max_waves as usize > PlannerPolicyV1::MAX_WAVES {
            return Err(DomainError::Planning(PlanningError::InvalidBudget));
        }
        if max_obligations_per_wave == 0
            || max_obligations_per_wave as usize > PlannerPolicyV1::MAX_OBLIGATIONS_PER_WAVE
        {
            return Err(DomainError::Planning(PlanningError::InvalidBudget));
        }
        Ok(Self {
            max_waves,
            max_obligations_per_wave,
        })
    }

    #[must_use]
    pub const fn max_waves(self) -> u32 {
        self.max_waves
    }
    #[must_use]
    pub const fn max_obligations_per_wave(self) -> u32 {
        self.max_obligations_per_wave
    }
}

impl Serialize for PlanBudget {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("PlanBudget", 2)?;
        state.serialize_field("max_obligations_per_wave", &self.max_obligations_per_wave)?;
        state.serialize_field("max_waves", &self.max_waves)?;
        state.end()
    }
}

/// Minimal immutable planning view of one generated obligation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannerObligation {
    id: StableId,
    depends_on: Vec<StableId>,
    weight: f64,
    lifecycle: ObligationLifecycle,
}

impl PlannerObligation {
    pub fn from_obligation(obligation: &Obligation) -> Self {
        Self {
            id: obligation.id().clone(),
            depends_on: obligation.depends_on().to_vec(),
            weight: obligation.weight(),
            lifecycle: obligation.lifecycle(),
        }
    }

    /// Public only for deterministic adapters; `PlannerInput::new` enforces
    /// the Generated-only D1 boundary.
    #[cfg(test)]
    pub(crate) fn new(
        id: StableId,
        depends_on: Vec<StableId>,
        weight: f64,
        lifecycle: ObligationLifecycle,
    ) -> Self {
        Self {
            id,
            depends_on,
            weight,
            lifecycle,
        }
    }
}

/// Validated exact input to the D1 scheduler.
#[derive(Clone, Debug)]
pub(crate) struct PlannerInput {
    budget: PlanBudget,
    obligations: Vec<PlannerObligation>,
    planner_policy_hash: ContentHash,
    snapshot_id: StableId,
    universe_id: StableId,
}

impl PlannerInput {
    pub(crate) fn new(
        budget: PlanBudget,
        mut obligations: Vec<PlannerObligation>,
        snapshot_id: StableId,
        universe_id: StableId,
    ) -> Result<Self> {
        let policy = PlannerPolicyV1::baseline();
        if obligations.len() > PlannerPolicyV1::MAX_OBLIGATIONS {
            return Err(incomplete(
                "planner input obligations",
                PlannerPolicyV1::MAX_OBLIGATIONS,
                obligations.len(),
            ));
        }
        let mut edges = 0_usize;
        for obligation in &obligations {
            validate_text(&obligation.id.to_string())?;
            if obligation.id.kind() != "obligation"
                || obligation
                    .depends_on
                    .iter()
                    .any(|dependency| dependency.kind() != "obligation")
            {
                return Err(DomainError::Planning(PlanningError::InvalidIdKind));
            }
            if obligation.lifecycle != ObligationLifecycle::Generated {
                return Err(DomainError::Planning(PlanningError::NonGenerated));
            }
            if !obligation.weight.is_finite() || obligation.weight <= 0.0 {
                return Err(DomainError::Planning(PlanningError::InvalidWeight));
            }
            edges = edges
                .checked_add(obligation.depends_on.len())
                .ok_or_else(|| {
                    incomplete(
                        "planner input edges",
                        PlannerPolicyV1::MAX_EDGES,
                        usize::MAX,
                    )
                })?;
            if edges > PlannerPolicyV1::MAX_EDGES {
                return Err(incomplete(
                    "planner input edges",
                    PlannerPolicyV1::MAX_EDGES,
                    edges,
                ));
            }
            for dependency in &obligation.depends_on {
                validate_text(&dependency.to_string())?;
            }
        }
        obligations.sort_by(|left, right| left.id.cmp(&right.id));
        let mut ids = BTreeSet::new();
        for obligation in &mut obligations {
            obligation.depends_on.sort();
            if obligation
                .depends_on
                .windows(2)
                .any(|pair| pair[0] == pair[1])
            {
                return Err(DomainError::Planning(PlanningError::DuplicateDependency));
            }
            if !ids.insert(obligation.id.clone()) {
                return Err(DomainError::Planning(PlanningError::DuplicateObligation));
            }
        }
        validate_text(&snapshot_id.to_string())?;
        validate_text(&universe_id.to_string())?;
        if snapshot_id.kind() != "snapshot" || universe_id.kind() != "universe" {
            return Err(DomainError::Planning(PlanningError::InvalidIdKind));
        }
        for obligation in &obligations {
            for dependency in &obligation.depends_on {
                if !ids.contains(dependency) {
                    return Err(DomainError::DanglingReference {
                        owner: "planner obligation",
                        owner_id: obligation.id.clone(),
                        reference: dependency.clone(),
                    });
                }
            }
        }
        let input = Self {
            budget,
            obligations,
            planner_policy_hash: policy.hash()?,
            snapshot_id,
            universe_id,
        };
        input.canonical_bytes()?;
        Ok(input)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let mut out = BoundedJson::new(PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES);
        out.push("{\"budget\":{")?;
        out.push("\"max_obligations_per_wave\":")?;
        out.push_u32(self.budget.max_obligations_per_wave)?;
        out.push(",\"max_waves\":")?;
        out.push_u32(self.budget.max_waves)?;
        out.push("},\"obligations\":[")?;
        for (index, obligation) in self.obligations.iter().enumerate() {
            if index != 0 {
                out.push(",")?;
            }
            out.push("{\"depends_on\":[")?;
            write_ids(&mut out, &obligation.depends_on)?;
            out.push("],\"id\":")?;
            out.string(&obligation.id.to_string())?;
            out.push(",\"weight_ieee754_bits\":")?;
            out.string(&format!("{:016x}", obligation.weight.to_bits()))?;
            out.push("}")?;
        }
        out.push("],\"planner_policy_hash\":")?;
        out.string(&self.planner_policy_hash.to_string())?;
        out.push(",\"snapshot_id\":")?;
        out.string(&self.snapshot_id.to_string())?;
        out.push(",\"universe_id\":")?;
        out.string(&self.universe_id.to_string())?;
        out.push("}")?;
        out.finish()
    }

    pub fn hash(&self) -> Result<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }
}

/// Risk derived deterministically from an obligation weight. It has no prose rationale.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RiskDescriptor {
    impact: Severity,
    likelihood: Severity,
}
impl RiskDescriptor {
    #[must_use]
    pub const fn impact(self) -> Severity {
        self.impact
    }
    #[must_use]
    pub const fn likelihood(self) -> Severity {
        self.likelihood
    }
}

/// Closed D1 deferral reasons.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DeferralReason {
    BudgetExhausted,
    PrerequisiteDeferred,
}
impl DeferralReason {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BudgetExhausted => "budget_exhausted",
            Self::PrerequisiteDeferred => "prerequisite_deferred",
        }
    }
}

/// A wave deliberately has no reason field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleWave {
    id: StableId,
    wave_index: u32,
    obligation_ids: Vec<StableId>,
}
impl ScheduleWave {
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub const fn wave_index(&self) -> u32 {
        self.wave_index
    }
    #[must_use]
    pub fn obligation_ids(&self) -> &[StableId] {
        &self.obligation_ids
    }
}

/// Private constructors ensure plans can only come from validated scheduling.
#[derive(Clone, Debug)]
pub struct ReviewPlan {
    id: StableId,
    universe_id: StableId,
    snapshot_id: StableId,
    planner_policy_hash: ContentHash,
    planner_input_hash: ContentHash,
    budget: PlanBudget,
    risk_breakdown: BTreeMap<StableId, RiskDescriptor>,
    waves: Vec<ScheduleWave>,
    deferred: BTreeMap<StableId, DeferralReason>,
}

impl ReviewPlan {
    pub(crate) fn allocated_bytes(&self) -> usize {
        let risk_nodes = self
            .risk_breakdown
            .len()
            .saturating_mul(std::mem::size_of::<(StableId, RiskDescriptor)>());
        let risk_keys = self
            .risk_breakdown
            .keys()
            .map(StableId::allocated_bytes)
            .sum::<usize>();
        let wave_storage = self
            .waves
            .capacity()
            .saturating_mul(std::mem::size_of::<ScheduleWave>());
        let wave_heap = self.waves.iter().fold(0_usize, |total, wave| {
            total
                .saturating_add(wave.id.allocated_bytes())
                .saturating_add(
                    wave.obligation_ids
                        .capacity()
                        .saturating_mul(std::mem::size_of::<StableId>()),
                )
                .saturating_add(
                    wave.obligation_ids
                        .iter()
                        .map(StableId::allocated_bytes)
                        .sum::<usize>(),
                )
        });
        let deferred_nodes = self
            .deferred
            .len()
            .saturating_mul(std::mem::size_of::<(StableId, DeferralReason)>());
        let deferred_keys = self
            .deferred
            .keys()
            .map(StableId::allocated_bytes)
            .sum::<usize>();
        self.id
            .allocated_bytes()
            .saturating_add(self.universe_id.allocated_bytes())
            .saturating_add(self.snapshot_id.allocated_bytes())
            .saturating_add(self.planner_policy_hash.allocated_bytes())
            .saturating_add(self.planner_input_hash.allocated_bytes())
            .saturating_add(risk_nodes)
            .saturating_add(risk_keys)
            .saturating_add(wave_storage)
            .saturating_add(wave_heap)
            .saturating_add(deferred_nodes)
            .saturating_add(deferred_keys)
    }

    pub(crate) fn build(input: PlannerInput) -> Result<Self> {
        schedule(input)
    }
    #[must_use]
    pub fn id(&self) -> &StableId {
        &self.id
    }
    #[must_use]
    pub fn waves(&self) -> &[ScheduleWave] {
        &self.waves
    }
    #[must_use]
    pub fn deferred(&self) -> &BTreeMap<StableId, DeferralReason> {
        &self.deferred
    }
    #[must_use]
    pub fn planner_input_hash(&self) -> &ContentHash {
        &self.planner_input_hash
    }
    #[must_use]
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }
    #[must_use]
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    #[must_use]
    pub fn planner_policy_hash(&self) -> &ContentHash {
        &self.planner_policy_hash
    }
    #[must_use]
    pub const fn planner_policy_version(&self) -> &'static str {
        PlannerPolicyV1::VERSION
    }
    #[must_use]
    pub const fn budget(&self) -> PlanBudget {
        self.budget
    }
    pub fn identity_body_hash(&self) -> Result<ContentHash> {
        let mut contents = Vec::new();
        for wave in &self.waves {
            contents.push(wave.obligation_ids.clone());
        }
        let input = PlannerInput {
            budget: self.budget,
            obligations: Vec::new(),
            planner_policy_hash: self.planner_policy_hash.clone(),
            snapshot_id: self.snapshot_id.clone(),
            universe_id: self.universe_id.clone(),
        };
        let mut out = BoundedJson::new(PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES);
        write_plan_identity(
            &mut out,
            &input,
            &self.planner_input_hash,
            &contents,
            &self.deferred,
        )?;
        Ok(ContentHash::sha256(&out.finish()?))
    }
    #[must_use]
    pub fn risk_breakdown(&self) -> &BTreeMap<StableId, RiskDescriptor> {
        &self.risk_breakdown
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        plan_bytes(self, PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES)
    }

    pub(crate) fn from_event_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES {
            return Err(incomplete(
                "review plan canonical bytes",
                PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES,
                bytes.len(),
            ));
        }
        let raw: RawPlan = serde_json::from_slice(bytes)
            .map_err(|_| DomainError::Planning(PlanningError::InvalidInput))?;
        validate_raw_limits(&raw)?;
        if raw.planner_policy_version != PlannerPolicyV1::VERSION
            || raw.planner_policy_hash != PlannerPolicyV1::baseline().hash()?
        {
            return Err(DomainError::Planning(PlanningError::InvalidInput));
        }
        if raw.id.kind() != "plan"
            || raw.snapshot_id.kind() != "snapshot"
            || raw.universe_id.kind() != "universe"
        {
            return Err(DomainError::Planning(PlanningError::InvalidIdKind));
        }
        let budget = PlanBudget::new(raw.budget.max_waves, raw.budget.max_obligations_per_wave)?;
        let mut scheduled = BTreeSet::new();
        let mut waves = Vec::new();
        for (position, raw_wave) in raw.waves.iter().enumerate() {
            if raw_wave.wave_index
                != u32::try_from(position)
                    .map_err(|_| DomainError::Planning(PlanningError::InvalidInput))?
                || raw_wave.obligation_ids.is_empty()
            {
                return Err(DomainError::Planning(PlanningError::InvalidInput));
            }
            let mut ids = raw_wave.obligation_ids.clone();
            if ids.iter().any(|id| id.kind() != "obligation")
                || ids.iter().any(|id| !scheduled.insert(id.clone()))
            {
                return Err(DomainError::Planning(PlanningError::InvalidInput));
            }
            let id = derive_wave_id(&raw.id, raw_wave.wave_index, &ids)?;
            waves.push(ScheduleWave {
                id,
                wave_index: raw_wave.wave_index,
                obligation_ids: std::mem::take(&mut ids),
            });
        }
        let mut deferred = BTreeMap::new();
        for item in raw.deferred {
            if item.id.kind() != "obligation"
                || !scheduled.insert(item.id.clone())
                || deferred
                    .insert(item.id, parse_reason(&item.reason)?)
                    .is_some()
            {
                return Err(DomainError::Planning(PlanningError::InvalidInput));
            }
        }
        let mut risks = BTreeMap::new();
        for item in raw.risk_breakdown {
            if item.id.kind() != "obligation"
                || !scheduled.contains(&item.id)
                || risks
                    .insert(
                        item.id,
                        RiskDescriptor {
                            impact: parse_severity(&item.impact)?,
                            likelihood: parse_severity(&item.likelihood)?,
                        },
                    )
                    .is_some()
                || item.likelihood != "medium"
            {
                return Err(DomainError::Planning(PlanningError::InvalidInput));
            }
        }
        if risks.len() != scheduled.len()
            || raw.waves.len() > usize::try_from(budget.max_waves).unwrap_or(usize::MAX)
        {
            return Err(DomainError::Planning(PlanningError::InvalidInput));
        }
        let plan = Self {
            id: raw.id,
            universe_id: raw.universe_id,
            snapshot_id: raw.snapshot_id,
            planner_policy_hash: raw.planner_policy_hash,
            planner_input_hash: raw.planner_input_hash,
            budget,
            risk_breakdown: risks,
            waves,
            deferred,
        };
        if plan.identity_body_hash()?
            != ContentHash::parse(
                plan.id
                    .to_string()
                    .strip_prefix("plan:")
                    .ok_or(DomainError::Planning(PlanningError::InvalidIdKind))?
                    .to_owned(),
            )?
            || plan.canonical_bytes()? != bytes
        {
            return Err(DomainError::Planning(PlanningError::InvalidInput));
        }
        Ok(plan)
    }

    /// Strict metadata-only decode for durable projections. This reconstructs
    /// and validates the complete canonical plan against the immutable
    /// aggregate, but does not create any event admission capability.
    pub fn from_canonical_bytes(bytes: &[u8], aggregate: &ReviewAggregate) -> Result<Self> {
        let plan = Self::from_event_bytes(bytes)?;
        plan.validate_against(aggregate)?;
        Ok(plan)
    }

    pub fn validate_against(&self, aggregate: &ReviewAggregate) -> Result<()> {
        let rebuilt = plan(aggregate, self.budget)?;
        if rebuilt.canonical_bytes()? != self.canonical_bytes()? {
            return Err(DomainError::Planning(PlanningError::InvalidInput));
        }
        Ok(())
    }
}

impl Serialize for ReviewPlan {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Deferred<'a> {
            id: &'a StableId,
            reason: &'a str,
        }
        #[derive(Serialize)]
        struct Risk<'a> {
            id: &'a StableId,
            impact: &'a str,
            likelihood: &'static str,
        }
        #[derive(Serialize)]
        struct Wave<'a> {
            obligation_ids: &'a [StableId],
            wave_index: u32,
        }
        let deferred = self
            .deferred
            .iter()
            .map(|(id, reason)| Deferred {
                id,
                reason: reason.as_str(),
            })
            .collect::<Vec<_>>();
        let risk = self
            .risk_breakdown
            .iter()
            .map(|(id, value)| Risk {
                id,
                impact: severity(value.impact),
                likelihood: "medium",
            })
            .collect::<Vec<_>>();
        let waves = self
            .waves
            .iter()
            .map(|wave| Wave {
                obligation_ids: &wave.obligation_ids,
                wave_index: wave.wave_index,
            })
            .collect::<Vec<_>>();
        let mut state = serializer.serialize_struct("ReviewPlan", 10)?;
        state.serialize_field("budget", &self.budget)?;
        state.serialize_field("deferred", &deferred)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("planner_input_hash", &self.planner_input_hash)?;
        state.serialize_field("planner_policy_hash", &self.planner_policy_hash)?;
        state.serialize_field("planner_policy_version", PlannerPolicyV1::VERSION)?;
        state.serialize_field("risk_breakdown", &risk)?;
        state.serialize_field("snapshot_id", &self.snapshot_id)?;
        state.serialize_field("universe_id", &self.universe_id)?;
        state.serialize_field("waves", &waves)?;
        state.end()
    }
}

/// Public D1 entry point. The plan is bound to the aggregate's exact universe
/// denominator and snapshot; callers cannot substitute an arbitrary input set.
pub fn plan(aggregate: &ReviewAggregate, budget: PlanBudget) -> Result<ReviewPlan> {
    let expected = aggregate.universe().obligation_ids();
    if expected.len() > PlannerPolicyV1::MAX_OBLIGATIONS {
        return Err(incomplete(
            "planner input obligations",
            PlannerPolicyV1::MAX_OBLIGATIONS,
            expected.len(),
        ));
    }
    aggregate.validate()?;
    let mut obligations = Vec::new();
    obligations.try_reserve_exact(expected.len()).map_err(|_| {
        incomplete(
            "planner input obligations",
            PlannerPolicyV1::MAX_OBLIGATIONS,
            expected.len(),
        )
    })?;
    for obligation in aggregate.obligations() {
        obligations.push(PlannerObligation::from_obligation(obligation));
    }
    if obligations.len() != expected.len()
        || obligations
            .iter()
            .map(|item| item.id.clone())
            .collect::<BTreeSet<_>>()
            != *expected
    {
        return Err(DomainError::Planning(PlanningError::InvalidInput));
    }
    if aggregate.universe().snapshot_id() != aggregate.program().snapshot_id() {
        return Err(DomainError::Planning(PlanningError::InvalidInput));
    }
    plan_input(PlannerInput::new(
        budget,
        obligations,
        aggregate.universe().snapshot_id().clone(),
        aggregate.universe().id().clone(),
    )?)
}

pub(crate) fn plan_input(input: PlannerInput) -> Result<ReviewPlan> {
    ReviewPlan::build(input)
}

fn schedule(input: PlannerInput) -> Result<ReviewPlan> {
    let input_hash = input.hash()?;
    ensure_acyclic(&input)?;
    let mut by_id = BTreeMap::new();
    for obligation in input.obligations.iter().cloned() {
        by_id.insert(obligation.id.clone(), obligation);
    }
    let mut children: BTreeMap<StableId, Vec<StableId>> = BTreeMap::new();
    let mut remaining: BTreeMap<StableId, usize> = BTreeMap::new();
    let mut depth = BTreeMap::new();
    for (id, obligation) in &by_id {
        remaining.insert(id.clone(), obligation.depends_on.len());
        for dependency in &obligation.depends_on {
            children
                .entry(dependency.clone())
                .or_default()
                .push(id.clone());
        }
    }
    let mut ready: BTreeSet<StableId> = remaining
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut waves_contents: Vec<Vec<StableId>> = Vec::new();
    let mut placed = BTreeSet::new();
    while !ready.is_empty()
        && waves_contents.len()
            < usize::try_from(input.budget.max_waves)
                .map_err(|_| DomainError::Planning(PlanningError::InvalidBudget))?
    {
        let mut candidates = ready.iter().cloned().collect::<Vec<_>>();
        candidates.sort_by(|left, right| priority_cmp(left, right, &by_id, &depth));
        let selected = candidates
            .into_iter()
            .take(
                usize::try_from(input.budget.max_obligations_per_wave)
                    .map_err(|_| DomainError::Planning(PlanningError::InvalidBudget))?,
            )
            .collect::<Vec<_>>();
        for id in &selected {
            ready.remove(id);
            placed.insert(id.clone());
        }
        for id in &selected {
            let current_depth = depth.get(id).copied().unwrap_or(0);
            for child in children.get(id).into_iter().flatten() {
                let child_depth = depth.entry(child.clone()).or_insert(0);
                *child_depth = (*child_depth).max(current_depth + 1);
                let count = remaining.get_mut(child).expect("known child");
                *count -= 1;
                if *count == 0 {
                    ready.insert(child.clone());
                }
            }
        }
        waves_contents.push(selected);
    }
    if placed.len() != by_id.len() && ready.is_empty() {
        return Err(DomainError::Planning(PlanningError::Cycle));
    }
    let mut deferred = BTreeMap::new();
    for id in by_id.keys().filter(|id| !placed.contains(*id)) {
        let reason = if remaining[id] == 0 {
            DeferralReason::BudgetExhausted
        } else {
            DeferralReason::PrerequisiteDeferred
        };
        deferred.insert(id.clone(), reason);
    }
    let mut risks = BTreeMap::new();
    for (id, obligation) in &by_id {
        risks.insert(id.clone(), risk(obligation.weight));
    }
    let plan_id = derive_plan_id(&input, &input_hash, &waves_contents, &deferred)?;
    let waves = waves_contents
        .into_iter()
        .enumerate()
        .map(|(wave_index, ids)| {
            let wave_index = u32::try_from(wave_index)
                .map_err(|_| incomplete("wave index", u32::MAX as usize, usize::MAX))?;
            let id = derive_wave_id(&plan_id, wave_index, &ids)?;
            Ok(ScheduleWave {
                id,
                wave_index,
                obligation_ids: ids,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let result = ReviewPlan {
        id: plan_id,
        universe_id: input.universe_id,
        snapshot_id: input.snapshot_id,
        planner_policy_hash: input.planner_policy_hash,
        planner_input_hash: input_hash,
        budget: input.budget,
        risk_breakdown: risks,
        waves,
        deferred,
    };
    result.canonical_bytes()?;
    Ok(result)
}

fn ensure_acyclic(input: &PlannerInput) -> Result<()> {
    let mut remaining = input
        .obligations
        .iter()
        .map(|item| (item.id.clone(), item.depends_on.len()))
        .collect::<BTreeMap<_, _>>();
    let mut children = BTreeMap::<StableId, Vec<StableId>>::new();
    for item in &input.obligations {
        for dependency in &item.depends_on {
            children
                .entry(dependency.clone())
                .or_default()
                .push(item.id.clone());
        }
    }
    let mut ready = remaining
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| id.clone())
        .collect::<BTreeSet<_>>();
    let mut seen = 0_usize;
    while let Some(id) = ready.pop_first() {
        seen += 1;
        for child in children.get(&id).into_iter().flatten() {
            let count = remaining.get_mut(child).expect("validated planner child");
            *count -= 1;
            if *count == 0 {
                ready.insert(child.clone());
            }
        }
    }
    if seen == input.obligations.len() {
        Ok(())
    } else {
        Err(DomainError::Planning(PlanningError::Cycle))
    }
}

fn risk(weight: f64) -> RiskDescriptor {
    let impact = if weight <= 0.25 {
        Severity::Info
    } else if weight <= 0.5 {
        Severity::Low
    } else if weight <= 1.0 {
        Severity::Medium
    } else if weight <= 2.0 {
        Severity::High
    } else {
        Severity::Critical
    };
    RiskDescriptor {
        impact,
        likelihood: Severity::Medium,
    }
}

fn priority_cmp(
    left: &StableId,
    right: &StableId,
    all: &BTreeMap<StableId, PlannerObligation>,
    depth: &BTreeMap<StableId, usize>,
) -> std::cmp::Ordering {
    let left_obligation = &all[left];
    let right_obligation = &all[right];
    let impact_order = risk(right_obligation.weight)
        .impact
        .cmp(&risk(left_obligation.weight).impact);
    impact_order
        .then_with(|| {
            depth
                .get(right)
                .copied()
                .unwrap_or(0)
                .cmp(&depth.get(left).copied().unwrap_or(0))
        })
        .then_with(|| left.cmp(right))
}

fn derive_plan_id(
    input: &PlannerInput,
    input_hash: &ContentHash,
    waves: &[Vec<StableId>],
    deferred: &BTreeMap<StableId, DeferralReason>,
) -> Result<StableId> {
    let mut out = BoundedJson::new(PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES);
    write_plan_identity(&mut out, input, input_hash, waves, deferred)?;
    let hash = ContentHash::sha256(&out.finish()?);
    StableId::parse(format!("plan:{hash}"))
}

fn derive_wave_id(plan_id: &StableId, wave_index: u32, ids: &[StableId]) -> Result<StableId> {
    let mut out = BoundedJson::new(PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES);
    out.push("{\"ids\":[")?;
    write_ids(&mut out, ids)?;
    out.push("],\"plan_id\":")?;
    out.string(&plan_id.to_string())?;
    out.push(",\"wave_index\":")?;
    out.push_u32(wave_index)?;
    out.push("}")?;
    StableId::parse(format!(
        "schedule-wave:{}",
        ContentHash::sha256(&out.finish()?)
    ))
}

fn plan_bytes(plan: &ReviewPlan, max: usize) -> Result<Vec<u8>> {
    let mut out = BoundedJson::new(max);
    out.push("{")?;
    out.push("\"budget\":{")?;
    out.push("\"max_obligations_per_wave\":")?;
    out.push_u32(plan.budget.max_obligations_per_wave)?;
    out.push(",\"max_waves\":")?;
    out.push_u32(plan.budget.max_waves)?;
    out.push("},")?;
    out.push("\"deferred\":[")?;
    for (index, (id, reason)) in plan.deferred.iter().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.push("{\"id\":")?;
        out.string(&id.to_string())?;
        out.push(",\"reason\":")?;
        out.string(reason.as_str())?;
        out.push("}")?;
    }
    out.push("],\"id\":")?;
    out.string(&plan.id.to_string())?;
    out.push(",\"planner_input_hash\":")?;
    out.string(&plan.planner_input_hash.to_string())?;
    out.push(",\"planner_policy_hash\":")?;
    out.string(&plan.planner_policy_hash.to_string())?;
    out.push(",\"planner_policy_version\":\"scheduler.baseline@1\",")?;
    out.push("\"risk_breakdown\":[")?;
    for (index, (id, value)) in plan.risk_breakdown.iter().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.push("{\"id\":")?;
        out.string(&id.to_string())?;
        out.push(",\"impact\":")?;
        out.string(severity(value.impact))?;
        out.push(",\"likelihood\":\"medium\"}")?;
    }
    out.push("],\"snapshot_id\":")?;
    out.string(&plan.snapshot_id.to_string())?;
    out.push(",\"universe_id\":")?;
    out.string(&plan.universe_id.to_string())?;
    out.push(",\"waves\":[")?;
    for (index, wave) in plan.waves.iter().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.push("{\"obligation_ids\":[")?;
        write_ids(&mut out, &wave.obligation_ids)?;
        out.push("],\"wave_index\":")?;
        out.push_u32(wave.wave_index)?;
        out.push("}")?;
    }
    out.push("]}")?;
    out.finish()
}

fn write_plan_identity(
    out: &mut BoundedJson,
    input: &PlannerInput,
    input_hash: &ContentHash,
    waves: &[Vec<StableId>],
    deferred: &BTreeMap<StableId, DeferralReason>,
) -> Result<()> {
    out.push("{\"budget\":{")?;
    out.push("\"max_obligations_per_wave\":")?;
    out.push_u32(input.budget.max_obligations_per_wave)?;
    out.push(",\"max_waves\":")?;
    out.push_u32(input.budget.max_waves)?;
    out.push("},\"deferred_ids\":[")?;
    for (index, id) in deferred.keys().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.string(&id.to_string())?;
    }
    out.push("],\"planner_input_hash\":")?;
    out.string(&input_hash.to_string())?;
    out.push(",\"planner_policy_hash\":")?;
    out.string(&input.planner_policy_hash.to_string())?;
    out.push(",\"planner_policy_version\":\"scheduler.baseline@1\",\"snapshot_id\":")?;
    out.string(&input.snapshot_id.to_string())?;
    out.push(",\"universe_id\":")?;
    out.string(&input.universe_id.to_string())?;
    out.push(",\"waves\":[")?;
    for (index, ids) in waves.iter().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.push("{\"obligation_ids\":[")?;
        write_ids(out, ids)?;
        out.push("],\"wave_index\":")?;
        out.push_usize(index)?;
        out.push("}")?;
    }
    out.push("]}")
}

fn write_ids(out: &mut BoundedJson, ids: &[StableId]) -> Result<()> {
    for (index, id) in ids.iter().enumerate() {
        if index != 0 {
            out.push(",")?;
        }
        out.string(&id.to_string())?;
    }
    Ok(())
}
fn severity(value: Severity) -> &'static str {
    match value {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}
fn validate_text(value: &str) -> Result<()> {
    if value.len() > PlannerPolicyV1::MAX_STRING_BYTES {
        Err(incomplete(
            "planner canonical string",
            PlannerPolicyV1::MAX_STRING_BYTES,
            value.len(),
        ))
    } else {
        Ok(())
    }
}
fn incomplete(operation: &'static str, limit: usize, observed: usize) -> DomainError {
    DomainError::Incomplete {
        operation,
        limit,
        observed,
    }
}

struct BoundedJson {
    bytes: Vec<u8>,
    limit: usize,
}
impl BoundedJson {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
    fn push(&mut self, value: &str) -> Result<()> {
        let next = self
            .bytes
            .len()
            .checked_add(value.len())
            .ok_or_else(|| incomplete("planner canonical bytes", self.limit, usize::MAX))?;
        if next > self.limit {
            return Err(incomplete("planner canonical bytes", self.limit, next));
        }
        self.bytes
            .try_reserve_exact(value.len())
            .map_err(|_| incomplete("planner canonical reservation", self.limit, next))?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
    fn string(&mut self, value: &str) -> Result<()> {
        validate_text(value)?;
        self.push("\"")?;
        self.push(value)?;
        self.push("\"")
    }
    fn push_u32(&mut self, value: u32) -> Result<()> {
        self.push(&value.to_string())
    }
    fn push_usize(&mut self, value: usize) -> Result<()> {
        self.push(&value.to_string())
    }
    fn finish(self) -> Result<Vec<u8>> {
        Ok(self.bytes)
    }
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPlan {
    budget: RawBudget,
    deferred: Vec<RawDeferred>,
    id: StableId,
    planner_input_hash: ContentHash,
    planner_policy_hash: ContentHash,
    planner_policy_version: String,
    risk_breakdown: Vec<RawRisk>,
    snapshot_id: StableId,
    universe_id: StableId,
    waves: Vec<RawWave>,
}
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBudget {
    max_obligations_per_wave: u32,
    max_waves: u32,
}
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDeferred {
    id: StableId,
    reason: String,
}
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRisk {
    id: StableId,
    impact: String,
    likelihood: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWave {
    obligation_ids: Vec<StableId>,
    wave_index: u32,
}

fn validate_raw_limits(raw: &RawPlan) -> Result<()> {
    let text = |value: &str| validate_text(value);
    text(&raw.id.to_string())?;
    text(&raw.planner_input_hash.to_string())?;
    text(&raw.planner_policy_hash.to_string())?;
    text(&raw.planner_policy_version)?;
    text(&raw.snapshot_id.to_string())?;
    text(&raw.universe_id.to_string())?;
    let budget = PlanBudget::new(raw.budget.max_waves, raw.budget.max_obligations_per_wave)?;
    if raw.waves.len() > usize::try_from(budget.max_waves).unwrap_or(usize::MAX) {
        return Err(incomplete(
            "raw plan waves",
            budget.max_waves as usize,
            raw.waves.len(),
        ));
    }
    if raw.deferred.len() > PlannerPolicyV1::MAX_OBLIGATIONS {
        return Err(incomplete(
            "raw plan deferred",
            PlannerPolicyV1::MAX_OBLIGATIONS,
            raw.deferred.len(),
        ));
    }
    if raw.risk_breakdown.len() > PlannerPolicyV1::MAX_OBLIGATIONS {
        return Err(incomplete(
            "raw plan risks",
            PlannerPolicyV1::MAX_OBLIGATIONS,
            raw.risk_breakdown.len(),
        ));
    }
    let mut total = raw.deferred.len();
    for wave in &raw.waves {
        if wave.obligation_ids.len()
            > usize::try_from(budget.max_obligations_per_wave).unwrap_or(usize::MAX)
        {
            return Err(incomplete(
                "raw plan wave obligations",
                budget.max_obligations_per_wave as usize,
                wave.obligation_ids.len(),
            ));
        }
        total = total
            .checked_add(wave.obligation_ids.len())
            .ok_or_else(|| {
                incomplete(
                    "raw plan obligations",
                    PlannerPolicyV1::MAX_OBLIGATIONS,
                    usize::MAX,
                )
            })?;
        for id in &wave.obligation_ids {
            text(&id.to_string())?;
        }
    }
    if total > PlannerPolicyV1::MAX_OBLIGATIONS {
        return Err(incomplete(
            "raw plan obligations",
            PlannerPolicyV1::MAX_OBLIGATIONS,
            total,
        ));
    }
    for value in &raw.deferred {
        text(&value.id.to_string())?;
        text(&value.reason)?;
    }
    for value in &raw.risk_breakdown {
        text(&value.id.to_string())?;
        text(&value.impact)?;
        text(&value.likelihood)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn parse_reason(value: &str) -> Result<DeferralReason> {
    match value {
        "budget_exhausted" => Ok(DeferralReason::BudgetExhausted),
        "prerequisite_deferred" => Ok(DeferralReason::PrerequisiteDeferred),
        _ => Err(DomainError::Planning(PlanningError::InvalidInput)),
    }
}
#[allow(dead_code)]
fn parse_severity(value: &str) -> Result<Severity> {
    match value {
        "info" => Ok(Severity::Info),
        "low" => Ok(Severity::Low),
        "medium" => Ok(Severity::Medium),
        "high" => Ok(Severity::High),
        "critical" => Ok(Severity::Critical),
        _ => Err(DomainError::Planning(PlanningError::InvalidInput)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] =
        include_bytes!("../../../examples/double-submit-payment/program-space.json");

    fn id(value: &str) -> StableId {
        StableId::parse(value).unwrap()
    }

    fn aggregate() -> ReviewAggregate {
        aggregate_from_value(serde_json::from_slice(FIXTURE).expect("fixture JSON"))
    }

    fn aggregate_from_value(value: serde_json::Value) -> ReviewAggregate {
        let bytes = serde_json::to_vec(&value).expect("fixture serialization");
        let program = crate::ProgramSpace::from_json_slice(&bytes).expect("valid fixture program");
        let (universe, obligations) = crate::MvpRulePack::synthesize(&program)
            .expect("fixture synthesis")
            .into_parts();
        ReviewAggregate::new(program, universe, obligations).expect("valid fixture aggregate")
    }

    fn rewrite_string(value: &mut serde_json::Value, before: &str, after: &str) {
        match value {
            serde_json::Value::String(text) if text == before => *text = after.to_owned(),
            serde_json::Value::Array(values) => {
                for value in values {
                    rewrite_string(value, before, after);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    rewrite_string(value, before, after);
                }
            }
            _ => {}
        }
    }

    fn assert_invalid_plan(result: Result<ReviewPlan>) {
        assert!(matches!(
            result,
            Err(DomainError::Planning(PlanningError::InvalidInput))
        ));
    }

    fn item(value: &str, dependencies: &[&str], weight: f64) -> PlannerObligation {
        PlannerObligation::new(
            id(value),
            dependencies.iter().map(|value| id(value)).collect(),
            weight,
            ObligationLifecycle::Generated,
        )
    }
    fn input(items: Vec<PlannerObligation>, waves: u32) -> PlannerInput {
        PlannerInput::new(
            PlanBudget::new(waves, 2).unwrap(),
            items,
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap()
    }

    fn items_with_edge_count(edge_count: usize) -> Vec<PlannerObligation> {
        let ids = (0..PlannerPolicyV1::MAX_OBLIGATIONS)
            .map(|index| id(&format!("obligation:{}", base62(index))))
            .collect::<Vec<_>>();
        let mut remaining = edge_count;
        let items = ids
            .iter()
            .enumerate()
            .map(|(index, identifier)| {
                let take = index.min(remaining);
                remaining -= take;
                PlannerObligation::new(
                    identifier.clone(),
                    ids[..take].to_vec(),
                    1.0,
                    ObligationLifecycle::Generated,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(remaining, 0, "fixture must realize requested edges");
        items
    }

    fn base62(mut value: usize) -> String {
        const DIGITS: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut reversed = Vec::new();
        loop {
            reversed.push(DIGITS[value % DIGITS.len()]);
            value /= DIGITS.len();
            if value == 0 {
                break;
            }
        }
        reversed.reverse();
        String::from_utf8(reversed).expect("ASCII base62")
    }

    fn padded_items(count: usize, extra_bytes: usize) -> Vec<PlannerObligation> {
        let bases = (0..count)
            .map(|index| format!("obligation:p{index:04}"))
            .collect::<Vec<_>>();
        let capacity = bases
            .iter()
            .map(|value| PlannerPolicyV1::MAX_STRING_BYTES - value.len())
            .sum::<usize>();
        assert!(
            extra_bytes <= capacity,
            "padding fixture has enough capacity"
        );
        let mut remaining = extra_bytes;
        let items = bases
            .into_iter()
            .map(|mut value| {
                let take = remaining.min(PlannerPolicyV1::MAX_STRING_BYTES - value.len());
                value.push_str(&"x".repeat(take));
                remaining -= take;
                PlannerObligation::new(id(&value), Vec::new(), 1.0, ObligationLifecycle::Generated)
            })
            .collect::<Vec<_>>();
        assert_eq!(remaining, 0, "all requested padding is assigned");
        items
    }

    fn padded_snapshot(extra_bytes: usize) -> StableId {
        id(&format!("snapshot:test{}", "x".repeat(extra_bytes)))
    }
    #[test]
    fn empty_plan_is_deterministic() {
        let one = plan_input(input(vec![], 1)).unwrap();
        let two = plan_input(input(vec![], 1)).unwrap();
        assert_eq!(one.id(), two.id());
        assert!(one.waves().is_empty());
    }
    #[test]
    fn respects_dependency_waves_and_priority() {
        let result = plan_input(input(
            vec![
                item("obligation:a", &[], 1.0),
                item("obligation:b", &["obligation:a"], 3.0),
                item("obligation:c", &[], 2.0),
            ],
            2,
        ))
        .unwrap();
        assert_eq!(
            result.waves()[0].obligation_ids(),
            &[id("obligation:c"), id("obligation:a")]
        );
        assert_eq!(result.waves()[1].obligation_ids(), &[id("obligation:b")]);
    }
    #[test]
    fn cycle_and_dangling_are_rejected() {
        assert!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![item("obligation:a", &["obligation:missing"], 1.0)],
                id("snapshot:test"),
                id("universe:test")
            )
            .is_err()
        );
        let cyclic = input(
            vec![
                item("obligation:a", &["obligation:b"], 1.0),
                item("obligation:b", &["obligation:a"], 1.0),
            ],
            2,
        );
        assert!(plan_input(cyclic).is_err());
    }
    #[test]
    fn budget_defers_with_closed_reasons() {
        let result = plan_input(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![
                    item("obligation:a", &[], 2.0),
                    item("obligation:b", &[], 1.0),
                    item("obligation:c", &["obligation:b"], 1.0),
                ],
                id("snapshot:test"),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            result.deferred().get(&id("obligation:b")),
            Some(&DeferralReason::BudgetExhausted)
        );
        assert_eq!(
            result.deferred().get(&id("obligation:c")),
            Some(&DeferralReason::PrerequisiteDeferred)
        );
    }
    #[test]
    fn input_hash_uses_ieee_bits_and_policy_is_stable() {
        let input = input(vec![item("obligation:a", &[], 0.5)], 1);
        assert!(
            String::from_utf8(input.canonical_bytes().unwrap())
                .unwrap()
                .contains("3fe0000000000000")
        );
        assert_eq!(
            PlannerPolicyV1::baseline().hash().unwrap().to_string(),
            "sha256:c34a2aca00cc3711148e482f89dafc6be9bca58f4cab55950f1c01f3b841f2fc"
        );
    }

    #[test]
    fn serialized_wire_is_the_canonical_plan_shape_without_wave_ids() {
        let result = plan_input(input(vec![item("obligation:a", &[], 1.0)], 1)).unwrap();
        assert_eq!(
            crate::canonical_json(&result).unwrap(),
            result.canonical_bytes().unwrap()
        );
        let text = String::from_utf8(result.canonical_bytes().unwrap()).unwrap();
        assert!(!text.contains("schedule-wave:"));
        assert!(
            result.waves()[0]
                .id()
                .to_string()
                .starts_with("schedule-wave:")
        );
    }

    #[test]
    fn persisted_wire_tamper_shapes_are_not_canonical() {
        let plan = plan_input(input(vec![item("obligation:a", &[], 1.0)], 1)).unwrap();
        let bytes = plan.canonical_bytes().unwrap();
        let mut tampered = bytes.clone();
        tampered[2] = b'z';
        assert_ne!(tampered, plan.canonical_bytes().unwrap());
        let mut noncanonical = bytes.clone();
        noncanonical.insert(0, b' ');
        assert_ne!(noncanonical, plan.canonical_bytes().unwrap());
    }

    #[test]
    fn risk_boundaries_invalid_weights_and_duplicate_dependencies_are_closed() {
        let cases = [
            (0.000_000_1, Severity::Info),
            (0.25, Severity::Info),
            (0.250_001, Severity::Low),
            (0.5, Severity::Low),
            (0.500_001, Severity::Medium),
            (1.0, Severity::Medium),
            (1.000_001, Severity::High),
            (2.0, Severity::High),
            (2.000_001, Severity::Critical),
        ];
        for (index, (weight, expected)) in cases.into_iter().enumerate() {
            let identifier = id(&format!("obligation:r{index}"));
            let result = plan_input(input(
                vec![PlannerObligation::new(
                    identifier.clone(),
                    vec![],
                    weight,
                    ObligationLifecycle::Generated,
                )],
                1,
            ))
            .unwrap();
            assert_eq!(result.risk_breakdown()[&identifier].impact(), expected);
        }
        for weight in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(
                PlannerInput::new(
                    PlanBudget::new(1, 1).unwrap(),
                    vec![PlannerObligation::new(
                        id("obligation:bad"),
                        vec![],
                        weight,
                        ObligationLifecycle::Generated
                    )],
                    id("snapshot:test"),
                    id("universe:test")
                )
                .is_err()
            );
        }
        assert!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![PlannerObligation::new(
                    id("obligation:dup"),
                    vec![id("obligation:x"), id("obligation:x")],
                    1.0,
                    ObligationLifecycle::Generated
                )],
                id("snapshot:test"),
                id("universe:test")
            )
            .is_err()
        );
        assert!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![PlannerObligation::new(
                    id("obligation:state"),
                    vec![],
                    1.0,
                    ObligationLifecycle::Planned
                )],
                id("snapshot:test"),
                id("universe:test")
            )
            .is_err()
        );
    }

    #[test]
    fn public_plan_round_trips_against_the_exact_aggregate() {
        let aggregate = aggregate();
        let planned = plan(&aggregate, PlanBudget::new(16, 2).unwrap()).unwrap();
        let bytes = planned.canonical_bytes().unwrap();
        let decoded = ReviewPlan::from_canonical_bytes(&bytes, &aggregate).unwrap();

        assert_eq!(decoded.canonical_bytes().unwrap(), bytes);
        assert_eq!(decoded.id(), planned.id());
        assert_eq!(decoded.universe_id(), aggregate.universe().id());
        assert_eq!(decoded.snapshot_id(), aggregate.program().snapshot_id());
        assert!(
            decoded.waves().len() > 1,
            "fixture exercises dependency waves"
        );
        assert!(
            decoded.risk_breakdown().len() > 1,
            "fixture exercises multiple prioritized obligations"
        );
        assert!(decoded.waves().iter().all(|wave| {
            wave.id().kind() == "schedule-wave" && !wave.obligation_ids().is_empty()
        }));
    }

    #[test]
    fn canonical_risk_and_deferral_tampering_is_rejected_by_aggregate_validation() {
        let aggregate = aggregate();
        let planned = plan(&aggregate, PlanBudget::new(1, 1).unwrap()).unwrap();
        let bytes = planned.canonical_bytes().unwrap();

        let mut risk_tamper: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let risk = risk_tamper["risk_breakdown"]
            .as_array_mut()
            .and_then(|items| items.first_mut())
            .expect("non-empty fixture risk breakdown");
        risk["impact"] = serde_json::Value::String(
            if risk["impact"] == "info" {
                "critical"
            } else {
                "info"
            }
            .to_owned(),
        );
        assert_invalid_plan(ReviewPlan::from_canonical_bytes(
            &crate::canonical_json(&risk_tamper).unwrap(),
            &aggregate,
        ));

        let mut deferral_tamper: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let deferred = deferral_tamper["deferred"]
            .as_array_mut()
            .and_then(|items| items.first_mut())
            .expect("one-wave fixture must defer obligations");
        deferred["reason"] = serde_json::Value::String(
            if deferred["reason"] == "budget_exhausted" {
                "prerequisite_deferred"
            } else {
                "budget_exhausted"
            }
            .to_owned(),
        );
        assert_invalid_plan(ReviewPlan::from_canonical_bytes(
            &crate::canonical_json(&deferral_tamper).unwrap(),
            &aggregate,
        ));
    }

    #[test]
    fn canonical_plan_rejects_other_denominators_and_snapshots() {
        let aggregate = aggregate();
        let planned = plan(&aggregate, PlanBudget::new(16, 2).unwrap()).unwrap();
        let bytes = planned.canonical_bytes().unwrap();

        let mut denominator_fixture: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        let changed_public_symbol = denominator_fixture["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["attributes"]["public"] == true)
            .expect("changed public fixture symbol");
        changed_public_symbol["attributes"]["public"] = serde_json::Value::Bool(false);
        let other_denominator = aggregate_from_value(denominator_fixture);
        assert_eq!(
            other_denominator.program().snapshot_id(),
            aggregate.program().snapshot_id()
        );
        assert_ne!(
            other_denominator.universe().obligation_ids(),
            aggregate.universe().obligation_ids()
        );
        assert_invalid_plan(ReviewPlan::from_canonical_bytes(&bytes, &other_denominator));

        let mut snapshot_fixture: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        rewrite_string(
            &mut snapshot_fixture,
            "snapshot:double-submit-v1",
            "snapshot:double-submit-v2",
        );
        let other_snapshot = aggregate_from_value(snapshot_fixture);
        assert_ne!(
            other_snapshot.program().snapshot_id(),
            aggregate.program().snapshot_id()
        );
        assert_invalid_plan(ReviewPlan::from_canonical_bytes(&bytes, &other_snapshot));
    }

    #[test]
    fn a_separate_cycle_is_not_hidden_by_ready_work_or_budget() {
        let mixed = PlannerInput::new(
            PlanBudget::new(1, 1).unwrap(),
            vec![
                item("obligation:ready", &[], 5.0),
                item("obligation:cycle-a", &["obligation:cycle-b"], 1.0),
                item("obligation:cycle-b", &["obligation:cycle-a"], 1.0),
            ],
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();

        assert!(matches!(
            plan_input(mixed),
            Err(DomainError::Planning(PlanningError::Cycle))
        ));
    }

    #[test]
    fn priority_orders_by_impact_then_depth_then_stable_id() {
        let impact = plan_input(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![
                    item("obligation:a-low", &[], 1.0),
                    item("obligation:z-high", &[], 3.0),
                ],
                id("snapshot:test"),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            impact.waves()[0].obligation_ids(),
            &[id("obligation:z-high")]
        );

        let depth = plan_input(
            PlannerInput::new(
                PlanBudget::new(2, 1).unwrap(),
                vec![
                    item("obligation:z-root", &[], 3.0),
                    item("obligation:z-deep", &["obligation:z-root"], 1.0),
                    item("obligation:a-flat", &[], 1.0),
                ],
                id("snapshot:test"),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            depth.waves()[0].obligation_ids(),
            &[id("obligation:z-root")]
        );
        assert_eq!(
            depth.waves()[1].obligation_ids(),
            &[id("obligation:z-deep")],
            "the deeper ready item precedes the lexically smaller flat item"
        );

        let stable_id = plan_input(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![
                    item("obligation:z", &[], 1.0),
                    item("obligation:a", &[], 1.0),
                ],
                id("snapshot:test"),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(stable_id.waves()[0].obligation_ids(), &[id("obligation:a")]);
    }

    #[test]
    fn input_reordering_preserves_input_bytes_and_plan_identity() {
        let forward = vec![
            item("obligation:a", &[], 1.0),
            item("obligation:b", &["obligation:a"], 2.0),
            item("obligation:c", &["obligation:b", "obligation:a"], 3.0),
        ];
        let reverse = vec![
            item("obligation:c", &["obligation:a", "obligation:b"], 3.0),
            item("obligation:b", &["obligation:a"], 2.0),
            item("obligation:a", &[], 1.0),
        ];
        let forward = PlannerInput::new(
            PlanBudget::new(3, 2).unwrap(),
            forward,
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();
        let reverse = PlannerInput::new(
            PlanBudget::new(3, 2).unwrap(),
            reverse,
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();

        assert_eq!(
            forward.canonical_bytes().unwrap(),
            reverse.canonical_bytes().unwrap()
        );
        let forward = plan_input(forward).unwrap();
        let reverse = plan_input(reverse).unwrap();
        assert_eq!(forward.id(), reverse.id());
        assert_eq!(
            forward.canonical_bytes().unwrap(),
            reverse.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn plan_and_wave_ids_have_golden_namespaces_and_preimages() {
        let input = input(vec![item("obligation:a", &[], 1.0)], 1);
        const INPUT_PREIMAGE: &str = concat!(
            "{\"budget\":{\"max_obligations_per_wave\":2,\"max_waves\":1},",
            "\"obligations\":[{\"depends_on\":[],\"id\":\"obligation:a\",",
            "\"weight_ieee754_bits\":\"3ff0000000000000\"}],",
            "\"planner_policy_hash\":\"sha256:",
            "c34a2aca00cc3711148e482f89dafc6be9bca58f4cab55950f1c01f3b841f2fc\",",
            "\"snapshot_id\":\"snapshot:test\",\"universe_id\":\"universe:test\"}"
        );
        assert_eq!(input.canonical_bytes().unwrap(), INPUT_PREIMAGE.as_bytes());
        assert_eq!(
            input.hash().unwrap().to_string(),
            "sha256:a2c6c1ca034fd29ec91eac84ba8352119212b3a43266580f4a400355670d3195"
        );
        let mut identity = BoundedJson::new(PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES);
        write_plan_identity(
            &mut identity,
            &input,
            &input.hash().unwrap(),
            &[vec![id("obligation:a")]],
            &BTreeMap::new(),
        )
        .unwrap();
        const PLAN_PREIMAGE: &str = concat!(
            "{\"budget\":{\"max_obligations_per_wave\":2,\"max_waves\":1},",
            "\"deferred_ids\":[],\"planner_input_hash\":\"sha256:",
            "a2c6c1ca034fd29ec91eac84ba8352119212b3a43266580f4a400355670d3195\",",
            "\"planner_policy_hash\":\"sha256:",
            "c34a2aca00cc3711148e482f89dafc6be9bca58f4cab55950f1c01f3b841f2fc\",",
            "\"planner_policy_version\":\"scheduler.baseline@1\",",
            "\"snapshot_id\":\"snapshot:test\",\"universe_id\":\"universe:test\",",
            "\"waves\":[{\"obligation_ids\":[\"obligation:a\"],\"wave_index\":0}]}"
        );
        assert_eq!(identity.finish().unwrap(), PLAN_PREIMAGE.as_bytes());
        let planned = plan_input(input).unwrap();
        assert_eq!(
            planned.id().to_string(),
            "plan:sha256:ddd9257abba7657b3f82feada46b8abdbb4927a11e9c17f4b14bb5d6658c4957"
        );
        assert_eq!(
            planned.identity_body_hash().unwrap().to_string(),
            "sha256:ddd9257abba7657b3f82feada46b8abdbb4927a11e9c17f4b14bb5d6658c4957"
        );
        assert_eq!(
            planned.waves()[0].id().to_string(),
            "schedule-wave:sha256:76ae53b3f2bbaceb1f83727c4aa9e19a73f1aa3e885aebbeea560bfe3fc030ab"
        );
        const WAVE_PREIMAGE: &str = concat!(
            "{\"ids\":[\"obligation:a\"],\"plan_id\":\"plan:sha256:",
            "ddd9257abba7657b3f82feada46b8abdbb4927a11e9c17f4b14bb5d6658c4957\",",
            "\"wave_index\":0}"
        );
        assert_eq!(
            ContentHash::sha256(WAVE_PREIMAGE.as_bytes()).to_string(),
            "sha256:76ae53b3f2bbaceb1f83727c4aa9e19a73f1aa3e885aebbeea560bfe3fc030ab"
        );
    }

    #[test]
    fn planner_rejects_non_obligation_namespaces_before_normalization() {
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![PlannerObligation::new(
                    id("other:a"),
                    Vec::new(),
                    1.0,
                    ObligationLifecycle::Generated,
                )],
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Planning(PlanningError::InvalidIdKind))
        ));
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![PlannerObligation::new(
                    id("obligation:a"),
                    vec![id("other:dependency")],
                    1.0,
                    ObligationLifecycle::Generated,
                )],
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Planning(PlanningError::InvalidIdKind))
        ));

        let mut exact = "obligation:".to_owned();
        exact.push_str(&"x".repeat(PlannerPolicyV1::MAX_STRING_BYTES - exact.len()));
        let exact = PlannerInput::new(
            PlanBudget::new(1, 1).unwrap(),
            vec![PlannerObligation::new(
                id(&exact),
                Vec::new(),
                1.0,
                ObligationLifecycle::Generated,
            )],
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();
        let planned = plan_input(exact).unwrap();
        assert_eq!(planned.id().kind(), "plan");
        assert_eq!(planned.waves()[0].id().kind(), "schedule-wave");

        let mut too_long = "obligation:".to_owned();
        too_long.push_str(&"x".repeat(PlannerPolicyV1::MAX_STRING_BYTES + 1 - too_long.len()));
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![PlannerObligation::new(
                    id(&too_long),
                    Vec::new(),
                    1.0,
                    ObligationLifecycle::Generated,
                )],
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Incomplete {
                operation: "planner canonical string",
                limit: PlannerPolicyV1::MAX_STRING_BYTES,
                observed,
            }) if observed == PlannerPolicyV1::MAX_STRING_BYTES + 1
        ));
    }

    #[test]
    fn production_canonical_input_and_plan_caps_are_exact() {
        const INPUT_ITEMS: usize = 64;
        let input_budget = PlanBudget::new(1, INPUT_ITEMS as u32).unwrap();
        let baseline_input = PlannerInput::new(
            input_budget,
            padded_items(INPUT_ITEMS, 0),
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();
        let input_padding = PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES
            - baseline_input.canonical_bytes().unwrap().len();
        let exact_input = PlannerInput::new(
            input_budget,
            padded_items(INPUT_ITEMS, input_padding),
            id("snapshot:test"),
            id("universe:test"),
        )
        .unwrap();
        assert_eq!(
            exact_input.canonical_bytes().unwrap().len(),
            PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES
        );
        assert!(matches!(
            PlannerInput::new(
                input_budget,
                padded_items(INPUT_ITEMS, input_padding + 1),
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Incomplete {
                operation: "planner canonical bytes",
                limit: PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES,
                observed,
            }) if observed == PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES + 1
        ));

        const PLAN_ITEMS: usize = 24;
        let plan_budget = PlanBudget::new(1, PLAN_ITEMS as u32).unwrap();
        let baseline_plan = plan_input(
            PlannerInput::new(
                plan_budget,
                padded_items(PLAN_ITEMS, 0),
                padded_snapshot(0),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        let initial_deficit = PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES
            - baseline_plan.canonical_bytes().unwrap().len();
        let snapshot_padding = initial_deficit % 2;
        let parity_adjusted_plan = plan_input(
            PlannerInput::new(
                plan_budget,
                padded_items(PLAN_ITEMS, 0),
                padded_snapshot(snapshot_padding),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        let remaining = PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES
            - parity_adjusted_plan.canonical_bytes().unwrap().len();
        assert_eq!(remaining % 2, 0, "snapshot padding adjusts plan parity");
        let plan_id_padding = remaining / 2;
        let exact_plan = plan_input(
            PlannerInput::new(
                plan_budget,
                padded_items(PLAN_ITEMS, plan_id_padding),
                padded_snapshot(snapshot_padding),
                id("universe:test"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            exact_plan.canonical_bytes().unwrap().len(),
            PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES
        );
        assert!(matches!(
            plan_input(
                PlannerInput::new(
                    plan_budget,
                    padded_items(PLAN_ITEMS, plan_id_padding),
                    padded_snapshot(snapshot_padding + 1),
                    id("universe:test"),
                )
                .unwrap(),
            ),
            Err(DomainError::Incomplete {
                operation: "planner canonical bytes",
                limit: PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES,
                observed,
            }) if observed == PlannerPolicyV1::MAX_CANONICAL_PLAN_BYTES + 1
        ));
    }

    #[test]
    fn public_plan_rejects_an_oversized_universe_before_collecting() {
        let mut fixture: serde_json::Value = serde_json::from_slice(FIXTURE).unwrap();
        let template = fixture["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["id"] == "function:checkout-submit")
            .unwrap()
            .clone();
        let artifacts = fixture["artifacts"].as_array_mut().unwrap();
        for index in 0..PlannerPolicyV1::MAX_OBLIGATIONS {
            let mut artifact = template.clone();
            artifact["id"] = serde_json::Value::String(format!("function:planner-cap-{index}"));
            artifact["label"] = serde_json::Value::String(format!("planner_cap_{index}"));
            artifact["attributes"]["changed"] = serde_json::Value::Bool(true);
            artifacts.push(artifact);
        }
        let aggregate = aggregate_from_value(fixture);
        let observed = aggregate.universe().obligation_ids().len();
        assert!(observed > PlannerPolicyV1::MAX_OBLIGATIONS);
        assert!(matches!(
            plan(&aggregate, PlanBudget::new(1, 1).unwrap()),
            Err(DomainError::Incomplete {
                operation: "planner input obligations",
                limit: PlannerPolicyV1::MAX_OBLIGATIONS,
                observed: actual,
            }) if actual == observed
        ));
    }

    #[test]
    fn fixed_caps_accept_exact_limits_and_reject_plus_one() {
        assert!(
            PlanBudget::new(
                PlannerPolicyV1::MAX_WAVES as u32,
                PlannerPolicyV1::MAX_OBLIGATIONS_PER_WAVE as u32,
            )
            .is_ok()
        );
        assert!(matches!(
            PlanBudget::new(PlannerPolicyV1::MAX_WAVES as u32 + 1, 1),
            Err(DomainError::Planning(PlanningError::InvalidBudget))
        ));
        assert!(matches!(
            PlanBudget::new(1, PlannerPolicyV1::MAX_OBLIGATIONS_PER_WAVE as u32 + 1),
            Err(DomainError::Planning(PlanningError::InvalidBudget))
        ));

        let exact_obligations = (0..PlannerPolicyV1::MAX_OBLIGATIONS)
            .map(|index| {
                PlannerObligation::new(
                    id(&format!("obligation:o{index}")),
                    vec![],
                    1.0,
                    ObligationLifecycle::Generated,
                )
            })
            .collect::<Vec<_>>();
        assert!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                exact_obligations.clone(),
                id("snapshot:test"),
                id("universe:test"),
            )
            .is_ok()
        );
        let mut too_many_obligations = exact_obligations;
        too_many_obligations.push(item("obligation:overflow", &[], 1.0));
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                too_many_obligations,
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Incomplete {
                operation: "planner input obligations",
                limit: PlannerPolicyV1::MAX_OBLIGATIONS,
                observed,
            }) if observed == PlannerPolicyV1::MAX_OBLIGATIONS + 1
        ));

        // With the required `obligation:` namespace, 65,536 distinct edge
        // references exceed the independent canonical-byte cap. Reaching the
        // serializer (rather than the edge check) proves the edge count itself
        // is accepted; the +1 case below must fail at the edge boundary first.
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                items_with_edge_count(PlannerPolicyV1::MAX_EDGES),
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Incomplete {
                operation: "planner canonical bytes",
                limit: PlannerPolicyV1::MAX_CANONICAL_INPUT_BYTES,
                ..
            })
        ));
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                items_with_edge_count(PlannerPolicyV1::MAX_EDGES + 1),
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Incomplete {
                operation: "planner input edges",
                limit: PlannerPolicyV1::MAX_EDGES,
                observed,
            }) if observed == PlannerPolicyV1::MAX_EDGES + 1
        ));

        assert!(validate_text(&"x".repeat(PlannerPolicyV1::MAX_STRING_BYTES)).is_ok());
        assert!(matches!(
            validate_text(&"x".repeat(PlannerPolicyV1::MAX_STRING_BYTES + 1)),
            Err(DomainError::Incomplete {
                operation: "planner canonical string",
                limit: PlannerPolicyV1::MAX_STRING_BYTES,
                observed,
            }) if observed == PlannerPolicyV1::MAX_STRING_BYTES + 1
        ));

        let mut writer = BoundedJson::new(8);
        writer.push("12345678").unwrap();
        assert!(matches!(
            writer.push("9"),
            Err(DomainError::Incomplete {
                operation: "planner canonical bytes",
                limit: 8,
                observed: 9,
            })
        ));
    }

    #[test]
    fn duplicate_inputs_have_closed_typed_failures() {
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![
                    item("obligation:a", &[], 1.0),
                    item("obligation:a", &[], 1.0),
                ],
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Planning(PlanningError::DuplicateObligation))
        ));
        assert!(matches!(
            PlannerInput::new(
                PlanBudget::new(1, 1).unwrap(),
                vec![
                    item("obligation:a", &[], 1.0),
                    item("obligation:b", &["obligation:a", "obligation:a"], 1.0,),
                ],
                id("snapshot:test"),
                id("universe:test"),
            ),
            Err(DomainError::Planning(PlanningError::DuplicateDependency))
        ));
    }
}
