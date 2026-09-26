//! Typed TypeScript payload contracts for the first I2 cohort.
//!
//! Fixed literals remain enums; policy vocabulary that the admission design
//! does not enumerate is represented by binding-checked opaque values. These
//! DTOs are raw submissions until runtime A1 admits them.

use reviewgraphen_core::canonical_json;
use reviewgraphen_core::source_review::admitted_source::AdmittedSourceBundleV1;
use reviewgraphen_core::source_review::basis::{SourceFileOutcome, SourceSyntaxRole};
use reviewgraphen_core::source_review::ids::{
    AccountingMismatch, CallerId, DeclarationId, SnapshotBinding, SourceFileId, SourceHash,
    SourceRange, SourceWitnessKeyV1, SyntaxKeyV1,
};
use reviewgraphen_core::source_review::reasons::{
    BindingReasonSetV1, BindingReasonV1, BindingStageV1, CallReason, CallStageV1,
    CallableReasonSetV1, CallableReasonV1, ReasonSet, RecordOutcomeV1, ResolutionKind,
    ResolutionOutcomeV1, ScopeReasonSetV1, ScopeReasonV1, SurfaceReasonSetV1, SurfaceReasonV1,
    VocabularyError,
};
use reviewgraphen_core::source_review::registry::{
    TypeScriptRegistryBinding, validate_typescript_binding,
};
use serde_json::{Value, json};
use tree_sitter::{Node, Parser};

use super::calls::{
    CallerScope, ImportCallAttempt, RelativeCallInput, UnresolvedCall, evaluate_local_calls,
    resolve_relative_call, unresolved_import_call, unresolved_syntax_call,
};
use super::import_bindings::{
    CallableClassification, ImportBinding, ImportKind, collect_imports,
    collect_top_level_callables, resolve_export_binding,
};
use super::native_syntax::{NativeSyntaxRecord, collect_native_syntax};
use super::relative_paths::{RelativeEntry, RelativeEntryOutcome};

/// The five source-native syntax roles in the first cohort.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum SyntaxRole {
    Callable,
    Call,
    Binding,
    Surface,
    Scope,
}

/// Callable eligibility determined by the bound adapter policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallableOutcomeV1 {
    EligiblePublic,
    NonPublic,
    NotRuntimeCallable,
    OutOfScope,
    Unsupported,
}

/// Export visibility observed by the source adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisibilityValueV1 {
    Exported,
    NonExported,
    Unknown,
}

/// The fixed import form vocabulary for a binding payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadImportKindV1 {
    Named,
    Default,
    TypeOnly,
    Namespace,
}

/// The sole scope kind admitted in the first TypeScript cohort.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeKindV1 {
    FileLexical,
}

/// The source view used by deterministic payload rebuilding. A plain admitted
/// bundle is intentionally an untrusted draft view and always reports an
/// incomplete target tree. Runtime A1 supplies its private A0 view for the
/// only accepted reconstruction path.
pub trait TypeScriptPayloadSourceViewV1 {
    fn source_bundle(&self) -> &AdmittedSourceBundleV1;
    fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome>;
    fn target_inventory_complete(&self) -> bool;
}

impl TypeScriptPayloadSourceViewV1 for AdmittedSourceBundleV1 {
    fn source_bundle(&self) -> &AdmittedSourceBundleV1 {
        self
    }

    fn target_outcome_for_path(&self, canonical_path: &str) -> Option<SourceFileOutcome> {
        self.target_outcome_for_path(canonical_path)
    }

    fn target_inventory_complete(&self) -> bool {
        false
    }
}

/// A registered descriptor identity with no string constructor.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct PayloadDescriptorId(String);

impl PayloadDescriptorId {
    /// Parses one descriptor from the fixed initial TypeScript registry.
    pub fn parse_wire(_wire: &str) -> Result<Self, VocabularyError> {
        if matches!(
            _wire,
            "reviewgraphen.typescript_syntax.binding@1"
                | "reviewgraphen.typescript_syntax.call@1"
                | "reviewgraphen.typescript_syntax.callable@1"
                | "reviewgraphen.typescript_syntax.scope@1"
                | "reviewgraphen.typescript_syntax.surface@1"
        ) {
            Ok(Self(_wire.to_owned()))
        } else {
            Err(VocabularyError {
                rejected_wire: _wire.to_owned(),
            })
        }
    }

    /// Returns the binding-checked descriptor literal.
    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

/// A binding key whose spelling is validated against the bound registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadBindingKeyV1(String);

impl PayloadBindingKeyV1 {
    pub fn parse_wire(
        _binding: &TypeScriptRegistryBinding,
        _wire: &str,
    ) -> Result<Self, VocabularyError> {
        if validate_typescript_binding(_binding).is_ok() && !_wire.is_empty() {
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

/// An export-slot key whose spelling is validated against the bound registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadExportSlotV1(String);

impl PayloadExportSlotV1 {
    pub fn parse_wire(
        _binding: &TypeScriptRegistryBinding,
        _wire: &str,
    ) -> Result<Self, VocabularyError> {
        if validate_typescript_binding(_binding).is_ok() && !_wire.is_empty() {
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

/// One candidate path retained by a binding record, including rejected paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidatePathV1 {
    pub path: String,
    pub file_key: Option<SourceFileId>,
    pub rejection_witness: bool,
    pub unexpanded_ancestor_key: Option<SourceWitnessKeyV1>,
}

/// Required callable data, kept separate from policy reasons.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableDataV1 {
    pub binding_key: Option<PayloadBindingKeyV1>,
    pub declaration_range: SourceRange,
    pub implementation_range: Option<SourceRange>,
    pub visibility_value: VisibilityValueV1,
    pub binding_obstruction_keys: Vec<SourceWitnessKeyV1>,
}

/// Required import/export binding data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingDataV1 {
    pub local_name: String,
    pub import_kind: PayloadImportKindV1,
    pub export_slot: Option<PayloadExportSlotV1>,
    pub specifier: String,
    pub specifier_range: SourceRange,
    pub candidate_paths: Vec<CandidatePathV1>,
    pub resolved_function_id: Option<DeclarationId>,
    pub evaluated_stages: Vec<BindingStageV1>,
}

/// Required export-surface data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SurfaceDataV1 {
    pub export_slots: Vec<PayloadExportSlotV1>,
    pub local_names: Vec<String>,
    pub target_specifier: Option<String>,
    pub declaration_key: Option<SyntaxKeyV1>,
}

/// Required call-resolution data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallDataV1 {
    pub caller_id: Option<CallerId>,
    pub callee_id: Option<DeclarationId>,
    pub resolution_kind: Option<ResolutionKind>,
    pub binding_key: Option<PayloadBindingKeyV1>,
    pub candidate_witness_keys: Vec<SourceWitnessKeyV1>,
    pub evaluated_stages: Vec<CallStageV1>,
}

/// Required file-lexical scope data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScopeDataV1 {
    pub scope_kind: ScopeKindV1,
    pub member_keys: Vec<SyntaxKeyV1>,
}

/// The five and only five payload-data shapes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeScriptPayloadData {
    Callable(CallableDataV1),
    Binding(BindingDataV1),
    Surface(SurfaceDataV1),
    Call(CallDataV1),
    Scope(ScopeDataV1),
}

/// Role-matched outcome for a typed payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeScriptOutcomeV1 {
    Callable(CallableOutcomeV1),
    Call(ResolutionOutcomeV1),
    Binding(ResolutionOutcomeV1),
    Surface(RecordOutcomeV1),
    Scope(RecordOutcomeV1),
}

/// Role-matched canonical reason set for a typed payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeScriptReasonsV1 {
    Callable(CallableReasonSetV1),
    Call(ReasonSet),
    Binding(BindingReasonSetV1),
    Surface(SurfaceReasonSetV1),
    Scope(ScopeReasonSetV1),
}

/// Optional role-matched primary reason retained with the set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TypeScriptPrimaryReasonV1 {
    Callable(CallableReasonV1),
    Call(CallReason),
    Binding(BindingReasonV1),
    Surface(SurfaceReasonV1),
    Scope(ScopeReasonV1),
}

/// Source keys supporting visibility, change, and other syntax decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxRefsV1 {
    pub visibility_witness_refs: Vec<SyntaxKeyV1>,
    pub change_witness_refs: Vec<SyntaxKeyV1>,
    pub support_refs: Vec<SourceWitnessKeyV1>,
}

/// Source-observed typed payload before A1 admits it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayloadDraft {
    pub role: SyntaxRole,
    pub kind: reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind,
    pub range: SourceRange,
    pub outcome: TypeScriptOutcomeV1,
    pub reasons: TypeScriptReasonsV1,
    pub primary_reason: Option<TypeScriptPrimaryReasonV1>,
    pub refs: SyntaxRefsV1,
    pub descriptor_id: PayloadDescriptorId,
    pub descriptor_hash: SourceHash,
    pub data: Value,
}

/// A source-reconstructed payload with the same envelope as `PayloadDraft`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeScriptPayload {
    pub role: SyntaxRole,
    pub kind: reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind,
    pub range: SourceRange,
    pub outcome: TypeScriptOutcomeV1,
    pub reasons: TypeScriptReasonsV1,
    pub primary_reason: Option<TypeScriptPrimaryReasonV1>,
    pub refs: SyntaxRefsV1,
    pub descriptor_id: PayloadDescriptorId,
    pub descriptor_hash: SourceHash,
    pub data: TypeScriptPayloadData,
}

/// Payload admission failed before source validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PayloadError {
    Vocabulary(VocabularyError),
    SourceMismatch(AccountingMismatch),
}

impl std::fmt::Display for PayloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vocabulary(_) => formatter.write_str("payload uses a rejected vocabulary item"),
            Self::SourceMismatch(_) => formatter.write_str("payload differs from source rebuild"),
        }
    }
}

impl std::error::Error for PayloadError {}

/// Rebuilds every raw [`TypeScriptPayload`] in one source file for runtime A1.
/// Its result is explicitly untrusted draft input: this public helper does not
/// accept a submitted draft, does not mint a capability, and cannot determine
/// an accepted result without runtime A1 validation through private A0.
pub fn rebuild_payload_from_source(
    _source_view: &impl TypeScriptPayloadSourceViewV1,
    _binding: &TypeScriptRegistryBinding,
    _snapshot: &SnapshotBinding,
    _file_id: SourceFileId,
) -> Result<Vec<TypeScriptPayload>, PayloadError> {
    let sources = _source_view.source_bundle();
    let Some(source) = sources.file(&_file_id) else {
        return Err(source_mismatch());
    };
    let records = collect_native_syntax(_file_id.clone(), &source.bytes);
    let scope_members = source_scope_members(&source.bytes, &records);
    let top_level_surface_ranges = source_surface_ranges(&source.bytes);
    let mut payloads = Vec::new();
    for record in &records {
        let NativeSyntaxRecord::Callable {
            kind,
            range,
            binding_key,
            implementation_range,
            visibility,
            ..
        } = record
        else {
            continue;
        };
        if !scope_members.iter().any(|(role, member_range)| {
            *role == SourceSyntaxRole::Callable && *member_range == *range
        }) {
            continue;
        }
        let (descriptor_id, descriptor_hash) = descriptor_for_role(SyntaxRole::Callable)?;
        let outcome = match visibility {
            VisibilityValueV1::Exported => CallableOutcomeV1::EligiblePublic,
            VisibilityValueV1::NonExported => CallableOutcomeV1::NonPublic,
            VisibilityValueV1::Unknown => CallableOutcomeV1::Unsupported,
        };
        let reasons =
            CallableReasonSetV1::new(_binding, Vec::new()).map_err(PayloadError::Vocabulary)?;
        payloads.push(TypeScriptPayload {
            role: SyntaxRole::Callable,
            kind: kind.clone(),
            range: *range,
            outcome: TypeScriptOutcomeV1::Callable(outcome),
            reasons: TypeScriptReasonsV1::Callable(reasons),
            primary_reason: None,
            refs: SyntaxRefsV1 {
                visibility_witness_refs: callable_visibility_witness_refs(
                    &records, _binding, _snapshot, &_file_id, *range,
                ),
                change_witness_refs: Vec::new(),
                support_refs: Vec::new(),
            },
            descriptor_id,
            descriptor_hash,
            data: TypeScriptPayloadData::Callable(CallableDataV1 {
                binding_key: binding_key
                    .as_deref()
                    .map(|key| PayloadBindingKeyV1::parse_wire(_binding, key))
                    .transpose()
                    .map_err(PayloadError::Vocabulary)?,
                declaration_range: *range,
                implementation_range: *implementation_range,
                visibility_value: *visibility,
                binding_obstruction_keys: Vec::new(),
            }),
        });
    }
    let parsed_tree = parse_typescript_tree(&source.bytes);
    let local_calls = parsed_tree
        .as_ref()
        .map(|tree| evaluate_local_calls(tree.root_node(), &source.bytes, _file_id.clone()));
    let caller_ids = parsed_tree
        .as_ref()
        .map(|tree| {
            collect_top_level_callables(tree.root_node(), &source.bytes, _file_id.clone())
                .into_iter()
                .filter_map(|candidate| match candidate {
                    CallableClassification::Callable(id) => {
                        payload_caller_range_for_id(tree.root_node(), &_file_id, &id).map(|range| {
                            (
                                CallerId::from_declaration(id),
                                CallerId::from_declaration(DeclarationId::from_source(
                                    _file_id.clone(),
                                    range,
                                )),
                            )
                        })
                    }
                    CallableClassification::Rejected(_) => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let import_bindings = parsed_tree
        .as_ref()
        .map(|tree| {
            collect_imports(tree.root_node(), &source.bytes, _file_id.clone())
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    for record in &records {
        let NativeSyntaxRecord::Call {
            kind,
            range,
            binding_key,
            ..
        } = record
        else {
            continue;
        };
        let (descriptor_id, descriptor_hash) = descriptor_for_role(SyntaxRole::Call)?;
        let call_node = parsed_tree
            .as_ref()
            .and_then(|tree| call_node_at_range(tree.root_node(), *range));
        let import_associations = call_node
            .map(|call| import_associations_for_call(call, &source.bytes, &import_bindings))
            .unwrap_or_default();
        let binding_key = import_associations
            .first()
            .map(|binding| binding_local_name(binding, &source.bytes))
            .or_else(|| binding_key.clone())
            .filter(|_| !import_associations.is_empty())
            .map(|key| PayloadBindingKeyV1::parse_wire(_binding, &key))
            .transpose()
            .map_err(PayloadError::Vocabulary)?;
        let imported_decision = call_node
            .filter(|_| !import_associations.is_empty())
            .map(|call| {
                imported_call_decision(
                    _source_view,
                    _binding,
                    _snapshot,
                    &_file_id,
                    &source.bytes,
                    &records,
                    &caller_ids,
                    call,
                    *range,
                    &import_associations,
                    binding_key.clone(),
                )
            })
            .transpose()?;
        let outcome_and_data = imported_decision
            .map(CallDecision::into_payload)
            .or_else(|| {
                local_calls.as_ref().and_then(|report| {
                    caller_ids
                        .iter()
                        .map(|(report_caller, payload_caller)| {
                            (Some(report_caller.clone()), Some(payload_caller.clone()))
                        })
                        .chain(std::iter::once((None, None)))
                        .find_map(|(report_caller_id, payload_caller_id)| {
                            let key =
                                reviewgraphen_core::source_review::ids::CallsiteId::from_source(
                                    _file_id.clone(),
                                    *range,
                                    report_caller_id,
                                );
                            report
                                .edges
                                .iter()
                                .find(|edge| edge.callsite_key == key)
                                .map(|edge| {
                                    (
                                        TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved),
                                        TypeScriptReasonsV1::Call(ReasonSet::new([])),
                                        None,
                                        CallDataV1 {
                                            caller_id: payload_caller_id.clone(),
                                            callee_id: Some(edge.callee_id.clone()),
                                            resolution_kind: Some(edge.resolution_kind),
                                            binding_key: binding_key.clone(),
                                            candidate_witness_keys: Vec::new(),
                                            evaluated_stages: edge.evaluated_stages.clone(),
                                        },
                                    )
                                })
                                .or_else(|| {
                                    report
                                        .unresolved
                                        .iter()
                                        .find(|call| call.callsite_key == key)
                                        .map(|call| {
                                            let reasons = call.reasons.clone();
                                            let primary = reasons
                                                .primary()
                                                .map(TypeScriptPrimaryReasonV1::Call);
                                            (
                                                TypeScriptOutcomeV1::Call(
                                                    ResolutionOutcomeV1::Unresolved,
                                                ),
                                                TypeScriptReasonsV1::Call(reasons),
                                                primary,
                                                CallDataV1 {
                                                    caller_id: payload_caller_id.clone(),
                                                    callee_id: None,
                                                    resolution_kind: None,
                                                    binding_key: binding_key.clone(),
                                                    candidate_witness_keys: Vec::new(),
                                                    evaluated_stages: call.evaluated_stages.clone(),
                                                },
                                            )
                                        })
                                })
                        })
                })
            });
        let (outcome, reasons, primary_reason, data) = outcome_and_data.unwrap_or_else(|| {
            CallDecision::from_unresolved(
                unresolved_syntax_call(
                    None,
                    reviewgraphen_core::source_review::ids::CallsiteId::from_source(
                        _file_id.clone(),
                        *range,
                        None,
                    ),
                    *range,
                ),
                None,
                binding_key.clone(),
            )
            .into_payload()
        });
        payloads.push(TypeScriptPayload {
            role: SyntaxRole::Call,
            kind: kind.clone(),
            range: *range,
            outcome,
            reasons,
            primary_reason,
            refs: SyntaxRefsV1 {
                visibility_witness_refs: Vec::new(),
                change_witness_refs: Vec::new(),
                support_refs: Vec::new(),
            },
            descriptor_id,
            descriptor_hash,
            data: TypeScriptPayloadData::Call(data),
        });
    }
    let caller_path = sources
        .canonical_basis_path(&_file_id)
        .ok_or_else(source_mismatch)?;
    for record in &records {
        let NativeSyntaxRecord::Binding {
            kind,
            range,
            local_name,
            import_kind,
            export_slot,
            specifier,
            specifier_range,
            ..
        } = record
        else {
            continue;
        };
        let decision = binding_decision(
            sources,
            _binding,
            &source.bytes,
            &records,
            caller_path,
            local_name,
            *import_kind,
            export_slot.as_deref(),
            specifier,
        )?;
        let (descriptor_id, descriptor_hash) = descriptor_for_role(SyntaxRole::Binding)?;
        payloads.push(TypeScriptPayload {
            role: SyntaxRole::Binding,
            kind: kind.clone(),
            range: *range,
            outcome: TypeScriptOutcomeV1::Binding(decision.outcome),
            reasons: TypeScriptReasonsV1::Binding(decision.reasons),
            primary_reason: decision.primary_reason,
            refs: SyntaxRefsV1 {
                visibility_witness_refs: Vec::new(),
                change_witness_refs: Vec::new(),
                support_refs: Vec::new(),
            },
            descriptor_id,
            descriptor_hash,
            data: TypeScriptPayloadData::Binding(BindingDataV1 {
                local_name: local_name.clone(),
                import_kind: *import_kind,
                export_slot: export_slot
                    .as_deref()
                    .map(|slot| PayloadExportSlotV1::parse_wire(_binding, slot))
                    .transpose()
                    .map_err(PayloadError::Vocabulary)?,
                specifier: specifier.clone(),
                specifier_range: *specifier_range,
                candidate_paths: decision.candidate_paths,
                resolved_function_id: decision.resolved_function_id,
                evaluated_stages: decision.evaluated_stages,
            }),
        });
    }
    for record in &records {
        let NativeSyntaxRecord::Surface {
            kind,
            range,
            export_slots,
            local_names,
            target_specifier,
            ..
        } = record
        else {
            continue;
        };
        if !top_level_surface_ranges.contains(range) {
            continue;
        }
        let (descriptor_id, descriptor_hash) = descriptor_for_role(SyntaxRole::Surface)?;
        let reasons =
            SurfaceReasonSetV1::new(_binding, Vec::new()).map_err(PayloadError::Vocabulary)?;
        payloads.push(TypeScriptPayload {
            role: SyntaxRole::Surface,
            kind: kind.clone(),
            range: *range,
            outcome: TypeScriptOutcomeV1::Surface(RecordOutcomeV1::Recorded),
            reasons: TypeScriptReasonsV1::Surface(reasons),
            primary_reason: None,
            refs: SyntaxRefsV1 {
                visibility_witness_refs: Vec::new(),
                change_witness_refs: Vec::new(),
                support_refs: Vec::new(),
            },
            descriptor_id,
            descriptor_hash,
            data: TypeScriptPayloadData::Surface(SurfaceDataV1 {
                export_slots: export_slots
                    .iter()
                    .map(|slot| PayloadExportSlotV1::parse_wire(_binding, slot))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(PayloadError::Vocabulary)?,
                local_names: local_names.clone(),
                target_specifier: target_specifier.clone(),
                declaration_key: surface_declaration_key(
                    &records,
                    _binding,
                    _snapshot,
                    &_file_id,
                    *range,
                    target_specifier,
                ),
            }),
        });
    }
    for record in &records {
        let NativeSyntaxRecord::Scope {
            kind,
            range,
            scope_kind,
            ..
        } = record
        else {
            continue;
        };
        let (descriptor_id, descriptor_hash) = descriptor_for_role(SyntaxRole::Scope)?;
        let reasons =
            ScopeReasonSetV1::new(_binding, Vec::new()).map_err(PayloadError::Vocabulary)?;
        payloads.push(TypeScriptPayload {
            role: SyntaxRole::Scope,
            kind: kind.clone(),
            range: *range,
            outcome: TypeScriptOutcomeV1::Scope(RecordOutcomeV1::Recorded),
            reasons: TypeScriptReasonsV1::Scope(reasons),
            primary_reason: None,
            refs: SyntaxRefsV1 {
                visibility_witness_refs: Vec::new(),
                change_witness_refs: Vec::new(),
                support_refs: Vec::new(),
            },
            descriptor_id,
            descriptor_hash,
            data: TypeScriptPayloadData::Scope(ScopeDataV1 {
                scope_kind: *scope_kind,
                member_keys: scope_member_keys(
                    &records,
                    _binding,
                    _snapshot,
                    &_file_id,
                    &scope_members,
                ),
            }),
        });
    }
    Ok(payloads)
}

/// The source resolver's complete result, including exactly the stages it
/// visited. The payload loop copies this structure; it does not reconstruct a
/// trace from a terminal reason.
struct CallDecision {
    outcome: TypeScriptOutcomeV1,
    reasons: TypeScriptReasonsV1,
    primary_reason: Option<TypeScriptPrimaryReasonV1>,
    data: CallDataV1,
}

impl CallDecision {
    fn from_unresolved(
        unresolved: UnresolvedCall,
        caller_id: Option<CallerId>,
        binding_key: Option<PayloadBindingKeyV1>,
    ) -> Self {
        let reasons = unresolved.reasons;
        Self {
            outcome: TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Unresolved),
            primary_reason: reasons.primary().map(TypeScriptPrimaryReasonV1::Call),
            data: CallDataV1 {
                caller_id,
                callee_id: None,
                resolution_kind: None,
                binding_key,
                candidate_witness_keys: Vec::new(),
                evaluated_stages: unresolved.evaluated_stages,
            },
            reasons: TypeScriptReasonsV1::Call(reasons),
        }
    }

    fn into_payload(
        self,
    ) -> (
        TypeScriptOutcomeV1,
        TypeScriptReasonsV1,
        Option<TypeScriptPrimaryReasonV1>,
        CallDataV1,
    ) {
        (self.outcome, self.reasons, self.primary_reason, self.data)
    }
}

fn import_unresolved_decision(
    caller_id: Option<CallerId>,
    payload_caller_id: Option<CallerId>,
    caller_file_id: &SourceFileId,
    call_range: SourceRange,
    reasons: impl IntoIterator<Item = CallReason>,
    attempted: ImportCallAttempt,
    binding_key: Option<PayloadBindingKeyV1>,
) -> CallDecision {
    let callsite_key = reviewgraphen_core::source_review::ids::CallsiteId::from_source(
        caller_file_id.clone(),
        call_range,
        caller_id.clone(),
    );
    CallDecision::from_unresolved(
        unresolved_import_call(caller_id, callsite_key, call_range, reasons, attempted),
        payload_caller_id,
        binding_key,
    )
}

#[allow(clippy::too_many_arguments)]
fn imported_call_decision(
    source_view: &impl TypeScriptPayloadSourceViewV1,
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    caller_file_id: &SourceFileId,
    caller_source: &[u8],
    caller_records: &[NativeSyntaxRecord],
    caller_ids: &[(CallerId, CallerId)],
    call_node: Node<'_>,
    call_range: SourceRange,
    associations: &[&ImportBinding],
    binding_key: Option<PayloadBindingKeyV1>,
) -> Result<CallDecision, PayloadError> {
    let sources = source_view.source_bundle();
    let caller = caller_scope_for_call(caller_file_id, caller_records, caller_ids, call_range);
    let Some((caller, payload_caller_id)) = caller else {
        return Ok(import_unresolved_decision(
            None,
            None,
            caller_file_id,
            call_range,
            [CallReason::UnsupportedCaller],
            ImportCallAttempt::Caller,
            binding_key,
        ));
    };
    if associations.len() != 1 {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ImportBindingAmbiguous],
            ImportCallAttempt::Form,
            binding_key,
        ));
    }
    let import = associations[0];
    let local_name = binding_local_name(import, caller_source);
    if import.import_kind == ImportKind::TypeOnly {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::TypeOnlyBinding],
            ImportCallAttempt::Form,
            binding_key,
        ));
    }
    if import.import_kind == ImportKind::Namespace {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ImportResolutionUnavailable],
            ImportCallAttempt::Form,
            binding_key,
        ));
    }
    if caller_local_conflicts(caller_source, caller_records, &local_name) {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ImportBindingAmbiguous],
            ImportCallAttempt::LocalBinding,
            binding_key,
        ));
    }
    let Some(specifier) = import_specifier_for_local(caller_records, &local_name) else {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ImportResolutionUnavailable],
            ImportCallAttempt::LocalBinding,
            binding_key,
        ));
    };
    if !is_relative_binding_specifier(specifier) {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ImportResolutionUnavailable],
            ImportCallAttempt::LocalBinding,
            binding_key,
        ));
    }
    let Some(caller_path) = sources.canonical_basis_path(caller_file_id) else {
        return Err(source_mismatch());
    };
    let candidate_paths = binding_candidate_paths(sources, caller_path, specifier);
    if candidate_paths.is_empty() {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::RelativeSpecifierUnsupported],
            ImportCallAttempt::Specifier,
            binding_key,
        ));
    }
    let candidate_entries = candidate_paths
        .iter()
        .filter_map(|candidate| {
            source_view
                .target_outcome_for_path(&candidate.path)
                .map(|outcome| RelativeEntry {
                    path: candidate.path.clone(),
                    outcome: relative_entry_outcome(outcome),
                })
        })
        .collect::<Vec<_>>();
    let complete = source_view.target_inventory_complete();
    let candidate_reasons = relative_call_candidate_reasons(
        &candidate_entries,
        complete,
        candidate_paths
            .iter()
            .any(|candidate| candidate.rejection_witness),
    );
    if !candidate_reasons.is_empty() {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            candidate_reasons,
            ImportCallAttempt::Candidates,
            binding_key,
        ));
    }
    let Some(candidate) = candidate_paths.iter().find(|candidate| {
        candidate.file_key.is_some()
            && candidate_entries.iter().any(|entry| {
                entry.path == candidate.path && entry.outcome == RelativeEntryOutcome::Parsed
            })
    }) else {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::RelativeTargetUnread],
            ImportCallAttempt::Candidates,
            binding_key,
        ));
    };
    let callee_file_id = candidate
        .file_key
        .as_ref()
        .expect("parsed candidate carries its A0 source file ID");
    let Some(callee_source) = sources.file(callee_file_id) else {
        return Err(source_mismatch());
    };
    let Some(callee_tree) = parse_typescript_tree(&callee_source.bytes) else {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ParseFailure],
            ImportCallAttempt::Candidates,
            binding_key,
        ));
    };
    let export_resolution = resolve_export_binding(
        caller_source,
        callee_tree.root_node(),
        &callee_source.bytes,
        callee_file_id.clone(),
        import,
    );
    let callsite_key = reviewgraphen_core::source_review::ids::CallsiteId::from_source(
        caller_file_id.clone(),
        call_range,
        Some(caller.caller_id.clone()),
    );
    let relative = resolve_relative_call(RelativeCallInput {
        caller: caller.clone(),
        callsite_key,
        callsite_node: call_node,
        caller_source,
        caller_file_id: caller_file_id.clone(),
        binding: import,
        export_resolution: export_resolution.clone(),
        candidate_entries,
        tree_complete: complete,
        callee_root: callee_tree.root_node(),
        callee_source: &callee_source.bytes,
        callee_file_id: callee_file_id.clone(),
    });
    let Some(edge) = relative.edge else {
        let unresolved = relative
            .unresolved
            .expect("relative resolver returns one result form");
        return Ok(CallDecision::from_unresolved(
            unresolved,
            Some(payload_caller_id),
            binding_key,
        ));
    };
    let Ok(resolved_export) = export_resolution else {
        unreachable!("an accepted relative edge requires its export proof")
    };
    let Some(witness) = relative_export_witness(
        binding,
        snapshot,
        callee_file_id,
        &callee_source.bytes,
        &resolved_export,
    ) else {
        return Ok(import_unresolved_decision(
            Some(caller.caller_id.clone()),
            Some(payload_caller_id),
            caller_file_id,
            call_range,
            [CallReason::ExportBindingUnsupported],
            ImportCallAttempt::ExportBinding,
            binding_key,
        ));
    };
    Ok(CallDecision {
        outcome: TypeScriptOutcomeV1::Call(ResolutionOutcomeV1::Resolved),
        reasons: TypeScriptReasonsV1::Call(ReasonSet::new([])),
        primary_reason: None,
        data: CallDataV1 {
            caller_id: Some(payload_caller_id),
            callee_id: Some(edge.callee_id),
            resolution_kind: Some(ResolutionKind::SyntacticUniqueRelativeImportV1),
            binding_key,
            candidate_witness_keys: vec![witness],
            evaluated_stages: edge.evaluated_stages,
        },
    })
}

fn caller_scope_for_call(
    file_id: &SourceFileId,
    records: &[NativeSyntaxRecord],
    caller_ids: &[(CallerId, CallerId)],
    call_range: SourceRange,
) -> Option<(CallerScope, CallerId)> {
    caller_ids
        .iter()
        .filter_map(|(report_id, payload_id)| {
            records.iter().find_map(|record| {
                let NativeSyntaxRecord::Callable { range, .. } = record else {
                    return None;
                };
                let declaration = DeclarationId::from_source(file_id.clone(), *range);
                (CallerId::from_declaration(declaration) == *report_id
                    && range_contains(*range, call_range))
                .then(|| {
                    (
                        CallerScope {
                            caller_id: report_id.clone(),
                            caller_range: *range,
                        },
                        payload_id.clone(),
                    )
                })
            })
        })
        .min_by_key(|(scope, _)| scope.caller_range.end() - scope.caller_range.start())
}

fn call_node_at_range(root: Node<'_>, range: SourceRange) -> Option<Node<'_>> {
    let mut calls = Vec::new();
    collect_nodes_of_kind(root, "call_expression", &mut calls);
    calls.into_iter().find(|call| {
        SourceRange::new(call.start_byte() as u64, call.end_byte() as u64).ok() == Some(range)
    })
}

fn import_associations_for_call<'a>(
    call: Node<'_>,
    source: &[u8],
    imports: &'a [ImportBinding],
) -> Vec<&'a ImportBinding> {
    let Some(function) = call.child_by_field_name("function") else {
        return Vec::new();
    };
    let binding = match function.kind() {
        "identifier" => Some(function),
        "member_expression" | "optional_member_expression" => function
            .child_by_field_name("object")
            .filter(|object| object.kind() == "identifier"),
        _ => None,
    };
    let Some(binding) = binding else {
        return Vec::new();
    };
    let local_name = std::str::from_utf8(&source[binding.start_byte()..binding.end_byte()])
        .expect("TypeScript parser receives UTF-8 source");
    imports
        .iter()
        .filter(|import| binding_local_name(import, source) == local_name)
        .collect()
}

fn binding_local_name(binding: &ImportBinding, source: &[u8]) -> String {
    source[binding.local_binding_range.start() as usize..binding.local_binding_range.end() as usize]
        .iter()
        .map(|byte| *byte as char)
        .collect()
}

fn import_specifier_for_local<'a>(
    records: &'a [NativeSyntaxRecord],
    local_name: &str,
) -> Option<&'a str> {
    records.iter().find_map(|record| {
        let NativeSyntaxRecord::Binding {
            local_name: observed,
            specifier,
            ..
        } = record
        else {
            return None;
        };
        (observed == local_name).then_some(specifier.as_str())
    })
}

fn relative_entry_outcome(outcome: SourceFileOutcome) -> RelativeEntryOutcome {
    match outcome {
        SourceFileOutcome::Parsed => RelativeEntryOutcome::Parsed,
        SourceFileOutcome::ProfileExcluded => RelativeEntryOutcome::Excluded,
        SourceFileOutcome::ParseFailed => RelativeEntryOutcome::ParseFailed,
        SourceFileOutcome::UnreadBound => RelativeEntryOutcome::Unread,
        SourceFileOutcome::NonTargetExtension | SourceFileOutcome::UnsupportedEntry => {
            RelativeEntryOutcome::Other
        }
    }
}

fn relative_call_candidate_reasons(
    entries: &[RelativeEntry],
    tree_complete: bool,
    has_rejection_witness: bool,
) -> Vec<CallReason> {
    let mut reasons = Vec::new();
    if entries.len() > 1 || has_rejection_witness {
        reasons.push(CallReason::RelativeTargetAmbiguous);
    }
    if entries
        .iter()
        .any(|entry| entry.outcome == RelativeEntryOutcome::Excluded)
    {
        reasons.push(CallReason::RelativeTargetExcluded);
    }
    if entries.iter().any(|entry| {
        matches!(
            entry.outcome,
            RelativeEntryOutcome::Unread
                | RelativeEntryOutcome::ParseFailed
                | RelativeEntryOutcome::Other
        )
    }) {
        reasons.push(CallReason::RelativeTargetUnread);
    }
    if entries
        .iter()
        .any(|entry| entry.outcome == RelativeEntryOutcome::ParseFailed)
    {
        reasons.push(CallReason::ParseFailure);
    }
    if !tree_complete {
        reasons.push(CallReason::RelativeTargetUnread);
    }
    if entries.is_empty() && tree_complete {
        reasons.push(CallReason::RelativeTargetMissing);
    }
    reasons
}

fn relative_export_witness(
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    file_id: &SourceFileId,
    source: &[u8],
    resolved: &super::import_bindings::ResolvedExportBinding,
) -> Option<SourceWitnessKeyV1> {
    let witness_range = *resolved.visibility_witnesses().first()?;
    collect_native_syntax(file_id.clone(), source)
        .into_iter()
        .find_map(|record| {
            let NativeSyntaxRecord::Surface { kind, range, .. } = record else {
                return None;
            };
            (range == witness_range).then(|| {
                SourceWitnessKeyV1::Syntax(SyntaxKeyV1::derive_from_source(
                    binding,
                    snapshot,
                    file_id,
                    SourceSyntaxRole::Surface,
                    &kind,
                    range,
                ))
            })
        })
}

fn binding_candidate_paths(
    sources: &AdmittedSourceBundleV1,
    caller_path: &str,
    specifier: &str,
) -> Vec<CandidatePathV1> {
    let Some(base) = normalized_relative_binding_path(caller_path, specifier) else {
        return Vec::new();
    };
    let mut paths = if extensionless_relative_specifier(specifier) {
        vec![
            base.clone(),
            format!("{base}.ts"),
            format!("{base}.tsx"),
            format!("{base}/index.ts"),
            format!("{base}/index.tsx"),
        ]
    } else if specifier.ends_with(".ts") || specifier.ends_with(".tsx") {
        vec![base]
    } else if specifier.ends_with(".js") {
        let Some(stem) = base.strip_suffix(".js").map(str::to_owned) else {
            return Vec::new();
        };
        vec![base, format!("{stem}.ts"), format!("{stem}.tsx")]
    } else {
        return Vec::new();
    };
    paths.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    paths.dedup();
    paths
        .into_iter()
        .map(|path| CandidatePathV1 {
            file_key: sources
                .file_by_canonical_basis_path(&path)
                .map(|file| file.file_id.clone()),
            path,
            rejection_witness: false,
            unexpanded_ancestor_key: None,
        })
        .collect()
}

fn normalized_relative_binding_path(caller_path: &str, specifier: &str) -> Option<String> {
    if !(specifier.starts_with("./") || specifier.starts_with("../"))
        || specifier.ends_with('/')
        || specifier.contains(['\\', '\0', '?', '#', '%'])
    {
        return None;
    }
    let mut parts = caller_path.split('/').collect::<Vec<_>>();
    parts.pop();
    for part in specifier.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn extensionless_relative_specifier(specifier: &str) -> bool {
    specifier
        .rsplit('/')
        .next()
        .is_some_and(|name| !name.contains('.'))
}

struct BindingDecision {
    candidate_paths: Vec<CandidatePathV1>,
    outcome: ResolutionOutcomeV1,
    reasons: BindingReasonSetV1,
    primary_reason: Option<TypeScriptPrimaryReasonV1>,
    resolved_function_id: Option<DeclarationId>,
    evaluated_stages: Vec<BindingStageV1>,
}

struct ResolvedBindingTarget {
    declaration_id: DeclarationId,
    file_id: SourceFileId,
    binding_key: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn binding_decision(
    sources: &AdmittedSourceBundleV1,
    binding: &TypeScriptRegistryBinding,
    caller_source: &[u8],
    caller_records: &[NativeSyntaxRecord],
    caller_path: &str,
    local_name: &str,
    import_kind: PayloadImportKindV1,
    export_slot: Option<&str>,
    specifier: &str,
) -> Result<BindingDecision, PayloadError> {
    if import_kind == PayloadImportKindV1::TypeOnly {
        return unresolved_binding_decision(binding, "type_only_binding", Vec::new(), &["b.form"]);
    }
    if import_kind == PayloadImportKindV1::Namespace || !is_relative_binding_specifier(specifier) {
        return unresolved_binding_decision(
            binding,
            "import_resolution_unavailable",
            Vec::new(),
            &["b.form"],
        );
    }
    if caller_local_conflicts(caller_source, caller_records, local_name) {
        return unresolved_binding_decision(
            binding,
            "import_binding_ambiguous",
            Vec::new(),
            &["b.form", "b.local_uniqueness"],
        );
    }
    if normalized_relative_binding_path(caller_path, specifier).is_none() {
        return unresolved_binding_decision(
            binding,
            "relative_specifier_unsupported",
            Vec::new(),
            &["b.form", "b.local_uniqueness", "b.specifier"],
        );
    }
    let candidate_paths = binding_candidate_paths(sources, caller_path, specifier);
    if candidate_paths.is_empty() {
        return unresolved_binding_decision(
            binding,
            "relative_specifier_unsupported",
            candidate_paths,
            &["b.form", "b.local_uniqueness", "b.specifier"],
        );
    }
    let admitted_candidate_count = candidate_paths
        .iter()
        .filter(|candidate| candidate.file_key.is_some())
        .count();
    if admitted_candidate_count == 0 {
        return unresolved_binding_decision(
            binding,
            "relative_target_missing",
            candidate_paths,
            &[
                "b.form",
                "b.local_uniqueness",
                "b.specifier",
                "b.candidates",
            ],
        );
    }
    if admitted_candidate_count > 1 {
        return unresolved_binding_decision(
            binding,
            "relative_target_ambiguous",
            candidate_paths,
            &[
                "b.form",
                "b.local_uniqueness",
                "b.specifier",
                "b.candidates",
            ],
        );
    }
    let Some(target) = resolved_binding_target(sources, &candidate_paths, import_kind, export_slot)
    else {
        return unresolved_binding_decision(
            binding,
            "export_binding_unsupported",
            candidate_paths,
            &[
                "b.form",
                "b.local_uniqueness",
                "b.specifier",
                "b.candidates",
                "b.export_binding",
            ],
        );
    };
    let callee_written = target.binding_key.as_deref().is_some_and(|binding_key| {
        sources
            .file(&target.file_id)
            .is_some_and(|source| source_has_binding_write(&source.bytes, binding_key))
    });
    if source_has_binding_write(caller_source, local_name) || callee_written {
        return unresolved_binding_decision(
            binding,
            "written_binding",
            candidate_paths,
            &[
                "b.form",
                "b.local_uniqueness",
                "b.specifier",
                "b.candidates",
                "b.export_binding",
                "b.writes",
            ],
        );
    }
    resolved_binding_decision(binding, candidate_paths, target.declaration_id)
}

fn unresolved_binding_decision(
    binding: &TypeScriptRegistryBinding,
    reason_wire: &str,
    candidate_paths: Vec<CandidatePathV1>,
    stage_wires: &[&str],
) -> Result<BindingDecision, PayloadError> {
    let reasons = BindingReasonSetV1::new(
        binding,
        vec![BindingReasonV1::parse_wire(binding, reason_wire).map_err(PayloadError::Vocabulary)?],
    )
    .map_err(PayloadError::Vocabulary)?;
    Ok(BindingDecision {
        candidate_paths,
        outcome: ResolutionOutcomeV1::Unresolved,
        primary_reason: reasons
            .primary()
            .cloned()
            .map(TypeScriptPrimaryReasonV1::Binding),
        reasons,
        resolved_function_id: None,
        evaluated_stages: binding_evaluated_stages(binding, stage_wires)?,
    })
}

fn resolved_binding_decision(
    binding: &TypeScriptRegistryBinding,
    candidate_paths: Vec<CandidatePathV1>,
    declaration_id: DeclarationId,
) -> Result<BindingDecision, PayloadError> {
    let reasons = BindingReasonSetV1::new(binding, Vec::new()).map_err(PayloadError::Vocabulary)?;
    Ok(BindingDecision {
        candidate_paths,
        outcome: ResolutionOutcomeV1::Resolved,
        reasons,
        primary_reason: None,
        resolved_function_id: Some(declaration_id),
        evaluated_stages: binding_evaluated_stages(
            binding,
            &[
                "b.form",
                "b.local_uniqueness",
                "b.specifier",
                "b.candidates",
                "b.export_binding",
                "b.writes",
                "b.result",
            ],
        )?,
    })
}

fn is_relative_binding_specifier(specifier: &str) -> bool {
    specifier.starts_with("./") || specifier.starts_with("../")
}

fn caller_local_conflicts(source: &[u8], records: &[NativeSyntaxRecord], local_name: &str) -> bool {
    records
        .iter()
        .filter(|record| {
            matches!(record, NativeSyntaxRecord::Binding { local_name: observed, .. } if observed == local_name)
        })
        .take(2)
        .count()
        > 1
        || source_has_module_declaration(source, local_name)
}

fn source_has_module_declaration(source: &[u8], local_name: &str) -> bool {
    let Ok(text) = std::str::from_utf8(source) else {
        return false;
    };
    let Some(tree) = parsed_typescript_tree(source) else {
        return false;
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .any(|node| module_declaration_has_name(node, text, local_name))
}

fn module_declaration_has_name(node: Node<'_>, source: &str, local_name: &str) -> bool {
    match node.kind() {
        "export_statement" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .any(|child| module_declaration_has_name(child, source, local_name))
        }
        "lexical_declaration" | "variable_declaration" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter(|child| child.kind() == "variable_declarator")
                .any(|child| declaration_has_name(child, source, local_name))
        }
        "function_declaration"
        | "generator_function_declaration"
        | "class_declaration"
        | "enum_declaration"
        | "interface_declaration"
        | "type_alias_declaration" => declaration_has_name(node, source, local_name),
        _ => false,
    }
}

fn declaration_has_name(node: Node<'_>, source: &str, local_name: &str) -> bool {
    node.child_by_field_name("name")
        .and_then(|name| source.get(name.byte_range()))
        .is_some_and(|name| name == local_name)
}

fn source_has_binding_write(source: &[u8], binding_key: &str) -> bool {
    let Ok(text) = std::str::from_utf8(source) else {
        return false;
    };
    let Some(tree) = parsed_typescript_tree(source) else {
        return false;
    };
    source_tree_has_binding_write(tree.root_node(), text, binding_key)
}

fn source_tree_has_binding_write(node: Node<'_>, source: &str, binding_key: &str) -> bool {
    if write_target_identifier(node)
        .filter(|target| target.kind() == "identifier")
        .and_then(|target| source.get(target.byte_range()))
        .is_some_and(|target| target == binding_key)
        && !write_is_shadowed(node, source, binding_key)
    {
        return true;
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .any(|child| source_tree_has_binding_write(child, source, binding_key))
}

fn write_target_identifier(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "assignment_expression" | "augmented_assignment_expression" => {
            node.child_by_field_name("left")
        }
        "update_expression" => node
            .child_by_field_name("argument")
            .or_else(|| node.child_by_field_name("operand"))
            .or_else(|| {
                let mut cursor = node.walk();
                node.named_children(&mut cursor).next()
            }),
        _ => None,
    }
}

fn write_is_shadowed<'tree>(node: Node<'tree>, source: &str, binding_key: &str) -> bool {
    let mut ancestor = node.parent();
    while let Some(scope) = ancestor {
        if scope_introduces_binding(scope, node, source, binding_key) {
            return true;
        }
        ancestor = scope.parent();
    }
    false
}

fn scope_introduces_binding<'tree>(
    scope: Node<'tree>,
    write: Node<'tree>,
    source: &str,
    binding_key: &str,
) -> bool {
    function_scope_has_parameter(scope, write, source, binding_key)
        || lexical_scope_has_local(scope, source, binding_key)
}

fn function_scope_has_parameter<'tree>(
    scope: Node<'tree>,
    write: Node<'tree>,
    source: &str,
    binding_key: &str,
) -> bool {
    runtime_function_scope_binds_name(scope, write, source, binding_key).unwrap_or(false)
}

/// Returns `None` for a non-runtime-function node or an unsupported/malformed
/// parameter shape.  In either case no shadow binding is inferred from an
/// arbitrary descendant identifier.
fn runtime_function_scope_binds_name<'tree>(
    scope: Node<'tree>,
    write: Node<'tree>,
    source: &str,
    binding_key: &str,
) -> Option<bool> {
    let body = scope.child_by_field_name("body")?;
    if !node_is_descendant_of(write, body) {
        return Some(false);
    }

    match scope.kind() {
        "function_declaration"
        | "generator_function_declaration"
        | "function_expression"
        | "generator_function"
        | "method_definition" => scope
            .child_by_field_name("parameters")
            .filter(|parameters| parameters.kind() == "formal_parameters")
            .and_then(|parameters| formal_parameters_bind_name(parameters, source, binding_key)),
        "arrow_function" => match (
            scope.child_by_field_name("parameter"),
            scope.child_by_field_name("parameters"),
        ) {
            (Some(parameter), None) if parameter.kind() == "identifier" => {
                binding_identifier_matches(parameter, source, binding_key)
            }
            (None, Some(parameters)) if parameters.kind() == "formal_parameters" => {
                formal_parameters_bind_name(parameters, source, binding_key)
            }
            _ => None,
        },
        _ => None,
    }
}

fn node_is_descendant_of<'tree>(node: Node<'tree>, ancestor: Node<'tree>) -> bool {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if candidate == ancestor {
            return true;
        }
        current = candidate.parent();
    }
    false
}

fn formal_parameters_bind_name(
    parameters: Node<'_>,
    source: &str,
    binding_key: &str,
) -> Option<bool> {
    if parameters.kind() != "formal_parameters" {
        return None;
    }
    let mut cursor = parameters.walk();
    let mut matches_binding = false;
    for parameter in parameters.named_children(&mut cursor) {
        if !matches!(
            parameter.kind(),
            "required_parameter" | "optional_parameter"
        ) {
            return None;
        }
        matches_binding |= parameter
            .child_by_field_name("pattern")
            .and_then(|pattern| binding_position_binds_name(pattern, source, binding_key))?;
    }
    Some(matches_binding)
}

fn binding_position_binds_name(node: Node<'_>, source: &str, binding_key: &str) -> Option<bool> {
    match node.kind() {
        "identifier" | "shorthand_property_identifier_pattern" => {
            binding_identifier_matches(node, source, binding_key)
        }
        "this" => Some(false),
        "array_pattern" => direct_binding_positions_bind_name(node, source, binding_key),
        "object_pattern" => object_pattern_binds_name(node, source, binding_key),
        "pair_pattern" => node
            .child_by_field_name("value")
            .and_then(|value| binding_position_binds_name(value, source, binding_key)),
        "object_assignment_pattern" | "assignment_pattern" => node
            .child_by_field_name("left")
            .and_then(|left| binding_position_binds_name(left, source, binding_key)),
        "rest_pattern" => {
            let rest_target = sole_direct_named_child(node)?;
            matches!(
                rest_target.kind(),
                "identifier" | "array_pattern" | "object_pattern"
            )
            .then(|| binding_position_binds_name(rest_target, source, binding_key))?
        }
        _ => None,
    }
}

fn binding_identifier_matches(node: Node<'_>, source: &str, binding_key: &str) -> Option<bool> {
    source
        .get(node.byte_range())
        .map(|identifier| identifier == binding_key)
}

fn direct_binding_positions_bind_name(
    node: Node<'_>,
    source: &str,
    binding_key: &str,
) -> Option<bool> {
    let mut cursor = node.walk();
    let mut matches_binding = false;
    for child in node.named_children(&mut cursor) {
        matches_binding |= binding_position_binds_name(child, source, binding_key)?;
    }
    Some(matches_binding)
}

fn object_pattern_binds_name(node: Node<'_>, source: &str, binding_key: &str) -> Option<bool> {
    let mut cursor = node.walk();
    let mut matches_binding = false;
    for child in node.named_children(&mut cursor) {
        if !matches!(
            child.kind(),
            "pair_pattern"
                | "object_assignment_pattern"
                | "rest_pattern"
                | "shorthand_property_identifier_pattern"
        ) {
            return None;
        }
        matches_binding |= binding_position_binds_name(child, source, binding_key)?;
    }
    Some(matches_binding)
}

fn sole_direct_named_child<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    let mut children = node.named_children(&mut cursor);
    let child = children.next()?;
    children.next().is_none().then_some(child)
}

fn lexical_scope_has_local(scope: Node<'_>, source: &str, binding_key: &str) -> bool {
    if !matches!(scope.kind(), "statement_block" | "switch_body") {
        return false;
    }
    let mut cursor = scope.walk();
    scope
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "lexical_declaration" | "variable_declaration"))
        .any(|declaration| declaration_declares_binding(declaration, source, binding_key))
}

fn declaration_declares_binding(node: Node<'_>, source: &str, binding_key: &str) -> bool {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| child.kind() == "variable_declarator")
        .any(|declarator| {
            declarator
                .child_by_field_name("name")
                .is_some_and(|name| subtree_has_identifier(name, source, binding_key))
        })
}

fn subtree_has_identifier(node: Node<'_>, source: &str, binding_key: &str) -> bool {
    (node.kind() == "identifier"
        && source
            .get(node.byte_range())
            .is_some_and(|identifier| identifier == binding_key))
        || {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .any(|child| subtree_has_identifier(child, source, binding_key))
        }
}

fn parsed_typescript_tree(source: &[u8]) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    (!tree.root_node().has_error()).then_some(tree)
}

fn resolved_binding_target(
    sources: &AdmittedSourceBundleV1,
    candidate_paths: &[CandidatePathV1],
    import_kind: PayloadImportKindV1,
    export_slot: Option<&str>,
) -> Option<ResolvedBindingTarget> {
    let mut admitted_candidates = candidate_paths
        .iter()
        .filter_map(|candidate| candidate.file_key.as_ref())
        .filter_map(|file_id| sources.file(file_id));
    let candidate = admitted_candidates.next()?;
    if admitted_candidates.next().is_some() {
        return None;
    }
    let default_ranges = default_exported_callable_ranges(&candidate.bytes);
    let mut matched_callables = collect_native_syntax(candidate.file_id.clone(), &candidate.bytes)
        .into_iter()
        .filter_map(|record| {
            let NativeSyntaxRecord::Callable {
                binding_key: Some(binding_key),
                range,
                visibility: VisibilityValueV1::Exported,
                ..
            } = record
            else {
                return None;
            };
            let matches_export = match import_kind {
                PayloadImportKindV1::Named => {
                    export_slot.is_some_and(|export_slot| binding_key == export_slot)
                }
                PayloadImportKindV1::Default => default_ranges.contains(&range),
                PayloadImportKindV1::TypeOnly | PayloadImportKindV1::Namespace => false,
            };
            matches_export.then(|| ResolvedBindingTarget {
                declaration_id: DeclarationId::from_source(candidate.file_id.clone(), range),
                file_id: candidate.file_id.clone(),
                binding_key: Some(binding_key),
            })
        });
    let matched = matched_callables.next()?;
    matched_callables.next().is_none().then_some(matched)
}

fn default_exported_callable_ranges(source: &[u8]) -> Vec<SourceRange> {
    let Some(tree) = parsed_typescript_tree(source) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| {
            node.kind() == "export_statement" && node_has_direct_child_kind(*node, "default")
        })
        .flat_map(|export_statement| {
            let mut cursor = export_statement.walk();
            export_statement
                .named_children(&mut cursor)
                .filter(|child| {
                    matches!(
                        child.kind(),
                        "function_declaration"
                            | "generator_function_declaration"
                            | "lexical_declaration"
                    )
                })
                .filter_map(|child| {
                    SourceRange::new(child.start_byte() as u64, child.end_byte() as u64).ok()
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn node_has_direct_child_kind(node: Node<'_>, kind: &str) -> bool {
    (0..node.child_count()).any(|index| node.child(index).is_some_and(|child| child.kind() == kind))
}

fn binding_evaluated_stages(
    binding: &TypeScriptRegistryBinding,
    stage_wires: &[&str],
) -> Result<Vec<BindingStageV1>, PayloadError> {
    stage_wires
        .iter()
        .map(|wire| BindingStageV1::parse_wire(binding, wire).map_err(PayloadError::Vocabulary))
        .collect()
}

/// Copies every field of a typed payload into its raw submission draft form.
/// Non-authority: it validates nothing, fills nothing, and never mints a capability.
pub fn encode_payload_draft(_payload: &TypeScriptPayload) -> PayloadDraft {
    PayloadDraft {
        role: _payload.role,
        kind: _payload.kind.clone(),
        range: _payload.range,
        outcome: _payload.outcome.clone(),
        reasons: _payload.reasons.clone(),
        primary_reason: _payload.primary_reason.clone(),
        refs: _payload.refs.clone(),
        descriptor_id: _payload.descriptor_id.clone(),
        descriptor_hash: _payload.descriptor_hash.clone(),
        data: payload_data_value(&_payload.data),
    }
}

fn descriptor_for_role(
    role: SyntaxRole,
) -> Result<(PayloadDescriptorId, SourceHash), PayloadError> {
    let wire = match role {
        SyntaxRole::Callable => "reviewgraphen.typescript_syntax.callable@1",
        SyntaxRole::Call => "reviewgraphen.typescript_syntax.call@1",
        SyntaxRole::Binding => "reviewgraphen.typescript_syntax.binding@1",
        SyntaxRole::Surface => "reviewgraphen.typescript_syntax.surface@1",
        SyntaxRole::Scope => "reviewgraphen.typescript_syntax.scope@1",
    };
    let descriptor = PayloadDescriptorId::parse_wire(wire).map_err(PayloadError::Vocabulary)?;
    let definition = match role {
        SyntaxRole::Callable => json!({
            "additional_properties": false,
            "record_role": "callable",
            "required": ["binding_key", "declaration_range", "implementation_range", "visibility_value", "binding_obstruction_keys"],
        }),
        SyntaxRole::Call => json!({
            "additional_properties": false,
            "record_role": "call",
            "required": ["caller_id", "callee_id", "resolution_kind", "binding_key", "candidate_witness_keys", "evaluated_stages"],
        }),
        SyntaxRole::Binding => json!({
            "additional_properties": false,
            "record_role": "binding",
            "required": ["local_name", "import_kind", "export_slot", "specifier", "specifier_range", "candidate_paths", "resolved_function_id", "evaluated_stages"],
        }),
        SyntaxRole::Surface => json!({
            "additional_properties": false,
            "record_role": "surface",
            "required": ["export_slots", "local_names", "target_specifier", "declaration_key"],
        }),
        SyntaxRole::Scope => json!({
            "additional_properties": false,
            "record_role": "scope",
            "required": ["scope_kind", "member_keys"],
        }),
    };
    let canonical =
        canonical_json(&definition).expect("fixed payload descriptor is canonical JSON");
    Ok((descriptor, SourceHash::from_source_bytes(&canonical)))
}

fn scope_member_keys(
    records: &[NativeSyntaxRecord],
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    file_id: &SourceFileId,
    members: &[(SourceSyntaxRole, SourceRange)],
) -> Vec<SyntaxKeyV1> {
    members
        .iter()
        .filter_map(|(member_role, member_range)| {
            records.iter().find_map(|record| {
                let (record_role, kind, record_range) = native_member_identity(record)?;
                (record_role == *member_role && record_range == *member_range).then(|| {
                    SyntaxKeyV1::derive_from_source(
                        binding,
                        snapshot,
                        file_id,
                        record_role,
                        kind,
                        record_range,
                    )
                })
            })
        })
        .collect()
}

fn source_scope_members(
    source: &[u8],
    records: &[NativeSyntaxRecord],
) -> Vec<(SourceSyntaxRole, SourceRange)> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    if root.has_error() {
        return Vec::new();
    }

    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .flat_map(|node| top_level_member_roles(node, records))
        .collect()
}

fn source_surface_ranges(source: &[u8]) -> Vec<SourceRange> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    if root.has_error() {
        return Vec::new();
    }
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter(|node| node.kind() == "export_statement")
        .filter_map(node_range)
        .collect()
}

fn top_level_member_roles(
    node: Node<'_>,
    records: &[NativeSyntaxRecord],
) -> Vec<(SourceSyntaxRole, SourceRange)> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" => top_level_records_for_ranges(
            records,
            SourceSyntaxRole::Callable,
            &node_range(node).into_iter().collect::<Vec<_>>(),
        ),
        "lexical_declaration" => top_level_records_for_ranges(
            records,
            SourceSyntaxRole::Callable,
            &callable_declaration_ranges(node),
        ),
        "export_statement" => {
            let mut cursor = node.walk();
            let callables = node
                .named_children(&mut cursor)
                .flat_map(callable_declaration_ranges)
                .collect::<Vec<_>>();
            let callables =
                top_level_records_for_ranges(records, SourceSyntaxRole::Callable, &callables);
            if callables.is_empty() {
                top_level_records_for_ranges(
                    records,
                    SourceSyntaxRole::Surface,
                    &node_range(node).into_iter().collect::<Vec<_>>(),
                )
            } else {
                callables
            }
        }
        "import_statement" => records_within_node(records, SourceSyntaxRole::Binding, node),
        "expression_statement" => {
            outermost_records_within_node(records, SourceSyntaxRole::Call, node)
        }
        _ => Vec::new(),
    }
}

fn top_level_records_for_ranges(
    records: &[NativeSyntaxRecord],
    role: SourceSyntaxRole,
    ranges: &[SourceRange],
) -> Vec<(SourceSyntaxRole, SourceRange)> {
    ranges
        .iter()
        .filter(|range| {
            records.iter().any(|record| {
                native_member_identity(record).is_some_and(|(record_role, _, record_range)| {
                    record_role == role && record_range == **range
                })
            })
        })
        .map(|range| (role, *range))
        .collect()
}

fn records_within_node(
    records: &[NativeSyntaxRecord],
    role: SourceSyntaxRole,
    node: Node<'_>,
) -> Vec<(SourceSyntaxRole, SourceRange)> {
    let Some(node_range) = node_range(node) else {
        return Vec::new();
    };
    records
        .iter()
        .filter_map(native_member_identity)
        .filter_map(|(record_role, _, record_range)| {
            (record_role == role && range_contains(node_range, record_range))
                .then_some((record_role, record_range))
        })
        .collect()
}

fn outermost_records_within_node(
    records: &[NativeSyntaxRecord],
    role: SourceSyntaxRole,
    node: Node<'_>,
) -> Vec<(SourceSyntaxRole, SourceRange)> {
    let candidates = records_within_node(records, role, node);
    candidates
        .iter()
        .filter(|(_, candidate_range)| {
            !candidates.iter().any(|(_, other_range)| {
                other_range != candidate_range && range_contains(*other_range, *candidate_range)
            })
        })
        .copied()
        .collect()
}

fn callable_declaration_ranges(node: Node<'_>) -> Vec<SourceRange> {
    match node.kind() {
        "function_declaration" | "generator_function_declaration" | "function_expression" => {
            node_range(node).into_iter().collect()
        }
        "lexical_declaration" => {
            let mut cursor = node.walk();
            node.named_children(&mut cursor)
                .filter(|child| child.kind() == "variable_declarator")
                .filter_map(node_range)
                .collect()
        }
        _ => Vec::new(),
    }
}

fn node_range(node: Node<'_>) -> Option<SourceRange> {
    SourceRange::new(node.start_byte() as u64, node.end_byte() as u64).ok()
}

fn native_member_identity(
    record: &NativeSyntaxRecord,
) -> Option<(
    SourceSyntaxRole,
    &reviewgraphen_core::source_review::reasons::TypeScriptSyntaxKind,
    SourceRange,
)> {
    match record {
        NativeSyntaxRecord::Callable { kind, range, .. } => {
            Some((SourceSyntaxRole::Callable, kind, *range))
        }
        NativeSyntaxRecord::Call { kind, range, .. } => {
            Some((SourceSyntaxRole::Call, kind, *range))
        }
        NativeSyntaxRecord::Binding { kind, range, .. } => {
            Some((SourceSyntaxRole::Binding, kind, *range))
        }
        NativeSyntaxRecord::Surface { kind, range, .. } => {
            Some((SourceSyntaxRole::Surface, kind, *range))
        }
        NativeSyntaxRecord::Scope { .. } => None,
    }
}

fn payload_data_value(data: &TypeScriptPayloadData) -> Value {
    match data {
        TypeScriptPayloadData::Callable(data) => json!({
            "binding_key": data.binding_key.as_ref().map(PayloadBindingKeyV1::wire_literal),
            "binding_obstruction_keys": data.binding_obstruction_keys.iter().map(source_witness_key_wire).collect::<Vec<_>>(),
            "declaration_range": range_value(data.declaration_range),
            "implementation_range": data.implementation_range.map(range_value),
            "visibility_value": visibility_wire(data.visibility_value),
        }),
        TypeScriptPayloadData::Binding(data) => json!({
            "local_name": data.local_name,
            "import_kind": import_kind_wire(data.import_kind),
            "export_slot": data.export_slot.as_ref().map(PayloadExportSlotV1::wire_literal),
            "specifier": data.specifier,
            "specifier_range": range_value(data.specifier_range),
            "candidate_paths": data.candidate_paths.iter().map(|candidate| json!({
                "path": candidate.path,
                "file_key": candidate.file_key.as_ref().map(SourceFileId::canonical_key),
                "rejection_witness": candidate.rejection_witness,
                "unexpanded_ancestor_key": candidate.unexpanded_ancestor_key.as_ref().map(source_witness_key_wire),
            })).collect::<Vec<_>>(),
            "resolved_function_id": data.resolved_function_id.as_ref().map(DeclarationId::canonical_key),
            "evaluated_stages": data.evaluated_stages.iter().map(BindingStageV1::wire_literal).collect::<Vec<_>>(),
        }),
        TypeScriptPayloadData::Surface(data) => json!({
            "export_slots": data.export_slots.iter().map(PayloadExportSlotV1::wire_literal).collect::<Vec<_>>(),
            "local_names": data.local_names,
            "target_specifier": data.target_specifier,
            "declaration_key": data.declaration_key.as_ref().map(SyntaxKeyV1::wire_literal),
        }),
        TypeScriptPayloadData::Call(data) => json!({
            "caller_id": data.caller_id.as_ref().map(CallerId::canonical_key),
            "callee_id": data.callee_id.as_ref().map(DeclarationId::canonical_key),
            "resolution_kind": data.resolution_kind.map(ResolutionKind::wire_literal),
            "binding_key": data.binding_key.as_ref().map(PayloadBindingKeyV1::wire_literal),
            "candidate_witness_keys": data.candidate_witness_keys.iter().map(source_witness_key_wire).collect::<Vec<_>>(),
            "evaluated_stages": data.evaluated_stages.iter().map(CallStageV1::wire_literal).collect::<Vec<_>>(),
        }),
        TypeScriptPayloadData::Scope(data) => json!({
            "member_keys": data.member_keys.iter().map(SyntaxKeyV1::wire_literal).collect::<Vec<_>>(),
            "scope_kind": scope_kind_wire(data.scope_kind),
        }),
    }
}

fn callable_visibility_witness_refs(
    records: &[NativeSyntaxRecord],
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    file_id: &SourceFileId,
    callable_range: SourceRange,
) -> Vec<SyntaxKeyV1> {
    records
        .iter()
        .filter_map(|record| {
            let NativeSyntaxRecord::Surface {
                kind,
                range,
                target_specifier,
                ..
            } = record
            else {
                return None;
            };
            (target_specifier.is_none() && range_contains(*range, callable_range)).then(|| {
                SyntaxKeyV1::derive_from_source(
                    binding,
                    snapshot,
                    file_id,
                    SourceSyntaxRole::Surface,
                    kind,
                    *range,
                )
            })
        })
        .collect()
}

fn surface_declaration_key(
    records: &[NativeSyntaxRecord],
    binding: &TypeScriptRegistryBinding,
    snapshot: &SnapshotBinding,
    file_id: &SourceFileId,
    surface_range: SourceRange,
    target_specifier: &Option<String>,
) -> Option<SyntaxKeyV1> {
    if target_specifier.is_some() {
        return None;
    }
    records.iter().find_map(|record| {
        let NativeSyntaxRecord::Callable { kind, range, .. } = record else {
            return None;
        };
        range_contains(surface_range, *range).then(|| {
            SyntaxKeyV1::derive_from_source(
                binding,
                snapshot,
                file_id,
                SourceSyntaxRole::Callable,
                kind,
                *range,
            )
        })
    })
}

fn range_contains(container: SourceRange, member: SourceRange) -> bool {
    container.start() <= member.start() && member.end() <= container.end()
}

fn source_witness_key_wire(key: &SourceWitnessKeyV1) -> &str {
    match key {
        SourceWitnessKeyV1::Syntax(key) => key.wire_literal(),
        SourceWitnessKeyV1::BasisEndpoint(key) => key.wire_literal(),
    }
}

fn parse_typescript_tree(source: &[u8]) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .ok()?;
    let tree = parser.parse(source, None)?;
    (!tree.root_node().has_error()).then_some(tree)
}

fn payload_caller_range_for_id(
    root: tree_sitter::Node<'_>,
    file_id: &SourceFileId,
    id: &DeclarationId,
) -> Option<SourceRange> {
    let mut declarations = Vec::new();
    for kind in [
        "function_declaration",
        "generator_function_declaration",
        "variable_declarator",
    ] {
        collect_nodes_of_kind(root, kind, &mut declarations);
    }
    declarations.into_iter().find_map(|declaration| {
        let range = SourceRange::new(
            declaration.start_byte() as u64,
            declaration.end_byte() as u64,
        )
        .expect("tree-sitter node ranges are ordered");
        (DeclarationId::from_source(file_id.clone(), range) == *id)
            .then(|| exported_declaration_range(declaration).unwrap_or(range))
    })
}

fn exported_declaration_range(node: tree_sitter::Node<'_>) -> Option<SourceRange> {
    let export = match node.parent() {
        Some(parent) if parent.kind() == "export_statement" => Some(parent),
        Some(parent) if parent.kind() == "lexical_declaration" => parent
            .parent()
            .filter(|grandparent| grandparent.kind() == "export_statement"),
        _ => None,
    }?;
    SourceRange::new(export.start_byte() as u64, export.end_byte() as u64).ok()
}

fn collect_nodes_of_kind<'tree>(
    node: tree_sitter::Node<'tree>,
    kind: &str,
    output: &mut Vec<tree_sitter::Node<'tree>>,
) {
    if node.kind() == kind {
        output.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_nodes_of_kind(child, kind, output);
    }
}

fn range_value(range: SourceRange) -> Value {
    json!({ "end": range.end(), "start": range.start() })
}

fn visibility_wire(value: VisibilityValueV1) -> &'static str {
    match value {
        VisibilityValueV1::Exported => "exported",
        VisibilityValueV1::NonExported => "non_exported",
        VisibilityValueV1::Unknown => "unknown",
    }
}

fn scope_kind_wire(value: ScopeKindV1) -> &'static str {
    match value {
        ScopeKindV1::FileLexical => "file_lexical",
    }
}

fn import_kind_wire(value: PayloadImportKindV1) -> &'static str {
    match value {
        PayloadImportKindV1::Named => "named",
        PayloadImportKindV1::Default => "default",
        PayloadImportKindV1::TypeOnly => "type_only",
        PayloadImportKindV1::Namespace => "namespace",
    }
}

fn source_mismatch() -> PayloadError {
    PayloadError::SourceMismatch(AccountingMismatch {
        missing: vec!["source payload".to_owned()],
        extra: vec!["submitted payload".to_owned()],
    })
}
