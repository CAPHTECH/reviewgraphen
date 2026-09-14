//! Fixed, deterministic Rust production review profiles.
//!
//! This module owns the profile's path grammar, matcher precedence, canonical
//! DTO, and D-rule exclusion identity.  It deliberately does not infer a
//! profile from filesystem state, Cargo metadata, or program attributes.

use crate::{ContentHash, StableId, canonical_json};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// The sole profile identifier admitted by the D `@1` rule.
pub const RUST_PRODUCTION_PROFILE_ID: &str = "rust.production.v1";
/// Production profile excluding conventional benchmark trees.
pub const RUST_PRODUCTION_V2_PROFILE_ID: &str = "rust.production.v2";
/// The sole profile schema identifier admitted by the D `@1` rule.
pub const REVIEW_PROFILE_SCHEMA: &str = "reviewgraphen.review_profile.v1";
/// The fixed profile content hash.
pub const RUST_PRODUCTION_PROFILE_HASH: &str =
    "sha256:4b6cca93794ab03b1576e17d2e395ec43f731d316685247a89363ae2e840dd96";
/// Fixed v2 profile content hash.
pub const RUST_PRODUCTION_V2_PROFILE_HASH: &str =
    "sha256:6f4ec559f8963f0abcc2a78718224fa0c1bd7c25c02302eb8066a7dd3915239d";
/// The D rule that is bound into a profile exclusion record.
pub const CHANGED_PUBLIC_CALLEE_RULE: &str = "relation.changed_public_callee@1";
/// The Node rule admitted only by the production-v4 mixed rule set.
pub const PUBLIC_FUNCTION_NODE_RULE: &str = "node.public_function_contract@1";
/// The retained exclusion weight, represented as an exact decimal string.
pub const D_EXCLUDED_WEIGHT: &str = "4.0";
/// The exact substantive D-obligation weight retained through planning.
pub const D_OBLIGATION_WEIGHT: &str = "4.0";
/// The exact substantive Node-obligation weight retained through planning.
pub const NODE_OBLIGATION_WEIGHT: &str = "3.0";

const CANONICAL_PROFILE_JSON: &str = r#"{"category_precedence":["vendor","generated","test","example","docs"],"exclusion_matchers":[{"category":"vendor","id":"path.vendor_component@1","operator":"component_equals_any","values":["third_party","vendor","vendored"]},{"category":"generated","id":"path.generated_component@1","operator":"component_equals_any","values":["generated","target"]},{"category":"generated","id":"path.generated_suffix@1","operator":"basename_suffix_any","values":[".generated.rs"]},{"category":"test","id":"path.test_component@1","operator":"component_equals_any","values":["benches","tests"]},{"category":"test","id":"path.test_basename@1","operator":"basename_equals_any","values":["test.rs","tests.rs"]},{"category":"test","id":"path.test_suffix@1","operator":"basename_suffix_any","values":["_test.rs","_tests.rs"]},{"category":"example","id":"path.example_component@1","operator":"component_equals_any","values":["example","examples"]},{"category":"docs","id":"path.docs_component@1","operator":"component_equals_any","values":["doc","docs"]}],"id":"rust.production.v1","path_normalization":{"absolute":"reject","backslash":"reject","case_fold":false,"dot":"reject","dot_dot":"reject","empty_component":"reject","encoding":"utf-8","nul":"reject","separator":"/","unicode_normalization":"none"},"reason_ids":{"docs":"profile.exclude.docs@1","example":"profile.exclude.example@1","generated":"profile.exclude.generated@1","test":"profile.exclude.test@1","vendor":"profile.exclude.vendor@1"},"rust_source":{"basename_suffix":".rs","case_sensitive":true},"schema":"reviewgraphen.review_profile.v1"}"#;

const CANONICAL_PROFILE_V2_JSON: &str = r#"{"category_precedence":["vendor","generated","test","example","docs"],"exclusion_matchers":[{"category":"vendor","id":"path.vendor_component@1","operator":"component_equals_any","values":["third_party","vendor","vendored"]},{"category":"generated","id":"path.generated_component@1","operator":"component_equals_any","values":["generated","target"]},{"category":"generated","id":"path.generated_suffix@1","operator":"basename_suffix_any","values":[".generated.rs"]},{"category":"test","id":"path.test_component@2","operator":"component_equals_any","values":["benches","benchmarks","tests"]},{"category":"test","id":"path.test_basename@1","operator":"basename_equals_any","values":["test.rs","tests.rs"]},{"category":"test","id":"path.test_suffix@1","operator":"basename_suffix_any","values":["_test.rs","_tests.rs"]},{"category":"example","id":"path.example_component@1","operator":"component_equals_any","values":["example","examples"]},{"category":"docs","id":"path.docs_component@1","operator":"component_equals_any","values":["doc","docs"]}],"id":"rust.production.v2","path_normalization":{"absolute":"reject","backslash":"reject","case_fold":false,"dot":"reject","dot_dot":"reject","empty_component":"reject","encoding":"utf-8","nul":"reject","separator":"/","unicode_normalization":"none"},"reason_ids":{"docs":"profile.exclude.docs@1","example":"profile.exclude.example@1","generated":"profile.exclude.generated@1","test":"profile.exclude.test@1","vendor":"profile.exclude.vendor@1"},"rust_source":{"basename_suffix":".rs","case_sensitive":true},"schema":"reviewgraphen.review_profile.v1"}"#;

const MATCHERS: &[Matcher] = &[
    Matcher::new(
        Category::Vendor,
        "path.vendor_component@1",
        Operator::ComponentEqualsAny,
        &["third_party", "vendor", "vendored"],
    ),
    Matcher::new(
        Category::Generated,
        "path.generated_component@1",
        Operator::ComponentEqualsAny,
        &["generated", "target"],
    ),
    Matcher::new(
        Category::Generated,
        "path.generated_suffix@1",
        Operator::BasenameSuffixAny,
        &[".generated.rs"],
    ),
    Matcher::new(
        Category::Test,
        "path.test_component@1",
        Operator::ComponentEqualsAny,
        &["benches", "tests"],
    ),
    Matcher::new(
        Category::Test,
        "path.test_basename@1",
        Operator::BasenameEqualsAny,
        &["test.rs", "tests.rs"],
    ),
    Matcher::new(
        Category::Test,
        "path.test_suffix@1",
        Operator::BasenameSuffixAny,
        &["_test.rs", "_tests.rs"],
    ),
    Matcher::new(
        Category::Example,
        "path.example_component@1",
        Operator::ComponentEqualsAny,
        &["example", "examples"],
    ),
    Matcher::new(
        Category::Docs,
        "path.docs_component@1",
        Operator::ComponentEqualsAny,
        &["doc", "docs"],
    ),
];

const MATCHERS_V2: &[Matcher] = &[
    MATCHERS[0],
    MATCHERS[1],
    MATCHERS[2],
    Matcher::new(
        Category::Test,
        "path.test_component@2",
        Operator::ComponentEqualsAny,
        &["benches", "benchmarks", "tests"],
    ),
    MATCHERS[4],
    MATCHERS[5],
    MATCHERS[6],
    MATCHERS[7],
];

/// A closed profile category, used only for syntactic path classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Vendored or third-party source.
    Vendor,
    /// Generated source.
    Generated,
    /// Test or benchmark source.
    Test,
    /// Example source.
    Example,
    /// Documentation source.
    Docs,
}

impl Category {
    const fn reason_id(self) -> &'static str {
        match self {
            Self::Vendor => "profile.exclude.vendor@1",
            Self::Generated => "profile.exclude.generated@1",
            Self::Test => "profile.exclude.test@1",
            Self::Example => "profile.exclude.example@1",
            Self::Docs => "profile.exclude.docs@1",
        }
    }
}

/// A closed matcher operator from the fixed profile DTO.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// Match a complete path component.
    ComponentEqualsAny,
    /// Match the complete basename.
    BasenameEqualsAny,
    /// Match a suffix of the basename.
    BasenameSuffixAny,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Matcher {
    category: Category,
    id: &'static str,
    operator: Operator,
    values: &'static [&'static str],
}

impl Matcher {
    const fn new(
        category: Category,
        id: &'static str,
        operator: Operator,
        values: &'static [&'static str],
    ) -> Self {
        Self {
            category,
            id,
            operator,
            values,
        }
    }

    fn matches(self, path: &str) -> bool {
        let basename = path.rsplit('/').next().expect("validated nonempty path");
        match self.operator {
            Operator::ComponentEqualsAny => path
                .split('/')
                .any(|component| self.values.contains(&component)),
            Operator::BasenameEqualsAny => self.values.contains(&basename),
            Operator::BasenameSuffixAny => {
                self.values.iter().any(|suffix| basename.ends_with(suffix))
            }
        }
    }
}

/// The closed reasons that can make a Git profile path ineligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvalidPathReason {
    /// Input bytes were not valid UTF-8.
    InvalidUtf8,
    /// The path was empty.
    Empty,
    /// The path contained a NUL byte.
    Nul,
    /// The path used a backslash rather than the Git `/` separator.
    Backslash,
    /// The path was absolute.
    Absolute,
    /// The path contained an empty `/`-separated component.
    EmptyComponent,
    /// The path contained a `.` component.
    Dot,
    /// The path contained a `..` component.
    DotDot,
}

/// A typed obstruction produced while evaluating the fixed profile.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProfileError {
    /// The supplied profile DTO is not the one frozen by this contract.
    #[error("review profile DTO is not an exact supported rust.production canonical value")]
    InvalidCanonicalProfile,
    /// A repository-relative Git path could not be classified.
    #[error("invalid {endpoint} profile path: {reason:?}")]
    InvalidPath {
        /// Which candidate endpoint supplied the path.
        endpoint: &'static str,
        /// Closed reason for rejection.
        reason: InvalidPathReason,
    },
    /// The caller or callee did not have an accepted location path.
    #[error("missing {endpoint} profile path")]
    MissingPath {
        /// Which candidate endpoint had no path.
        endpoint: &'static str,
    },
    /// An exclusion identity binding was malformed.
    #[error("invalid D exclusion record: {0}")]
    InvalidExclusion(String),
    /// A Stage 0 fan-out or deferred-set input was not the exact fixed domain.
    #[error("invalid Stage 0 profile input: {0}")]
    InvalidStage0(String),
}

/// The result type returned by profile evaluation and exclusion construction.
pub type ProfileResult<T> = std::result::Result<T, ProfileError>;

/// The fixed review profile.  Construction never reads the filesystem.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewProfile {
    revision: ProfileRevision,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProfileRevision {
    V1,
    V2,
}

impl Default for ReviewProfile {
    fn default() -> Self {
        rust_production_v1()
    }
}

/// Compatibility alias for the fixed Rust production profile implementation.
pub type RustProductionProfile = ReviewProfile;

/// Returns the one admitted profile for `relation.changed_public_callee@1`.
#[must_use]
pub const fn rust_production_v1() -> ReviewProfile {
    ReviewProfile {
        revision: ProfileRevision::V1,
    }
}

/// Returns production v2, which additionally excludes `benchmarks` trees.
#[must_use]
pub const fn rust_production_v2() -> ReviewProfile {
    ReviewProfile {
        revision: ProfileRevision::V2,
    }
}

/// Resolves a supported profile ID without inferring policy from the tree.
#[must_use]
pub fn rust_production_profile(id: &str) -> Option<ReviewProfile> {
    match id {
        RUST_PRODUCTION_PROFILE_ID => Some(rust_production_v1()),
        RUST_PRODUCTION_V2_PROFILE_ID => Some(rust_production_v2()),
        _ => None,
    }
}

impl ReviewProfile {
    /// Reads the fixed DTO, requiring exact JCS canonical bytes and hash.
    pub fn from_canonical_bytes(bytes: &[u8]) -> ProfileResult<Self> {
        let value: Value =
            serde_json::from_slice(bytes).map_err(|_| ProfileError::InvalidCanonicalProfile)?;
        let recanonicalized =
            canonical_json(&value).map_err(|_| ProfileError::InvalidCanonicalProfile)?;
        if recanonicalized != bytes {
            return Err(ProfileError::InvalidCanonicalProfile);
        }
        let profile = if bytes == CANONICAL_PROFILE_JSON.as_bytes() {
            rust_production_v1()
        } else if bytes == CANONICAL_PROFILE_V2_JSON.as_bytes() {
            rust_production_v2()
        } else {
            return Err(ProfileError::InvalidCanonicalProfile);
        };
        if ContentHash::sha256(bytes).as_str() != profile.expected_hash() {
            return Err(ProfileError::InvalidCanonicalProfile);
        }
        Ok(profile)
    }

    /// Returns the profile identifier bound into baseline packets and exclusions.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self.revision {
            ProfileRevision::V1 => RUST_PRODUCTION_PROFILE_ID,
            ProfileRevision::V2 => RUST_PRODUCTION_V2_PROFILE_ID,
        }
    }

    /// Returns the profile schema identifier.
    #[must_use]
    pub const fn schema(self) -> &'static str {
        REVIEW_PROFILE_SCHEMA
    }

    /// Returns the exact fixed JCS canonical UTF-8 bytes, without a newline.
    #[must_use]
    pub const fn canonical_bytes(self) -> &'static [u8] {
        match self.revision {
            ProfileRevision::V1 => CANONICAL_PROFILE_JSON.as_bytes(),
            ProfileRevision::V2 => CANONICAL_PROFILE_V2_JSON.as_bytes(),
        }
    }

    /// Returns the SHA-256 hash of [`Self::canonical_bytes`].
    #[must_use]
    pub fn hash(self) -> ContentHash {
        ContentHash::sha256(self.canonical_bytes())
    }

    /// Classifies one validated path according to the fixed matcher precedence.
    pub fn classify_path(self, path: &str) -> ProfileResult<PathClassification> {
        validate_path(path, "path")?;
        Ok(classify_valid_path(path, self.matchers()))
    }

    const fn expected_hash(self) -> &'static str {
        match self.revision {
            ProfileRevision::V1 => RUST_PRODUCTION_PROFILE_HASH,
            ProfileRevision::V2 => RUST_PRODUCTION_V2_PROFILE_HASH,
        }
    }

    const fn matchers(self) -> &'static [Matcher] {
        match self.revision {
            ProfileRevision::V1 => MATCHERS,
            ProfileRevision::V2 => MATCHERS_V2,
        }
    }

    /// Classifies raw Git-path bytes, rejecting non-UTF-8 input before matching.
    pub fn classify_path_bytes(self, path: &[u8]) -> ProfileResult<PathClassification> {
        let path = std::str::from_utf8(path).map_err(|_| ProfileError::InvalidPath {
            endpoint: "path",
            reason: InvalidPathReason::InvalidUtf8,
        })?;
        self.classify_path(path)
    }

    /// Classifies D endpoints in the ADR-required callee, then caller order.
    ///
    /// Invalid or absent endpoints are returned as typed obstructions rather
    /// than profile exclusions, so the caller can retain the target as unknown.
    pub fn classify_candidate(
        self,
        callee_path: Option<&[u8]>,
        caller_path: Option<&[u8]>,
    ) -> ProfileResult<CandidateClassification> {
        let callee = endpoint_path(callee_path, "callee")?;
        let caller = endpoint_path(caller_path, "caller")?;
        for category in [
            Category::Vendor,
            Category::Generated,
            Category::Test,
            Category::Example,
            Category::Docs,
        ] {
            for matcher in self
                .matchers()
                .iter()
                .copied()
                .filter(|matcher| matcher.category == category)
            {
                for (endpoint, path) in [
                    (CandidateEndpoint::Callee, callee),
                    (CandidateEndpoint::Caller, caller),
                ] {
                    if matcher.matches(path) {
                        return Ok(CandidateClassification::Excluded(ProfileMatch {
                            category,
                            matcher_id: matcher.id,
                            endpoint,
                        }));
                    }
                }
            }
        }
        Ok(CandidateClassification::Included)
    }

    /// Builds the D-specific stable exclusion record from a profile match.
    ///
    /// The returned record retains the excluded weight and sorted source trace;
    /// it must remain visible outside the eligible coverage denominator.
    pub fn exclusion_record(
        self,
        candidate: DExclusionCandidate,
        profile_match: &ProfileMatch,
    ) -> ProfileResult<DExclusionRecord> {
        let mut source_ids = BTreeSet::from([
            candidate.relation_id.clone(),
            candidate.caller_id,
            candidate.callee_id,
        ]);
        source_ids.extend(candidate.change_artifact_ids);
        source_ids.extend(candidate.containment_witness_ids);
        DExclusionRecord::new(DExclusionBindings {
            snapshot_id: candidate.snapshot_id,
            candidate_key: format!("{CHANGED_PUBLIC_CALLEE_RULE}|{}", candidate.relation_id),
            rule: CHANGED_PUBLIC_CALLEE_RULE.to_owned(),
            profile_id: self.id().to_owned(),
            profile_hash: self.hash(),
            reason_id: profile_match.reason_id().to_owned(),
            matcher_id: profile_match.matcher_id().to_owned(),
            excluded_weight: D_EXCLUDED_WEIGHT.to_owned(),
            source_ids,
        })
    }

    /// Builds a rule-neutral profile exclusion record for one closed candidate
    /// family.  The legacy D wrapper remains byte-identical; this API is used
    /// by production-v4 Node synthesis without changing the profile bytes.
    pub fn exclusion_record_for_candidate(
        self,
        candidate: ProfileExclusionCandidate,
        profile_match: &ProfileMatch,
    ) -> ProfileResult<ProfileExclusionRecord> {
        let snapshot_id = match &candidate {
            ProfileExclusionCandidate::ChangedPublicCallee { snapshot_id, .. }
            | ProfileExclusionCandidate::PublicFunctionNode { snapshot_id, .. } => {
                snapshot_id.clone()
            }
        };
        let (rule, target_id, excluded_weight, source_ids) = match candidate {
            ProfileExclusionCandidate::ChangedPublicCallee {
                snapshot_id: _,
                relation_id,
                caller_id,
                callee_id,
                change_artifact_ids,
                containment_witness_ids,
            } => {
                let mut source_ids = BTreeSet::from([relation_id.clone(), caller_id, callee_id]);
                source_ids.extend(change_artifact_ids);
                source_ids.extend(containment_witness_ids);
                (
                    CHANGED_PUBLIC_CALLEE_RULE,
                    relation_id,
                    D_EXCLUDED_WEIGHT.to_owned(),
                    source_ids,
                )
            }
            ProfileExclusionCandidate::PublicFunctionNode {
                snapshot_id: _,
                function_id,
                owning_module_ids,
                containment_witness_ids,
            } => {
                let mut source_ids = BTreeSet::from([function_id.clone()]);
                source_ids.extend(owning_module_ids);
                source_ids.extend(containment_witness_ids);
                (
                    PUBLIC_FUNCTION_NODE_RULE,
                    function_id,
                    NODE_OBLIGATION_WEIGHT.to_owned(),
                    source_ids,
                )
            }
        };
        ProfileExclusionRecord::new(ProfileExclusionBindings {
            snapshot_id,
            candidate_key: format!("{rule}|{target_id}"),
            rule: rule.to_owned(),
            profile_id: self.id().to_owned(),
            profile_hash: self.hash(),
            reason_id: profile_match.reason_id().to_owned(),
            matcher_id: profile_match.matcher_id().to_owned(),
            excluded_weight,
            source_ids,
        })
    }

    /// Builds a rule-neutral exclusion record from a single-path profile
    /// match.  Node candidates have no caller/callee tie-breaker.
    pub fn exclusion_record_for_path_candidate(
        self,
        candidate: ProfileExclusionCandidate,
        profile_match: PathMatch,
    ) -> ProfileResult<ProfileExclusionRecord> {
        self.exclusion_record_for_candidate(
            candidate,
            &ProfileMatch {
                category: profile_match.category,
                matcher_id: profile_match.matcher_id,
                endpoint: CandidateEndpoint::Callee,
            },
        )
    }
}

/// Closed candidate input for profile exclusions in versioned rule families.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileExclusionCandidate {
    /// Legacy changed-public-callee relation candidate.
    ChangedPublicCallee {
        snapshot_id: StableId,
        relation_id: StableId,
        caller_id: StableId,
        callee_id: StableId,
        change_artifact_ids: BTreeSet<StableId>,
        containment_witness_ids: BTreeSet<StableId>,
    },
    /// Production-v4 public free-function candidate.
    PublicFunctionNode {
        snapshot_id: StableId,
        function_id: StableId,
        owning_module_ids: BTreeSet<StableId>,
        containment_witness_ids: BTreeSet<StableId>,
    },
}

/// Rule-neutral identity bindings.  Their canonical preimage intentionally
/// remains the legacy D preimage field set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileExclusionBindings {
    pub snapshot_id: StableId,
    pub candidate_key: String,
    pub rule: String,
    pub profile_id: String,
    pub profile_hash: ContentHash,
    pub reason_id: String,
    pub matcher_id: String,
    pub excluded_weight: String,
    pub source_ids: BTreeSet<StableId>,
}

/// Source-traceable exclusion record for a versioned rule family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProfileExclusionRecord {
    pub id: StableId,
    pub snapshot_id: StableId,
    pub candidate_key: String,
    pub rule: String,
    pub profile_id: String,
    pub profile_hash: ContentHash,
    pub reason_id: String,
    pub matcher_id: String,
    pub excluded_weight: String,
    pub source_ids: BTreeSet<StableId>,
}

impl ProfileExclusionRecord {
    pub fn new(bindings: ProfileExclusionBindings) -> ProfileResult<Self> {
        validate_profile_exclusion_bindings(&bindings)?;
        let id = profile_exclusion_id(&bindings)?;
        Ok(Self {
            id,
            snapshot_id: bindings.snapshot_id,
            candidate_key: bindings.candidate_key,
            rule: bindings.rule,
            profile_id: bindings.profile_id,
            profile_hash: bindings.profile_hash,
            reason_id: bindings.reason_id,
            matcher_id: bindings.matcher_id,
            excluded_weight: bindings.excluded_weight,
            source_ids: bindings.source_ids,
        })
    }
}

fn validate_profile_exclusion_bindings(bindings: &ProfileExclusionBindings) -> ProfileResult<()> {
    let profile = rust_production_profile(&bindings.profile_id)
        .filter(|profile| bindings.profile_hash.as_str() == profile.expected_hash());
    let Some(profile) = profile else {
        return Err(ProfileError::InvalidExclusion(
            "fixed profile exclusion bindings are not exact".to_owned(),
        ));
    };
    if bindings.snapshot_id.kind() != "snapshot" || bindings.source_ids.is_empty() {
        return Err(ProfileError::InvalidExclusion(
            "fixed profile exclusion bindings are not exact".to_owned(),
        ));
    }
    let (prefix, weight) = match bindings.rule.as_str() {
        CHANGED_PUBLIC_CALLEE_RULE => ("relation:", D_EXCLUDED_WEIGHT),
        PUBLIC_FUNCTION_NODE_RULE => ("function:", NODE_OBLIGATION_WEIGHT),
        _ => {
            return Err(ProfileError::InvalidExclusion(
                "exclusion rule is outside the closed production registry".to_owned(),
            ));
        }
    };
    if !bindings
        .candidate_key
        .starts_with(&format!("{}|{prefix}", bindings.rule))
        || bindings.excluded_weight != weight
        || !profile
            .matchers()
            .iter()
            .any(|matcher| matcher.id == bindings.matcher_id)
    {
        return Err(ProfileError::InvalidExclusion(
            "profile exclusion candidate or matcher is not exact".to_owned(),
        ));
    }
    let matcher = profile
        .matchers()
        .iter()
        .find(|matcher| matcher.id == bindings.matcher_id)
        .expect("matcher was checked above");
    if bindings.reason_id != matcher.category.reason_id() {
        return Err(ProfileError::InvalidExclusion(
            "reason_id does not match matcher category".to_owned(),
        ));
    }
    Ok(())
}

fn profile_exclusion_id(bindings: &ProfileExclusionBindings) -> ProfileResult<StableId> {
    let source_ids = Value::Array(
        bindings
            .source_ids
            .iter()
            .map(|id| Value::String(id.to_string()))
            .collect(),
    );
    let map = BTreeMap::from([
        (
            "candidate_key".to_owned(),
            Value::String(bindings.candidate_key.clone()),
        ),
        (
            "excluded_weight".to_owned(),
            Value::String(bindings.excluded_weight.clone()),
        ),
        (
            "matcher_id".to_owned(),
            Value::String(bindings.matcher_id.clone()),
        ),
        (
            "profile_hash".to_owned(),
            Value::String(bindings.profile_hash.to_string()),
        ),
        (
            "profile_id".to_owned(),
            Value::String(bindings.profile_id.clone()),
        ),
        (
            "reason_id".to_owned(),
            Value::String(bindings.reason_id.clone()),
        ),
        ("rule".to_owned(), Value::String(bindings.rule.clone())),
        (
            "snapshot_id".to_owned(),
            Value::String(bindings.snapshot_id.to_string()),
        ),
        ("source_ids".to_owned(), source_ids),
    ]);
    StableId::derived("exclusion", &map)
        .map_err(|error| ProfileError::InvalidExclusion(error.to_string()))
}

fn endpoint_path<'a>(path: Option<&'a [u8]>, endpoint: &'static str) -> ProfileResult<&'a str> {
    let path = path.ok_or(ProfileError::MissingPath { endpoint })?;
    let path = std::str::from_utf8(path).map_err(|_| ProfileError::InvalidPath {
        endpoint,
        reason: InvalidPathReason::InvalidUtf8,
    })?;
    validate_path(path, endpoint)?;
    Ok(path)
}

fn validate_path(path: &str, endpoint: &'static str) -> ProfileResult<()> {
    let reason = if path.is_empty() {
        Some(InvalidPathReason::Empty)
    } else if path.contains('\0') {
        Some(InvalidPathReason::Nul)
    } else if path.contains('\\') {
        Some(InvalidPathReason::Backslash)
    } else if path.starts_with('/') {
        Some(InvalidPathReason::Absolute)
    } else if path.split('/').any(|component| component.is_empty()) {
        Some(InvalidPathReason::EmptyComponent)
    } else if path.split('/').any(|component| component == ".") {
        Some(InvalidPathReason::Dot)
    } else if path.split('/').any(|component| component == "..") {
        Some(InvalidPathReason::DotDot)
    } else {
        None
    };
    reason.map_or(Ok(()), |reason| {
        Err(ProfileError::InvalidPath { endpoint, reason })
    })
}

fn classify_valid_path(path: &str, matchers: &[Matcher]) -> PathClassification {
    let mut matches = Vec::new();
    for category in [
        Category::Vendor,
        Category::Generated,
        Category::Test,
        Category::Example,
        Category::Docs,
    ] {
        for matcher in matchers
            .iter()
            .copied()
            .filter(|matcher| matcher.category == category)
        {
            if matcher.matches(path) {
                matches.push(PathMatch {
                    category,
                    matcher_id: matcher.id,
                });
            }
        }
    }
    let first = matches.first().copied();
    PathClassification {
        rust_source: path
            .rsplit('/')
            .next()
            .is_some_and(|basename| basename.ends_with(".rs")),
        non_test_rust_source: path
            .rsplit('/')
            .next()
            .is_some_and(|basename| basename.ends_with(".rs"))
            && !matchers
                .iter()
                .filter(|matcher| matcher.category == Category::Test)
                .any(|matcher| matcher.matches(path)),
        production_rust_source: path
            .rsplit('/')
            .next()
            .is_some_and(|basename| basename.ends_with(".rs"))
            && first.is_none(),
        matched: first,
    }
}

/// Syntactic classification of one normalized Git path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathClassification {
    rust_source: bool,
    non_test_rust_source: bool,
    production_rust_source: bool,
    matched: Option<PathMatch>,
}

impl PathClassification {
    /// Whether the basename ends in lowercase `.rs`.
    #[must_use]
    pub const fn is_rust_source(self) -> bool {
        self.rust_source
    }
    /// Whether the path is Rust and matches none of the fixed test matchers.
    #[must_use]
    pub const fn is_non_test_rust_source(self) -> bool {
        self.non_test_rust_source
    }
    /// Whether the path is Rust and matches no fixed profile matcher.
    #[must_use]
    pub const fn is_production_rust_source(self) -> bool {
        self.production_rust_source
    }
    /// The first profile match, if the path is excluded.
    #[must_use]
    pub const fn matched(self) -> Option<PathMatch> {
        self.matched
    }
}

/// A single fixed matcher hit for one path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathMatch {
    category: Category,
    matcher_id: &'static str,
}

impl PathMatch {
    /// The winning category.
    #[must_use]
    pub const fn category(self) -> Category {
        self.category
    }
    /// The winning fixed matcher identifier.
    #[must_use]
    pub const fn matcher_id(self) -> &'static str {
        self.matcher_id
    }
    /// The category's closed typed exclusion reason identifier.
    #[must_use]
    pub const fn reason_id(self) -> &'static str {
        self.category.reason_id()
    }
}

/// The endpoint used as the final tie-break in D candidate classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateEndpoint {
    /// The accepted callee location path.
    Callee,
    /// The accepted caller location path.
    Caller,
}

/// The deterministic profile result for a D relation candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateClassification {
    /// Neither endpoint matched a fixed profile exclusion matcher.
    Included,
    /// One endpoint matched; the match fixes the exclusion reason and identity.
    Excluded(ProfileMatch),
}

/// The one winning matcher and endpoint for an excluded D candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileMatch {
    category: Category,
    matcher_id: &'static str,
    endpoint: CandidateEndpoint,
}

/// Source-bound input for deriving a D profile exclusion record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DExclusionCandidate {
    /// Snapshot that owns the relation candidate.
    pub snapshot_id: StableId,
    /// Accepted direct-call relation ID.
    pub relation_id: StableId,
    /// Accepted caller artifact ID.
    pub caller_id: StableId,
    /// Accepted callee artifact ID.
    pub callee_id: StableId,
    /// Every matching change artifact ID.
    pub change_artifact_ids: BTreeSet<StableId>,
    /// Every matching containment witness ID.
    pub containment_witness_ids: BTreeSet<StableId>,
}

impl ProfileMatch {
    /// The winning profile category.
    #[must_use]
    pub const fn category(self) -> Category {
        self.category
    }
    /// The winning matcher identifier.
    #[must_use]
    pub const fn matcher_id(self) -> &'static str {
        self.matcher_id
    }
    /// The typed exclusion reason fixed by the category.
    #[must_use]
    pub const fn reason_id(self) -> &'static str {
        self.category.reason_id()
    }
    /// The endpoint selected after category and matcher precedence.
    #[must_use]
    pub const fn endpoint(self) -> CandidateEndpoint {
        self.endpoint
    }
}

/// All identity bindings required to derive a D profile exclusion record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DExclusionBindings {
    /// Snapshot that owns the candidate.
    pub snapshot_id: StableId,
    /// Exact D candidate key.
    pub candidate_key: String,
    /// Versioned rule identity.
    pub rule: String,
    /// Fixed profile identifier.
    pub profile_id: String,
    /// Fixed profile canonical-byte hash.
    pub profile_hash: ContentHash,
    /// Closed typed exclusion reason.
    pub reason_id: String,
    /// Fixed matcher identity.
    pub matcher_id: String,
    /// Exact retained decimal weight.
    pub excluded_weight: String,
    /// Sorted-unique source trace inputs.
    pub source_ids: BTreeSet<StableId>,
}

/// A source-traceable, D-specific profile exclusion record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DExclusionRecord {
    /// Stable ID derived from all D exclusion bindings.
    pub id: StableId,
    /// Snapshot that owns the candidate.
    pub snapshot_id: StableId,
    /// Exact D candidate key.
    pub candidate_key: String,
    /// Versioned D rule identity.
    pub rule: String,
    /// Fixed profile identifier.
    pub profile_id: String,
    /// Fixed profile hash.
    pub profile_hash: ContentHash,
    /// Closed typed exclusion reason.
    pub reason_id: String,
    /// Winning matcher identifier.
    pub matcher_id: String,
    /// Exact retained decimal weight.
    pub excluded_weight: String,
    /// Sorted-unique source trace.
    pub source_ids: BTreeSet<StableId>,
}

impl DExclusionRecord {
    /// Validates bindings and derives the stable D exclusion ID.
    pub fn new(bindings: DExclusionBindings) -> ProfileResult<Self> {
        validate_bindings(&bindings)?;
        let id = exclusion_id(&bindings)?;
        Ok(Self {
            id,
            snapshot_id: bindings.snapshot_id,
            candidate_key: bindings.candidate_key,
            rule: bindings.rule,
            profile_id: bindings.profile_id,
            profile_hash: bindings.profile_hash,
            reason_id: bindings.reason_id,
            matcher_id: bindings.matcher_id,
            excluded_weight: bindings.excluded_weight,
            source_ids: bindings.source_ids,
        })
    }

    /// Recomputes and validates every D identity binding in this record.
    pub fn validate(&self) -> ProfileResult<()> {
        let bindings = DExclusionBindings {
            snapshot_id: self.snapshot_id.clone(),
            candidate_key: self.candidate_key.clone(),
            rule: self.rule.clone(),
            profile_id: self.profile_id.clone(),
            profile_hash: self.profile_hash.clone(),
            reason_id: self.reason_id.clone(),
            matcher_id: self.matcher_id.clone(),
            excluded_weight: self.excluded_weight.clone(),
            source_ids: self.source_ids.clone(),
        };
        validate_bindings(&bindings)?;
        if self.id != exclusion_id(&bindings)? {
            return Err(ProfileError::InvalidExclusion(
                "stable ID does not bind record body".to_owned(),
            ));
        }
        Ok(())
    }
}

fn validate_bindings(bindings: &DExclusionBindings) -> ProfileResult<()> {
    if bindings.snapshot_id.kind() != "snapshot" {
        return Err(ProfileError::InvalidExclusion(
            "snapshot_id must use snapshot namespace".to_owned(),
        ));
    }
    let profile = rust_production_profile(&bindings.profile_id)
        .filter(|profile| bindings.profile_hash.as_str() == profile.expected_hash());
    let Some(profile) = profile else {
        return Err(ProfileError::InvalidExclusion(
            "fixed D bindings are not exact".to_owned(),
        ));
    };
    if bindings.candidate_key.is_empty()
        || bindings.rule != CHANGED_PUBLIC_CALLEE_RULE
        || !bindings
            .candidate_key
            .starts_with(&format!("{CHANGED_PUBLIC_CALLEE_RULE}|relation:"))
        || bindings.excluded_weight != D_EXCLUDED_WEIGHT
        || bindings.source_ids.is_empty()
    {
        return Err(ProfileError::InvalidExclusion(
            "fixed D bindings are not exact".to_owned(),
        ));
    }
    let matcher = profile
        .matchers()
        .iter()
        .find(|matcher| matcher.id == bindings.matcher_id);
    let Some(matcher) = matcher else {
        return Err(ProfileError::InvalidExclusion(
            "matcher_id is outside the fixed profile".to_owned(),
        ));
    };
    if bindings.reason_id != matcher.category.reason_id() {
        return Err(ProfileError::InvalidExclusion(
            "reason_id does not match matcher category".to_owned(),
        ));
    }
    Ok(())
}

fn exclusion_id(bindings: &DExclusionBindings) -> ProfileResult<StableId> {
    let source_ids = Value::Array(
        bindings
            .source_ids
            .iter()
            .map(|id| Value::String(id.to_string()))
            .collect(),
    );
    let map = BTreeMap::from([
        (
            "candidate_key".to_owned(),
            Value::String(bindings.candidate_key.clone()),
        ),
        (
            "excluded_weight".to_owned(),
            Value::String(bindings.excluded_weight.clone()),
        ),
        (
            "matcher_id".to_owned(),
            Value::String(bindings.matcher_id.clone()),
        ),
        (
            "profile_hash".to_owned(),
            Value::String(bindings.profile_hash.to_string()),
        ),
        (
            "profile_id".to_owned(),
            Value::String(bindings.profile_id.clone()),
        ),
        (
            "reason_id".to_owned(),
            Value::String(bindings.reason_id.clone()),
        ),
        ("rule".to_owned(), Value::String(bindings.rule.clone())),
        (
            "snapshot_id".to_owned(),
            Value::String(bindings.snapshot_id.to_string()),
        ),
        ("source_ids".to_owned(), source_ids),
    ]);
    StableId::derived("exclusion", &map)
        .map_err(|error| ProfileError::InvalidExclusion(error.to_string()))
}

/// The exact fixed number of Stage 0 commit clusters.
pub const STAGE0_CLUSTER_COUNT: usize = 300;

/// The frozen, exact commit-cluster domain used for one Stage 0 evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenStage0ClusterSet {
    cluster_ids: BTreeSet<StableId>,
}

impl FrozenStage0ClusterSet {
    /// Validates the exact 300-member `cluster:` namespace domain.
    pub fn new(cluster_ids: Vec<StableId>) -> ProfileResult<Self> {
        let normalized = cluster_ids.iter().cloned().collect::<BTreeSet<_>>();
        if cluster_ids.len() != STAGE0_CLUSTER_COUNT
            || normalized.len() != STAGE0_CLUSTER_COUNT
            || normalized.iter().any(|id| id.kind() != "cluster")
        {
            return Err(ProfileError::InvalidStage0(
                "frozen Stage 0 domain must be 300 unique cluster IDs".to_owned(),
            ));
        }
        Ok(Self {
            cluster_ids: normalized,
        })
    }

    /// The exact sorted commit-cluster set `C`.
    #[must_use]
    pub fn cluster_ids(&self) -> &BTreeSet<StableId> {
        &self.cluster_ids
    }
}

/// Closed planning reasons retained for every deferred D obligation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage0DeferralReason {
    /// The plan's bounded capacity was exhausted.
    BudgetExhausted,
    /// A prerequisite was itself deferred.
    PrerequisiteDeferred,
}

/// One applicable D obligation retained in a Stage 0 cluster's exact `A_c` set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Stage0ApplicableObligation {
    /// Exact decimal D-obligation weight.
    pub weight: String,
}

/// One deferred D obligation retained in a Stage 0 cluster's exact `D_c` set.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Stage0DeferredObligation {
    /// Exact decimal D-obligation weight, repeated from the applicable record.
    pub weight: String,
    /// Closed reason explaining why the candidate was deferred.
    pub reason: Stage0DeferralReason,
}

/// One source-bound Stage 0 cluster's exact `A_c` and `D_c` sets.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Stage0Cluster {
    /// Stable commit-cluster identity.
    pub id: StableId,
    /// Exact `A_c`, keyed by D obligation ID and retaining each weight.
    pub applicable: BTreeMap<StableId, Stage0ApplicableObligation>,
    /// Exact `D_c`, keyed by deferred D obligation ID and retaining its reason.
    pub deferred: BTreeMap<StableId, Stage0DeferredObligation>,
}

/// A sorted `(cluster ID, applicable D-obligation count)` fan-out entry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FanOutCount {
    /// Commit-cluster identity.
    pub cluster_id: StableId,
    /// Number of applicable D obligations in that cluster.
    pub applicable_count: usize,
}

/// Exact, model-free Stage 0 planning gates required by ADR 0038 §4.3.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Stage0Gates {
    /// Exact sorted frozen 300-cluster set `C`.
    pub cluster_ids: BTreeSet<StableId>,
    /// Each cluster's exact `A_c` / `D_c` records, keyed by its ID.
    pub clusters: BTreeMap<StableId, Stage0Cluster>,
    /// Exact union of all applicable D obligation IDs.
    pub applicable_obligation_ids: BTreeSet<StableId>,
    /// Exact union of all deferred D obligation IDs.
    pub deferred_obligation_ids: BTreeSet<StableId>,
    /// Counts sorted by `(count, cluster ID)`; zero-count clusters are retained.
    pub fan_out_counts: Vec<FanOutCount>,
    /// One-based nearest-rank p95 (285 for the fixed 300-cluster domain).
    pub p95_rank: usize,
    /// The p95 fan-out count.
    pub p95_count: usize,
    /// Whether `p95_count <= 50`.
    pub fan_out_passes: bool,
    /// Whether the exact integer comparison `20 * |D| <= |A|` passes.
    pub deferred_fraction_passes: bool,
}

impl Stage0Gates {
    /// Canonical artifact bytes containing `C`, every `A_c` / `D_c`, and the
    /// global `A` / `D` sets required for the Stage 0 advance decision.
    pub fn canonical_bytes(&self) -> ProfileResult<Vec<u8>> {
        canonical_json(self).map_err(|error| ProfileError::InvalidStage0(error.to_string()))
    }

    /// Hash of the complete canonical Stage 0 artifact.
    pub fn canonical_hash(&self) -> ProfileResult<ContentHash> {
        Ok(ContentHash::sha256(&self.canonical_bytes()?))
    }
}

/// Performs ADR §4.3's exact deferred-fraction comparison.
///
/// Overflow is a typed invalid Stage 0 input rather than a rounded pass/fail.
pub fn deferred_fraction_gate(
    applicable_count: usize,
    deferred_count: usize,
) -> ProfileResult<bool> {
    let scaled_deferred = deferred_count.checked_mul(20).ok_or_else(|| {
        ProfileError::InvalidStage0("deferred fraction integer multiplication overflow".to_owned())
    })?;
    Ok(applicable_count == 0 || scaled_deferred <= applicable_count)
}

/// Evaluates the fixed Stage 0 fan-out and deferred-fraction gates.
///
/// This is deliberately separate from profile exclusion: every supplied
/// applicable ID remains in the denominator, including deferred IDs.
pub fn evaluate_stage0_gates(
    frozen_clusters: FrozenStage0ClusterSet,
    clusters: Vec<Stage0Cluster>,
) -> ProfileResult<Stage0Gates> {
    let mut by_cluster = BTreeMap::new();
    let mut applicable_obligation_ids = BTreeSet::new();
    let mut deferred_obligation_ids = BTreeSet::new();
    for cluster in clusters {
        if cluster.id.kind() != "cluster" {
            return Err(ProfileError::InvalidStage0(
                "cluster ID must use cluster namespace".to_owned(),
            ));
        }
        if cluster
            .applicable
            .keys()
            .any(|id| id.kind() != "obligation")
        {
            return Err(ProfileError::InvalidStage0(
                "applicable IDs must use obligation namespace".to_owned(),
            ));
        }
        if cluster
            .applicable
            .values()
            .any(|obligation| obligation.weight != D_OBLIGATION_WEIGHT)
        {
            return Err(ProfileError::InvalidStage0(
                "applicable D obligation weight must be exact 4.0".to_owned(),
            ));
        }
        if !cluster
            .deferred
            .keys()
            .all(|id| cluster.applicable.contains_key(id))
        {
            return Err(ProfileError::InvalidStage0(
                "deferred IDs must be an applicable-ID subset".to_owned(),
            ));
        }
        if cluster.deferred.iter().any(|(id, deferred)| {
            deferred.weight != D_OBLIGATION_WEIGHT
                || cluster.applicable[id].weight != deferred.weight
        }) {
            return Err(ProfileError::InvalidStage0(
                "deferred D obligation must retain exact applicable weight 4.0".to_owned(),
            ));
        }
        let cluster_applicable_ids = cluster.applicable.keys().cloned().collect::<BTreeSet<_>>();
        if !applicable_obligation_ids.is_disjoint(&cluster_applicable_ids) {
            return Err(ProfileError::InvalidStage0(
                "applicable IDs must be globally unique".to_owned(),
            ));
        }
        applicable_obligation_ids.extend(cluster_applicable_ids);
        if by_cluster.insert(cluster.id.clone(), cluster).is_some() {
            return Err(ProfileError::InvalidStage0(
                "cluster IDs must be unique".to_owned(),
            ));
        }
    }
    if by_cluster.keys().cloned().collect::<BTreeSet<_>>() != *frozen_clusters.cluster_ids() {
        return Err(ProfileError::InvalidStage0(
            "cluster records must equal the exact frozen Stage 0 domain".to_owned(),
        ));
    }
    let mut fan_out_counts = Vec::with_capacity(STAGE0_CLUSTER_COUNT);
    for cluster in by_cluster.values() {
        applicable_obligation_ids.extend(cluster.applicable.keys().cloned());
        deferred_obligation_ids.extend(cluster.deferred.keys().cloned());
        fan_out_counts.push(FanOutCount {
            cluster_id: cluster.id.clone(),
            applicable_count: cluster.applicable.len(),
        });
    }
    fan_out_counts.sort_by(|left, right| {
        left.applicable_count
            .cmp(&right.applicable_count)
            .then_with(|| left.cluster_id.cmp(&right.cluster_id))
    });
    let p95_rank = 285;
    let p95_count = fan_out_counts[p95_rank - 1].applicable_count;
    let deferred_fraction_passes = deferred_fraction_gate(
        applicable_obligation_ids.len(),
        deferred_obligation_ids.len(),
    )?;
    Ok(Stage0Gates {
        cluster_ids: frozen_clusters.cluster_ids,
        clusters: by_cluster,
        applicable_obligation_ids,
        deferred_obligation_ids,
        fan_out_counts,
        p95_rank,
        p95_count,
        fan_out_passes: p95_count <= 50,
        deferred_fraction_passes,
    })
}

#[cfg(test)]
mod shared_path_contract_tests {
    use super::validate_path;

    #[test]
    fn profile_path_obeys_shared_snapshot_relative_contract() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/snapshot-relative-path-contract.v1.json"
        )))
        .expect("shared path contract fixture");
        assert_eq!(
            fixture["schema"],
            "reviewgraphen.snapshot_relative_path_contract_cases.v1"
        );
        for case in fixture["common_cases"].as_array().expect("common cases") {
            let path = case["path"].as_str().expect("case path");
            let valid = case["valid"].as_bool().expect("case validity");
            assert_eq!(
                validate_path(path, "shared-contract").is_ok(),
                valid,
                "shared path contract case {path:?}"
            );
        }
    }
}
