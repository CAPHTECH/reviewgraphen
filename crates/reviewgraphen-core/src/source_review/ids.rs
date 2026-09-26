//! Source-derived identities and the validation capability for source review.
//!
//! These types deliberately prevent the TypeScript adapter from making an edge
//! from display names.  The design requires endpoints, ranges, and reports to
//! remain bound to the admitted source.

use super::basis::SourceSyntaxRole;
use super::reasons::{TypeScriptSyntaxKind, VocabularyError};
use super::registry::{TypeScriptRegistryBinding, validate_typescript_binding};
use crate::{ContentHash, canonical_json};
use serde_json::json;
use std::fmt;

/// A canonical file identity supplied by the admitted source-review basis.
///
/// The value is not a repository path: its producer must bind it to the
/// snapshot and source hash before a declaration, callsite, or witness uses it
///.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceFileId(String);

impl SourceFileId {
    /// Creates an ID only from the basis-owned canonical file identity.
    #[must_use]
    pub fn from_basis_file_key(canonical_file_key: CanonicalFileKey) -> Self {
        Self(canonical_file_key.0)
    }

    /// Returns the sole name-free key used when this ID enters another canonical
    /// preimage. It is UTF-8 `canonical_json` text of
    /// `{"basis_file_key": BASIS_FILE_KEY, "domain": "source_review_identity.v1",
    /// "kind": "source_file"}`. `BASIS_FILE_KEY` is the admitted canonical
    /// file key, not a declaration binding or display label; snapshot, producer,
    /// and registry binding remain on their owning enclosing preimages.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        canonical_identity_key(json!({
            "basis_file_key": self.0,
            "domain": "source_review_identity.v1",
            "kind": "source_file",
        }))
    }
}

/// A canonical key reconstructed from an admitted source draft, never a label.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CanonicalFileKey(String);

impl CanonicalFileKey {
    /// Receives a path key only at the admitted-basis boundary. Callers still
    /// need [`SourceFileId::from_basis_file_key`] before using it as identity.
    /// Invalid, non-repository-relative paths are rejected as a typed error;
    /// callers must not turn this admission failure into a panic.
    pub fn from_basis_path(canonical_path: &str) -> Result<Self, SourceIdentityError> {
        if canonical_path.is_empty()
            || canonical_path.starts_with('/')
            || canonical_path.contains('\\')
            || canonical_path.as_bytes().contains(&0)
            || canonical_path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(SourceIdentityError::InvalidBasisPath);
        }
        Ok(Self(canonical_path.to_owned()))
    }
}

/// A UTF-8 byte, half-open source range.
///
/// Construction validates `start <= end`; callers must not substitute line,
/// UTF-16, or inferred ranges.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceRange {
    start: u64,
    end: u64,
}

impl SourceRange {
    /// Creates a checked byte range for a node already observed in source.
    pub fn new(_start: u64, _end: u64) -> Result<Self, SourceIdentityError> {
        if _start > _end {
            return Err(SourceIdentityError::InvalidRange);
        }
        Ok(Self {
            start: _start,
            end: _end,
        })
    }

    /// Returns the inclusive UTF-8 byte start of this source-observed range.
    #[must_use]
    pub fn start(&self) -> u64 {
        self.start
    }

    /// Returns the exclusive UTF-8 byte end of this source-observed range.
    #[must_use]
    pub fn end(&self) -> u64 {
        self.end
    }
}

#[cfg(test)]
mod source_range_tests {
    use super::{SourceIdentityError, SourceRange};

    #[test]
    fn source_range_rejects_inverted_byte_interval() {
        assert_eq!(
            SourceRange::new(4, 3),
            Err(SourceIdentityError::InvalidRange)
        );
    }
}

/// A declaration identity: exactly a source file identity plus its declaration
/// node range.  No public name-based constructor exists.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeclarationId {
    file_id: SourceFileId,
    declaration_range: SourceRange,
}

impl DeclarationId {
    /// Builds a declaration ID from its source file and full declaration node.
    #[must_use]
    pub fn from_source(_file_id: SourceFileId, _declaration_range: SourceRange) -> Self {
        Self {
            file_id: _file_id,
            declaration_range: _declaration_range,
        }
    }

    /// Returns the sole name-free declaration key for canonical preimages. It is
    /// UTF-8 `canonical_json` text of `{"declaration_range":{"end": END,
    /// "start": START}, "domain": "source_review_identity.v1", "file_key":
    /// FILE_KEY, "kind": "declaration"}`, where `FILE_KEY` is
    /// [`SourceFileId::canonical_key`]. A binding/FQN/display name is never an
    /// input to this representation.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        canonical_identity_key(json!({
            "declaration_range": {
                "end": self.declaration_range.end(),
                "start": self.declaration_range.start(),
            },
            "domain": "source_review_identity.v1",
            "file_key": self.file_id.canonical_key(),
            "kind": "declaration",
        }))
    }
}

/// The supported top-level callable enclosing a callsite.
///
/// This wrapper forbids using a caller display name as an edge endpoint
///.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CallerId(DeclarationId);

impl CallerId {
    /// Binds a caller only to the declaration ID of its enclosing callable.
    #[must_use]
    pub fn from_declaration(_declaration: DeclarationId) -> Self {
        Self(_declaration)
    }

    /// Returns the canonical caller key: UTF-8 `canonical_json` text of
    /// `{"declaration_key": DECLARATION_KEY, "domain":
    /// "source_review_identity.v1", "kind": "caller"}`. `DECLARATION_KEY`
    /// is the enclosing [`DeclarationId::canonical_key`]; no callable name is
    /// encoded.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        canonical_identity_key(json!({
            "declaration_key": self.0.canonical_key(),
            "domain": "source_review_identity.v1",
            "kind": "caller",
        }))
    }
}

/// One observed source call expression, optionally contained by a caller.
///
/// Module-level calls intentionally carry `None`; they never receive an
/// invented caller ID.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CallsiteId {
    file_id: SourceFileId,
    callsite_range: SourceRange,
    caller_id: Option<CallerId>,
}

impl CallsiteId {
    /// Builds a source-anchored callsite identity and records its containment.
    #[must_use]
    pub fn from_source(
        _file_id: SourceFileId,
        _callsite_range: SourceRange,
        _caller_id: Option<CallerId>,
    ) -> Self {
        Self {
            file_id: _file_id,
            callsite_range: _callsite_range,
            caller_id: _caller_id,
        }
    }

    /// Returns the canonical callsite key: UTF-8 `canonical_json` text of
    /// `{"caller_key": CALLER_KEY_OR_NULL, "callsite_range":{"end": END,
    /// "start": START}, "domain": "source_review_identity.v1", "file_key":
    /// FILE_KEY, "kind": "callsite"}`. `FILE_KEY` and `CALLER_KEY` are the
    /// corresponding typed canonical keys; `null` records a module-level call.
    /// Binding text is not encoded.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        canonical_identity_key(json!({
            "caller_key": self.caller_id.as_ref().map(CallerId::canonical_key),
            "callsite_range": {
                "end": self.callsite_range.end(),
                "start": self.callsite_range.start(),
            },
            "domain": "source_review_identity.v1",
            "file_key": self.file_id.canonical_key(),
            "kind": "callsite",
        }))
    }
}

/// A pair is made only from declaration-backed endpoints, never `String` names.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DeclarationPairId {
    caller_id: CallerId,
    callee_id: DeclarationId,
}

impl DeclarationPairId {
    /// Constructs an edge pair from the only two permitted endpoint ID types.
    #[must_use]
    pub fn new(_caller_id: CallerId, _callee_id: DeclarationId) -> Self {
        Self {
            caller_id: _caller_id,
            callee_id: _callee_id,
        }
    }

    /// Returns the canonical relation key: UTF-8 `canonical_json` text of
    /// `{"callee_key": CALLEE_KEY, "caller_key": CALLER_KEY, "domain":
    /// "source_review_identity.v1", "kind": "declaration_pair"}`. The two
    /// values are [`DeclarationId::canonical_key`] and
    /// [`CallerId::canonical_key`], respectively, so pair identity cannot depend
    /// on a caller or callee binding name.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        canonical_identity_key(json!({
            "callee_key": self.callee_id.canonical_key(),
            "caller_key": self.caller_id.canonical_key(),
            "domain": "source_review_identity.v1",
            "kind": "declaration_pair",
        }))
    }
}

/// A snapshot-bound identity for an immutable source byte sequence.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceIdentity {
    file_id: SourceFileId,
    source_hash: SourceHash,
}

impl SourceIdentity {
    /// Binds a source identity to the admitted file and exact bytes hash.
    #[must_use]
    pub fn from_admitted_source(_file_id: SourceFileId, _source_hash: SourceHash) -> Self {
        Self {
            file_id: _file_id,
            source_hash: _source_hash,
        }
    }
}

/// A content hash already computed by the admitted source producer.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceHash(String);

impl SourceHash {
    /// Receives exact bytes only at the source-admission boundary.
    #[must_use]
    pub fn from_source_bytes(bytes: &[u8]) -> Self {
        Self(ContentHash::sha256(bytes).as_str().to_owned())
    }

    /// Returns the already-admitted source hash for serialization. This
    /// observation accessor neither creates a hash nor grants admission
    /// authority.
    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

/// A registry-checked raw syntax key. Its representation remains private so a
/// caller cannot manufacture an owner/role reference from arbitrary text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntaxKeyV1(String);

impl SyntaxKeyV1 {
    pub fn parse_wire(
        _binding: &TypeScriptRegistryBinding,
        _wire: &str,
    ) -> Result<Self, VocabularyError> {
        if validate_typescript_binding(_binding).is_ok()
            && ContentHash::parse(_wire.to_owned()).is_ok()
        {
            Ok(Self(_wire.to_ascii_lowercase()))
        } else {
            Err(VocabularyError {
                rejected_wire: _wire.to_owned(),
            })
        }
    }

    /// Derives the source-bound syntax key of one record.
    /// The origin rule and descriptor are derived from `role` through the bound
    /// arm, never supplied by the caller. This derives a value only; it mints no
    /// capability.
    #[must_use]
    pub fn derive_from_source(
        _binding: &TypeScriptRegistryBinding,
        _snapshot: &SnapshotBinding,
        _file_id: &SourceFileId,
        _role: SourceSyntaxRole,
        _kind: &TypeScriptSyntaxKind,
        _range: SourceRange,
    ) -> Self {
        let (record_role, origin_rule_id, descriptor_id) = syntax_key_role_fields(_role);
        let tuple_hash = registry_tuple_hash(_binding);
        let preimage = json!({
            "descriptor_id": descriptor_id,
            "domain": "syntax_key.v1",
            "file_key": _file_id.canonical_key(),
            "kind_id": _kind.wire_literal(),
            "origin_rule_id": origin_rule_id,
            "range": { "end": _range.end(), "start": _range.start() },
            "record_role": record_role,
            "registry_hash": _binding.registry_hash,
            "snapshot_id": _snapshot.as_str(),
            "tuple_hash": tuple_hash,
        });
        let canonical =
            canonical_json(&preimage).expect("fixed syntax-key preimage is always canonical JSON");
        Self(ContentHash::sha256(&canonical).as_str().to_owned())
    }

    #[must_use]
    pub fn wire_literal(&self) -> &str {
        &self.0
    }
}

fn syntax_key_role_fields(
    role: SourceSyntaxRole,
) -> (&'static str, Option<&'static str>, &'static str) {
    match role {
        SourceSyntaxRole::Callable => (
            "callable",
            Some("node.public_function_contract@2"),
            "reviewgraphen.typescript_syntax.callable@1",
        ),
        SourceSyntaxRole::Call => (
            "call",
            Some("relation.changed_public_callee@2"),
            "reviewgraphen.typescript_syntax.call@1",
        ),
        SourceSyntaxRole::Binding => ("binding", None, "reviewgraphen.typescript_syntax.binding@1"),
        SourceSyntaxRole::Surface => ("surface", None, "reviewgraphen.typescript_syntax.surface@1"),
        SourceSyntaxRole::Scope => ("scope", None, "reviewgraphen.typescript_syntax.scope@1"),
    }
}

/// Returns the canonical hash of the bound seven-field registry tuple.
///
/// The syntax-key and snapshot-binding preimages share this one derivation;
/// neither consumer owns a second tuple serialization.
#[must_use]
pub fn registry_tuple_hash(binding: &TypeScriptRegistryBinding) -> String {
    let tuple = &binding.tuple;
    let canonical = canonical_json(&json!({
        "extractor_set_hash": tuple.extractor_set_hash,
        "language": tuple.language,
        "producer_id": tuple.producer_id,
        "profile_id": tuple.profile_id,
        "profile_version": tuple.profile_version,
        "projection_id": tuple.projection_id,
        "rule_set_hash": tuple.rule_set_hash,
    }))
    .expect("fixed registry tuple is always canonical JSON");
    ContentHash::sha256(&canonical).as_str().to_owned()
}

/// A registry-checked basis endpoint key with no unchecked constructor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BasisEndpointKeyV1(String);

impl BasisEndpointKeyV1 {
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

/// The only two source-witness forms permitted in the r2 raw DTO surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceWitnessKeyV1 {
    Syntax(SyntaxKeyV1),
    BasisEndpoint(BasisEndpointKeyV1),
}

/// A declaration/range witness used by changed-edge evidence.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ChangeWitnessRef {
    pub declaration_id: DeclarationId,
    pub witness_range: SourceRange,
}

/// The stable, materialized identity of one review obligation.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ObligationId(String);

impl ObligationId {
    /// Exposes the materialized canonical ID for fixture comparison after
    /// source validation; it does not construct IDs from arbitrary text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A canonical snapshot binding used in an obligation preimage.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SnapshotBinding(String);

impl SnapshotBinding {
    /// Receives the immutable snapshot binding from an already admitted basis.
    ///
    /// I3 admission, not this string wrapper, verifies repository identity,
    /// base/target OIDs, tree OIDs, and the profile/extractor/rule tuple before
    /// calling this function.  Callers must therefore pass only that I3
    /// admission result; this I2 constructor cannot establish those facts from
    /// `&str`.
    #[must_use]
    pub fn from_admitted_binding(binding: &str) -> Self {
        Self(ContentHash::sha256(binding.as_bytes()).as_str().to_owned())
    }

    /// Exposes the admitted snapshot binding for independent fixture comparison.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Source comparison failed: the reconstructed and submitted canonical drafts
/// differ by at least one source-derived record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountingMismatch {
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl fmt::Display for AccountingMismatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "source accounting mismatch: missing {:?}, extra {:?}",
            self.missing, self.extra
        )
    }
}

impl std::error::Error for AccountingMismatch {}

/// Identity construction failed because source identity or its range is invalid.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceIdentityError {
    /// The byte interval is not a valid half-open interval.
    InvalidRange,
    /// The admitted basis path is not a repository-relative canonical key.
    InvalidBasisPath,
}

impl fmt::Display for SourceIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRange => formatter.write_str("source range has an inverted interval"),
            Self::InvalidBasisPath => formatter.write_str("source basis path is not canonical"),
        }
    }
}

impl std::error::Error for SourceIdentityError {}

fn canonical_identity_key(value: serde_json::Value) -> String {
    String::from_utf8(canonical_json(&value).expect("fixed source identity is canonical JSON"))
        .expect("canonical JSON is UTF-8")
}
