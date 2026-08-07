use crate::{ContentHash, DomainError, Result};
use serde::Serialize;
use serde_json::{Map, Value};

/// A byte-stable JSON payload together with its SHA-256 hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalJson {
    bytes: Vec<u8>,
    hash: ContentHash,
}

/// Serializes a value after recursively sorting every object key.
pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = serde_json::to_value(value)
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    serde_json::to_vec(&canonical_json_value(value))
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))
}

/// Canonicalizes a JSON value. Set-like collections must be sorted by their
/// owning domain constructors; arrays deliberately preserve sequence meaning.
#[must_use]
pub fn canonical_json_value(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonical_json_value).collect()),
        Value::Object(object) => {
            let mut entries = object.into_iter().collect::<Vec<_>>();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            let mut canonical = Map::new();
            for (key, value) in entries {
                canonical.insert(key, canonical_json_value(value));
            }
            Value::Object(canonical)
        }
        scalar => scalar,
    }
}

/// Returns the full SHA-256 hash of canonical JSON.
pub fn canonical_hash<T: Serialize>(value: &T) -> Result<String> {
    Ok(ContentHash::sha256(&canonical_json(value)?).to_string())
}

impl CanonicalJson {
    /// Builds canonical bytes and their hash.
    pub fn from_serializable(value: &impl Serialize) -> Result<Self> {
        let bytes = canonical_json(value)?;
        let hash = ContentHash::sha256(&bytes);
        Ok(Self { bytes, hash })
    }

    /// Canonical UTF-8 JSON bytes. The bytes cannot be mutated independently
    /// of their hash.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// SHA-256 hash of [`Self::bytes`].
    #[must_use]
    pub fn hash(&self) -> &ContentHash {
        &self.hash
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_json;
    use serde_json::json;

    #[test]
    fn recursively_orders_object_keys_without_reordering_arrays() {
        let value = json!({"z": {"b": 1, "a": 2}, "a": [2, 1]});
        assert_eq!(
            canonical_json(&value).unwrap(),
            br#"{"a":[2,1],"z":{"a":2,"b":1}}"#
        );
    }
}
