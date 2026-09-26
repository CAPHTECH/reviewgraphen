//! Closed TypeScript vocabulary for call reasons, kinds, and resolutions.
//!
//! The design binds the TypeScript arm to a
//! finite vocabulary and one precedence order.  This module is the only public
//! home of that precedence; callers cannot pass an unchecked reason string.

use super::registry::{TypeScriptRegistryBinding, validate_typescript_binding};
use std::collections::BTreeSet;
use std::fmt;

/// Every reason admitted by `typescript.call_reason_precedence@2`, in the
/// exact design order, plus the three closed D-pair
/// ineligibility reasons named by the design. The first sixteen variants
/// are the callsite policy; the final three only arise during D aggregation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum CallReason {
    ParseFailure,
    UnsupportedSyntax,
    UnsupportedCaller,
    DynamicDispatch,
    RelativeSpecifierUnsupported,
    RelativeTargetUnread,
    RelativeTargetAmbiguous,
    RelativeTargetExcluded,
    RelativeTargetMissing,
    ImportBindingAmbiguous,
    TypeOnlyBinding,
    ExportBindingUnsupported,
    ImportResolutionUnavailable,
    ShadowedBinding,
    WrittenBinding,
    UnresolvedName,
    UnchangedCallee,
    NonPublicCallee,
    ExcludedEndpoint,
}

impl CallReason {
    /// Emits the registered spelling retained for this variant. The first
    /// sixteen variants are call-codec values; the final three D values remain
    /// for aggregation compatibility and are converted to `DReasonV1` before
    /// they enter a non-call reason domain.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::ParseFailure => "parse_failure",
            Self::UnsupportedSyntax => "unsupported_syntax",
            Self::UnsupportedCaller => "unsupported_caller",
            Self::DynamicDispatch => "dynamic_dispatch",
            Self::RelativeSpecifierUnsupported => "relative_specifier_unsupported",
            Self::RelativeTargetUnread => "relative_target_unread",
            Self::RelativeTargetAmbiguous => "relative_target_ambiguous",
            Self::RelativeTargetExcluded => "relative_target_excluded",
            Self::RelativeTargetMissing => "relative_target_missing",
            Self::ImportBindingAmbiguous => "import_binding_ambiguous",
            Self::TypeOnlyBinding => "type_only_binding",
            Self::ExportBindingUnsupported => "export_binding_unsupported",
            Self::ImportResolutionUnavailable => "import_resolution_unavailable",
            Self::ShadowedBinding => "shadowed_binding",
            Self::WrittenBinding => "written_binding",
            Self::UnresolvedName => "unresolved_name",
            Self::UnchangedCallee => "unchanged_callee",
            Self::NonPublicCallee => "non_public_callee",
            Self::ExcludedEndpoint => "excluded_endpoint",
        }
    }

    /// Parses one call-codec wire spelling. D-only spellings are not admitted
    /// through this call codec.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "parse_failure" => Ok(Self::ParseFailure),
            "unsupported_syntax" => Ok(Self::UnsupportedSyntax),
            "unsupported_caller" => Ok(Self::UnsupportedCaller),
            "dynamic_dispatch" => Ok(Self::DynamicDispatch),
            "relative_specifier_unsupported" => Ok(Self::RelativeSpecifierUnsupported),
            "relative_target_unread" => Ok(Self::RelativeTargetUnread),
            "relative_target_ambiguous" => Ok(Self::RelativeTargetAmbiguous),
            "relative_target_excluded" => Ok(Self::RelativeTargetExcluded),
            "relative_target_missing" => Ok(Self::RelativeTargetMissing),
            "import_binding_ambiguous" => Ok(Self::ImportBindingAmbiguous),
            "type_only_binding" => Ok(Self::TypeOnlyBinding),
            "export_binding_unsupported" => Ok(Self::ExportBindingUnsupported),
            "import_resolution_unavailable" => Ok(Self::ImportResolutionUnavailable),
            "shadowed_binding" => Ok(Self::ShadowedBinding),
            "written_binding" => Ok(Self::WrittenBinding),
            "unresolved_name" => Ok(Self::UnresolvedName),
            _ => Err(VocabularyError::rejected(wire)),
        }
    }

    /// Returns the retained precedence index. The D-only values do not define
    /// the precedence policy for `DReasonSetV1`.
    #[must_use]
    pub fn precedence(self) -> u8 {
        self.call_precedence().unwrap_or(u8::MAX)
    }

    fn call_precedence(self) -> Option<u8> {
        match self {
            Self::ParseFailure => Some(0),
            Self::UnsupportedSyntax => Some(1),
            Self::UnsupportedCaller => Some(2),
            Self::DynamicDispatch => Some(3),
            Self::RelativeSpecifierUnsupported => Some(4),
            Self::RelativeTargetUnread => Some(5),
            Self::RelativeTargetAmbiguous => Some(6),
            Self::RelativeTargetExcluded => Some(7),
            Self::RelativeTargetMissing => Some(8),
            Self::ImportBindingAmbiguous => Some(9),
            Self::TypeOnlyBinding => Some(10),
            Self::ExportBindingUnsupported => Some(11),
            Self::ImportResolutionUnavailable => Some(12),
            Self::ShadowedBinding => Some(13),
            Self::WrittenBinding => Some(14),
            Self::UnresolvedName => Some(15),
            Self::UnchangedCallee | Self::NonPublicCallee | Self::ExcludedEndpoint => None,
        }
    }
}

/// The only direct-call resolutions admitted by the first TypeScript arm
///.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ResolutionKind {
    SyntacticUnique,
    SyntacticUniqueRelativeImportV1,
}

impl ResolutionKind {
    /// Emits the sole registered wire literal for this resolution kind
    ///.
    #[must_use]
    pub fn wire_literal(self) -> &'static str {
        match self {
            Self::SyntacticUnique => "syntactic_unique",
            Self::SyntacticUniqueRelativeImportV1 => "syntactic_unique_relative_import@1",
        }
    }

    /// Parses a registered resolution or rejects every foreign spelling.
    pub fn parse_wire(wire: &str) -> Result<Self, VocabularyError> {
        match wire {
            "syntactic_unique" => Ok(Self::SyntacticUnique),
            "syntactic_unique_relative_import@1" => Ok(Self::SyntacticUniqueRelativeImportV1),
            _ => Err(VocabularyError::rejected(wire)),
        }
    }
}

/// A bound TypeScript syntax-kind token. Its registered literal is opaque so
/// the public Rust enum does not invent the unread, versioned kind vocabulary
///.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct TypeScriptSyntaxKind(String);

impl TypeScriptSyntaxKind {
    /// Parses a registered kind ID or rejects a foreign payload vocabulary item.
    pub fn parse_wire(_wire: &str) -> Result<Self, VocabularyError> {
        if matches!(
            _wire,
            "program"
                | "variable_declarator"
                | "function_declaration"
                | "generator_function_declaration"
                | "generator_function"
                | "function_expression"
                | "arrow_function"
                | "call_expression"
                | "import_specifier"
                | "identifier"
                | "namespace_import"
                | "export_statement"
        ) {
            Ok(Self(_wire.to_owned()))
        } else {
            Err(VocabularyError {
                rejected_wire: _wire.to_owned(),
            })
        }
    }

    /// Returns the fixed-registry syntax-kind literal.
    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod syntax_kind_tests {
    use super::TypeScriptSyntaxKind;

    #[test]
    fn syntax_kind_rejects_foreign_node_name() {
        assert!(TypeScriptSyntaxKind::parse_wire("foreign_node").is_err());
    }
    #[test]
    fn syntax_kind_accepts_exactly_the_collector_node_names() {
        // V0 and V0b RESULT lists: parse_wire accepts only node names the native
        // collector emits, never another grammar node name.
        for wire in [
            "arrow_function",
            "call_expression",
            "export_statement",
            "function_declaration",
            "function_expression",
            "generator_function",
            "generator_function_declaration",
            "identifier",
            "import_specifier",
            "namespace_import",
            "program",
            "variable_declarator",
        ] {
            assert_eq!(
                TypeScriptSyntaxKind::parse_wire(wire)
                    .expect("collector node name")
                    .wire_literal(),
                wire
            );
        }
        for wire in [
            "class_declaration",
            "lexical_declaration",
            "method_definition",
        ] {
            assert!(
                TypeScriptSyntaxKind::parse_wire(wire).is_err(),
                "{wire} is not emitted by the collector"
            );
        }
    }
}

/// The two fixed resolution outcomes shared by call and binding payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionOutcomeV1 {
    Resolved,
    Unresolved,
}

/// The two fixed record outcomes shared by surface and scope payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordOutcomeV1 {
    Recorded,
    Unsupported,
}

/// A literal-known callable reason, without claiming a complete role vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownRoleReasonV1 {
    UnsupportedCallableKind,
    NotRuntimeCallable,
}

/// A literal-known D reason, without defining the complete D vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownDReasonV1 {
    UnchangedCallee,
    NonPublicCallee,
    ExcludedEndpoint,
}

/// Literal-known defer reasons retained as a partial convenience vocabulary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownDeferReasonV1 {
    ContextProjectionUnavailable,
    CandidateSpaceEnumerationIncomplete,
}

macro_rules! opaque_vocabulary_type {
    ($token:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $token(String);

        impl $token {
            pub fn parse_wire(
                _binding: &TypeScriptRegistryBinding,
                _wire: &str,
            ) -> Result<Self, VocabularyError> {
                if validate_typescript_binding(_binding).is_ok()
                    && opaque_vocabulary_wire_is_registered(stringify!($token), _wire)
                {
                    Ok(Self(_wire.to_owned()))
                } else {
                    Err(VocabularyError {
                        rejected_wire: _wire.to_owned(),
                    })
                }
            }

            #[must_use]
            pub fn wire_literal(&self) -> &str {
                &self.0
            }
        }
    };
}

macro_rules! opaque_reason_set_type {
    ($set:ident, $token:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $set(Vec<$token>);

        impl $set {
            pub fn new(
                _binding: &TypeScriptRegistryBinding,
                _reasons: Vec<$token>,
            ) -> Result<Self, VocabularyError> {
                if validate_typescript_binding(_binding).is_err() {
                    return Err(VocabularyError {
                        rejected_wire: _binding.registry_id.clone(),
                    });
                }
                let mut reasons = _reasons;
                reasons.sort_by(|left, right| left.0.cmp(&right.0));
                reasons.dedup_by(|left, right| left.0 == right.0);
                Ok(Self(reasons))
            }

            #[must_use]
            pub fn all(&self) -> &[$token] {
                &self.0
            }

            #[must_use]
            pub fn primary(&self) -> Option<&$token> {
                self.0.first()
            }

            #[must_use]
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }
    };
}

fn opaque_vocabulary_wire_is_registered(token: &str, wire: &str) -> bool {
    matches!((token, wire), ("CallableReasonV1", "not_runtime_callable"))
}

opaque_vocabulary_type!(CallableReasonV1);
opaque_vocabulary_type!(SurfaceReasonV1);
opaque_vocabulary_type!(ScopeReasonV1);
opaque_vocabulary_type!(DReasonV1);
opaque_vocabulary_type!(NodeObstructionReasonV1);
opaque_vocabulary_type!(ObligationDeferReasonV1);

/// The closed Binding-reason vocabulary carried by the validated r2 registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingReasonV1(String);

impl BindingReasonV1 {
    /// Parses a Binding reason only for the frozen TypeScript registry binding.
    pub fn parse_wire(
        binding: &TypeScriptRegistryBinding,
        wire: &str,
    ) -> Result<Self, VocabularyError> {
        validate_typescript_binding(binding).map_err(|_| VocabularyError::rejected(wire))?;
        Self::from_validated_wire(wire).ok_or_else(|| VocabularyError::rejected(wire))
    }

    pub(crate) fn from_validated_wire(wire: &str) -> Option<Self> {
        match wire {
            "export_binding_unsupported"
            | "import_binding_ambiguous"
            | "import_resolution_unavailable"
            | "parse_failure"
            | "relative_specifier_unsupported"
            | "relative_target_ambiguous"
            | "relative_target_excluded"
            | "relative_target_missing"
            | "relative_target_unread"
            | "type_only_binding"
            | "unsupported_syntax"
            | "written_binding" => Some(Self(wire.to_owned())),
            _ => None,
        }
    }

    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

/// The seven closed Binding-stage names carried by the validated r2 registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingStageV1(String);

impl BindingStageV1 {
    /// Parses a Binding stage only for the frozen TypeScript registry binding.
    pub fn parse_wire(
        binding: &TypeScriptRegistryBinding,
        wire: &str,
    ) -> Result<Self, VocabularyError> {
        validate_typescript_binding(binding).map_err(|_| VocabularyError::rejected(wire))?;
        Self::from_validated_wire(wire).ok_or_else(|| VocabularyError::rejected(wire))
    }

    pub(crate) fn from_validated_wire(wire: &str) -> Option<Self> {
        match wire {
            "b.form" | "b.local_uniqueness" | "b.specifier" | "b.candidates"
            | "b.export_binding" | "b.writes" | "b.result" => Some(Self(wire.to_owned())),
            _ => None,
        }
    }

    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

/// The ten closed Call-stage names carried by the validated r2 registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallStageV1(String);

impl CallStageV1 {
    /// Parses a Call stage only for the frozen TypeScript registry binding.
    pub fn parse_wire(
        binding: &TypeScriptRegistryBinding,
        wire: &str,
    ) -> Result<Self, VocabularyError> {
        validate_typescript_binding(binding).map_err(|_| VocabularyError::rejected(wire))?;
        Self::from_validated_wire(wire).ok_or_else(|| VocabularyError::rejected(wire))
    }

    pub(crate) fn from_validated_wire(wire: &str) -> Option<Self> {
        match wire {
            "c.syntax" | "c.caller" | "c.import_form" | "c.local_binding" | "c.specifier"
            | "c.candidates" | "c.export_binding" | "c.shadow" | "c.writes" | "c.resolution" => {
                Some(Self(wire.to_owned()))
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

opaque_reason_set_type!(CallableReasonSetV1, CallableReasonV1);
opaque_reason_set_type!(BindingReasonSetV1, BindingReasonV1);
opaque_reason_set_type!(SurfaceReasonSetV1, SurfaceReasonV1);
opaque_reason_set_type!(ScopeReasonSetV1, ScopeReasonV1);
opaque_reason_set_type!(DReasonSetV1, DReasonV1);
opaque_reason_set_type!(NodeObstructionReasonSetV1, NodeObstructionReasonV1);
opaque_reason_set_type!(ObligationDeferReasonSetV1, ObligationDeferReasonV1);

/// A finite, order-insensitive set of registered call reasons.
///
/// Storage is a set, while [`ReasonSet::primary`] applies the single frozen
/// precedence policy.  It is not the insertion order.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReasonSet {
    reasons: BTreeSet<CallReason>,
}

impl ReasonSet {
    /// Creates a set from typed reasons.  External wire text must first pass
    /// [`CallReason::parse_wire`], which can reject it.
    #[must_use]
    pub fn new(reasons: impl IntoIterator<Item = CallReason>) -> Self {
        Self {
            reasons: reasons.into_iter().collect(),
        }
    }

    /// Returns the full, order-independent reason set.
    #[must_use]
    pub fn all(&self) -> &BTreeSet<CallReason> {
        &self.reasons
    }

    /// Selects the one primary reason under the design; this is the sole
    /// public precedence selection point.
    #[must_use]
    pub fn primary(&self) -> Option<CallReason> {
        self.reasons
            .iter()
            .copied()
            .filter_map(|reason| {
                reason
                    .call_precedence()
                    .map(|precedence| (precedence, reason))
            })
            .min_by_key(|(precedence, _)| *precedence)
            .map(|(_, reason)| reason)
    }

    /// States whether the canonical reason set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.reasons.is_empty()
    }
}

/// Rejection for a wire spelling not represented by the registered vocabulary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VocabularyError {
    pub rejected_wire: String,
}

impl VocabularyError {
    fn rejected(wire: &str) -> Self {
        Self {
            rejected_wire: wire.to_owned(),
        }
    }
}

impl fmt::Display for VocabularyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unregistered vocabulary wire spelling `{}`",
            self.rejected_wire
        )
    }
}

impl std::error::Error for VocabularyError {}
