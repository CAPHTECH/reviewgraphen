//! Frozen TypeScript rule vocabulary for G1-S6.
//!
//! The design requires the arm's rule, property,
//! target, and coverage fields to be validated as a coupled, closed vocabulary.

use crate::source_review::reasons::VocabularyError;

fn rejected_wire(wire: &str) -> VocabularyError {
    VocabularyError {
        rejected_wire: wire.to_owned(),
    }
}

/// Registered TypeScript rule IDs. Wire strings must use [`RuleId::parse_wire`]
/// and may not be accepted as arbitrary values.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RuleId {
    ChangedPublicCallee,
    PublicFunctionContract,
    CapabilityGapOrigin,
}

impl RuleId {
    /// Parses a registered rule ID or rejects a foreign rule vocabulary item.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "relation.changed_public_callee@2" => Ok(Self::ChangedPublicCallee),
            "node.public_function_contract@2" => Ok(Self::PublicFunctionContract),
            "capability_gap.origin_rule@1" => Ok(Self::CapabilityGapOrigin),
            _ => Err(rejected_wire(wire)),
        }
    }

    /// Emits the registered wire spelling for a typed rule ID.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::ChangedPublicCallee => "relation.changed_public_callee@2",
            Self::PublicFunctionContract => "node.public_function_contract@2",
            Self::CapabilityGapOrigin => "capability_gap.origin_rule@1",
        }
    }
}

/// Registered TypeScript property IDs coupled to [`RuleId`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum PropertyId {
    CalleeContractReview,
    PublicFunctionContractReview,
    CapabilityGap,
}

impl PropertyId {
    /// Parses a registered property ID or rejects a foreign vocabulary item.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "typescript.callee_contract_review@1" => Ok(Self::CalleeContractReview),
            "typescript.public_function_contract_review@1" => {
                Ok(Self::PublicFunctionContractReview)
            }
            "reviewgraphen.capability_gap" => Ok(Self::CapabilityGap),
            _ => Err(rejected_wire(wire)),
        }
    }

    /// Emits the registered wire spelling for a typed property ID.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::CalleeContractReview => "typescript.callee_contract_review@1",
            Self::PublicFunctionContractReview => "typescript.public_function_contract_review@1",
            Self::CapabilityGap => "reviewgraphen.capability_gap",
        }
    }
}

/// The closed target vocabulary of the frozen TypeScript rule arm.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TargetKind {
    Node,
    Relation,
    Subgraph,
}

impl TargetKind {
    /// Parses a registered target-kind spelling or rejects foreign text, so
    /// acceptance can check the wire/enum mapping in both directions
    ///.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "node" => Ok(Self::Node),
            "relation" => Ok(Self::Relation),
            "subgraph" => Ok(Self::Subgraph),
            _ => Err(rejected_wire(wire)),
        }
    }

    /// Emits the registered target-kind spelling for fixture comparison.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Relation => "relation",
            Self::Subgraph => "subgraph",
        }
    }
}

/// The coverage layer must agree with the registered arm, rather than being a
/// caller-selected string.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CoverageLayer {
    SingleLayer,
    TwoLayer,
    SnapshotGap,
}

impl CoverageLayer {
    /// Parses a registered coverage-layer spelling or rejects foreign text,
    /// preserving the closed arm vocabulary.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "single_layer" => Ok(Self::SingleLayer),
            "two_layer" => Ok(Self::TwoLayer),
            "snapshot_gap" => Ok(Self::SnapshotGap),
            _ => Err(rejected_wire(wire)),
        }
    }

    /// Emits the registered coverage-layer spelling for fixture comparison.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::SingleLayer => "single_layer",
            Self::TwoLayer => "two_layer",
            Self::SnapshotGap => "snapshot_gap",
        }
    }
}

/// One registry-bound arm. Its fields are types, not independent text fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuleArm {
    pub name: &'static str,
    pub rule_id: RuleId,
    pub property_id: PropertyId,
    pub target_kind: TargetKind,
    pub weight_milli: u32,
    pub coverage_layer: CoverageLayer,
    pub origin_rule_id: Option<RuleId>,
}

pub const D_RULE_ID: RuleId = RuleId::ChangedPublicCallee;
pub const D_PROPERTY_ID: PropertyId = PropertyId::CalleeContractReview;
pub const N_RULE_ID: RuleId = RuleId::PublicFunctionContract;
pub const N_PROPERTY_ID: PropertyId = PropertyId::PublicFunctionContractReview;
pub const GAP_RULE_ID: RuleId = RuleId::CapabilityGapOrigin;
pub const GAP_PROPERTY_ID: PropertyId = PropertyId::CapabilityGap;

/// The only three rule/property/target/coverage combinations in registry r1.
/// Their frozen values match the design and registry.r1.json.
pub const TS_RULE_ARMS: [RuleArm; 3] = [
    RuleArm {
        name: "D",
        rule_id: D_RULE_ID,
        property_id: D_PROPERTY_ID,
        target_kind: TargetKind::Relation,
        weight_milli: 4000,
        coverage_layer: CoverageLayer::TwoLayer,
        origin_rule_id: None,
    },
    RuleArm {
        name: "Node",
        rule_id: N_RULE_ID,
        property_id: N_PROPERTY_ID,
        target_kind: TargetKind::Node,
        weight_milli: 3000,
        coverage_layer: CoverageLayer::SingleLayer,
        origin_rule_id: None,
    },
    RuleArm {
        name: "gap",
        rule_id: GAP_RULE_ID,
        property_id: GAP_PROPERTY_ID,
        target_kind: TargetKind::Subgraph,
        weight_milli: 4000,
        coverage_layer: CoverageLayer::SnapshotGap,
        origin_rule_id: Some(D_RULE_ID),
    },
];

/// Returns the complete registered arm for a typed rule ID.
#[must_use]
pub fn rule_arm(rule_id: RuleId) -> RuleArm {
    // `TS_RULE_ARMS` registers exactly one arm per `RuleId` variant.
    let index = match rule_id {
        RuleId::ChangedPublicCallee => 0,
        RuleId::PublicFunctionContract => 1,
        RuleId::CapabilityGapOrigin => 2,
    };
    let arm = TS_RULE_ARMS[index];
    debug_assert_eq!(arm.rule_id, rule_id);
    arm
}
