use crate::{DomainError, Result, canonical_hash};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// A validated identifier with a stable `kind:payload` grammar.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(transparent)]
pub struct StableId(String);

impl StableId {
    /// Parses an externally supplied stable ID.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let Some((kind, payload)) = value.split_once(':') else {
            return Err(DomainError::InvalidId {
                value,
                reason: "missing ':' separator".to_owned(),
            });
        };
        let kind_is_valid = !kind.is_empty()
            && kind.bytes().enumerate().all(|(index, byte)| match byte {
                b'a'..=b'z' => true,
                b'0'..=b'9' | b'.' | b'_' | b'-' => index > 0,
                _ => false,
            });
        let payload_is_valid = payload
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && payload.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || matches!(byte, b'.' | b'_' | b':' | b'@' | b'/' | b'+' | b'-')
            });
        if !kind_is_valid || !payload_is_valid {
            return Err(DomainError::InvalidId {
                value,
                reason: "expected lowercase kind and ASCII payload".to_owned(),
            });
        }
        Ok(Self(value))
    }

    /// Builds a deterministic ID from canonical, snapshot-bound components.
    pub fn derived(kind: &str, bindings: &BTreeMap<String, Value>) -> Result<Self> {
        let hash = canonical_hash(bindings)?;
        Self::parse(format!("{kind}:{hash}"))
    }

    /// Returns the ID kind before the first colon.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.0.split_once(':').map_or("", |(kind, _)| kind)
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// A supported content hash with its algorithm prefix retained.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(transparent)]
pub struct ContentHash(String);

impl ContentHash {
    /// Parses `sha256:`, `blake3:`, or `git:` hexadecimal hashes.
    pub fn parse(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let Some((algorithm, hex)) = value.split_once(':') else {
            return Err(DomainError::InvalidHash { value });
        };
        if !matches!(algorithm, "sha256" | "blake3" | "git")
            || hex.len() < 8
            || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(DomainError::InvalidHash { value });
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    /// Returns a SHA-256 content hash for canonical bytes.
    pub fn sha256(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(bytes);
        Self(format!("sha256:{digest:x}"))
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<'de> Deserialize<'de> for ContentHash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::parse(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// The tuple binding every generated obligation to its deterministic inputs.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
pub struct VersionTuple {
    /// Versioned profile identity, for example `code-review@1`.
    profile: String,
    /// Versioned rule identity.
    rule: String,
    /// Hash of the extractor/adaptor set.
    extractor_set: ContentHash,
    /// Fixed snapshot identity.
    snapshot: StableId,
}

impl VersionTuple {
    /// Versioned profile identity.
    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    /// Versioned rule identity.
    #[must_use]
    pub fn rule(&self) -> &str {
        &self.rule
    }

    /// Deterministic extractor-set hash.
    #[must_use]
    pub fn extractor_set(&self) -> &ContentHash {
        &self.extractor_set
    }

    /// Snapshot binding.
    #[must_use]
    pub fn snapshot(&self) -> &StableId {
        &self.snapshot
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVersionTuple {
    profile: String,
    rule: String,
    extractor_set: ContentHash,
    snapshot: StableId,
}

impl<'de> Deserialize<'de> for VersionTuple {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawVersionTuple::deserialize(deserializer)?;
        Self::new(raw.profile, raw.rule, raw.extractor_set, raw.snapshot)
            .map_err(serde::de::Error::custom)
    }
}

impl VersionTuple {
    /// Validates and constructs a complete tuple.
    pub fn new(
        profile: impl Into<String>,
        rule: impl Into<String>,
        extractor_set: ContentHash,
        snapshot: StableId,
    ) -> Result<Self> {
        let profile = profile.into();
        let rule = rule.into();
        if profile.is_empty() {
            return Err(DomainError::EmptyField { field: "profile" });
        }
        if rule.is_empty() {
            return Err(DomainError::EmptyField { field: "rule" });
        }
        if snapshot.kind() != "snapshot" {
            return Err(DomainError::Validation(
                "version tuple snapshot must use the snapshot ID namespace".to_owned(),
            ));
        }
        Ok(Self {
            profile,
            rule,
            extractor_set,
            snapshot,
        })
    }
}

/// Detects duplicate IDs and distinguishes identical replay from collisions.
#[derive(Debug, Default)]
pub struct IdRegistry {
    hashes: BTreeMap<StableId, ContentHash>,
}

impl IdRegistry {
    /// Reserves an ID for canonical content. Repeating identical content is safe.
    pub fn reserve(&mut self, id: StableId, content: &impl Serialize) -> Result<()> {
        let hash = ContentHash::sha256(&crate::canonical_json(content)?);
        match self.hashes.get(&id) {
            Some(existing) if existing != &hash => Err(DomainError::IdCollision { id }),
            Some(_) => Ok(()),
            None => {
                self.hashes.insert(id, hash);
                Ok(())
            }
        }
    }
}
