use crate::{ContentHash, DomainError, Result};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

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

struct CountingWriter {
    count: usize,
    limit: usize,
    overflow: bool,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let Some(next) = self.count.checked_add(bytes.len()) else {
            self.overflow = true;
            return Err(io::Error::other("canonical JSON byte count overflow"));
        };
        if next > self.limit {
            self.overflow = true;
            return Err(io::Error::other("canonical JSON byte count exceeds limit"));
        }
        self.count = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Counts the exact compact JSON wire length, including UTF-8 escaping,
/// without retaining an output buffer. Object ordering cannot change length;
/// canonical key ordering is still enforced by [`canonical_json`] after the
/// caller admits this count against its working-set limit.
pub(crate) fn canonical_json_count_bounded<T: Serialize>(
    value: &T,
    limit: usize,
    operation: &'static str,
) -> Result<u64> {
    let mut writer = CountingWriter {
        count: 0,
        limit,
        overflow: false,
    };
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        if writer.overflow {
            return Err(DomainError::Incomplete {
                operation,
                limit,
                observed: writer.count.saturating_add(1),
            });
        }
        return Err(DomainError::CanonicalJson(error.to_string()));
    }
    u64::try_from(writer.count).map_err(|_| DomainError::Incomplete {
        operation,
        limit,
        observed: usize::MAX,
    })
}

struct Sha256Writer(Sha256);

impl Write for Sha256Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Hashes a compact JSON stream without retaining canonical bytes. Callers
/// must provide a wrapper whose struct/map fields are already emitted in
/// canonical lexical order.
pub(crate) fn compact_json_sha256_streaming<T: Serialize>(value: &T) -> Result<ContentHash> {
    let mut writer = Sha256Writer(Sha256::new());
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    ContentHash::parse(format!("sha256:{:x}", writer.0.finalize()))
}

struct MatchingWriter<'a> {
    expected: &'a [u8],
    offset: usize,
    differs: bool,
}

impl Write for MatchingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.offset.saturating_add(bytes.len());
        if end > self.expected.len() || self.expected.get(self.offset..end) != Some(bytes) {
            self.differs = true;
        }
        self.offset = end;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Compares a compact, canonically field-ordered serializer with existing
/// bytes without materializing either a JSON value tree or an output buffer.
pub(crate) fn compact_json_eq_streaming<T: Serialize>(value: &T, expected: &[u8]) -> Result<bool> {
    let mut writer = MatchingWriter {
        expected,
        offset: 0,
        differs: false,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    Ok(!writer.differs && writer.offset == expected.len())
}

/// Hashes a compact JSON array from a fallible iterator without retaining the
/// array or its members. Each member serializer must already emit object keys
/// in canonical lexical order.
pub(crate) fn compact_json_array_sha256_streaming<T, I>(values: I) -> Result<ContentHash>
where
    T: Serialize,
    I: IntoIterator<Item = Result<T>>,
{
    let mut writer = Sha256Writer(Sha256::new());
    writer
        .write_all(b"[")
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    let mut first = true;
    for value in values {
        let value = value?;
        if !first {
            writer
                .write_all(b",")
                .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
        }
        first = false;
        serde_json::to_writer(&mut writer, &value)
            .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    }
    writer
        .write_all(b"]")
        .map_err(|error| DomainError::CanonicalJson(error.to_string()))?;
    ContentHash::parse(format!("sha256:{:x}", writer.0.finalize()))
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
    use super::{canonical_json, canonical_json_count_bounded};
    use serde_json::json;

    #[test]
    fn recursively_orders_object_keys_without_reordering_arrays() {
        let value = json!({"z": {"b": 1, "a": 2}, "a": [2, 1]});
        assert_eq!(
            canonical_json(&value).unwrap(),
            br#"{"a":[2,1],"z":{"a":2,"b":1}}"#
        );
    }

    #[test]
    fn counting_writer_matches_exact_escaped_wire_length_and_cap() {
        let value = json!({"escaped": "line\nquote\"slash\\tab\t雪"});
        let bytes = canonical_json(&value).unwrap();
        assert_eq!(
            canonical_json_count_bounded(&value, bytes.len(), "counting test").unwrap(),
            bytes.len() as u64
        );
        assert!(canonical_json_count_bounded(&value, bytes.len() - 1, "counting test").is_err());
    }
}
