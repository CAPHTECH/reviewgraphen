//! Pure M4 evidence, verification, decision, finding, and assessment contracts.
//!
//! This module deliberately contains no event, admission, persistence, tool,
//! verifier-execution, index, or report behavior.
//!
//! Public enums are output/domain types, never public serde input surfaces:
//! ```compile_fail
//! let _: reviewgraphen_core::VerifierDescriptorV3 =
//!     serde_json::from_str("\"reviewgraphen.static_fact_verifier@1\"").unwrap();
//! ```

use crate::{
    ClaimPolarity, ContentHash, DomainError, ExecutionClaimV2, Obligation, ProgramSpace, StableId,
};
use serde::de::DeserializeOwned;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::io::{self, Write};
use std::marker::PhantomData;
use thiserror::Error;

pub type M4Result<T> = std::result::Result<T, M4Error>;
type Result<T> = M4Result<T>;

/// Closed pure-domain failures for M4 record construction and reduction.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum M4Error {
    #[error("snapshot mismatch: expected {expected}, got {actual}")]
    SnapshotMismatch {
        expected: StableId,
        actual: StableId,
    },
    #[error("run mismatch: expected {expected}, got {actual}")]
    RunMismatch {
        expected: StableId,
        actual: StableId,
    },
    #[error("universe mismatch: expected {expected}, got {actual}")]
    UniverseMismatch {
        expected: StableId,
        actual: StableId,
    },
    #[error("claim mismatch: expected {expected}, got {actual}")]
    ClaimMismatch {
        expected: StableId,
        actual: StableId,
    },
    #[error("claim body hash mismatch")]
    ClaimBodyMismatch,
    #[error("property mismatch: expected {expected}, got {actual}")]
    PropertyMismatch { expected: String, actual: String },
    #[error("unsupported M4 property `{property_id}`")]
    UnsupportedProperty { property_id: String },
    #[error("dangling M4 reference from {owner} to {reference}")]
    DanglingReference {
        owner: &'static str,
        reference: StableId,
    },
    #[error("duplicate value in {field}")]
    Duplicate { field: &'static str },
    #[error("non-canonical order in {field}")]
    NonCanonicalOrder { field: &'static str },
    #[error("M4 ID collision for {id}")]
    IdCollision { id: StableId },
    #[error("decision source set does not equal the current claim closure")]
    DecisionSourceMismatch,
    #[error("finding is not projectable for this claim state")]
    FindingNotProjectable,
    #[error("finding would repeat the current projection without new authority")]
    RedundantFinding,
    #[error("M4 authority scope does not exactly match the assessment scope")]
    AuthorityScopeMismatch,
    #[error("illegal M4 {axis} transition: {from} -> {to}")]
    IllegalTransition {
        axis: &'static str,
        from: String,
        to: String,
    },
    #[error("{operation} exceeds limit {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: usize,
        observed: usize,
    },
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("invalid ID kind for {field}: expected {expected}, got {actual}")]
    InvalidIdKind {
        field: String,
        expected: String,
        actual: String,
    },
    #[error("invalid stable ID `{value}`: {reason}")]
    InvalidId { value: String, reason: String },
    #[error("invalid content hash `{value}`")]
    InvalidHash { value: String },
    #[error("canonical M4 serialization failed: {reason}")]
    Canonical { reason: String },
    #[error("invalid M4 record: {reason}")]
    InvalidRecord { reason: String },
}

impl From<DomainError> for M4Error {
    fn from(error: DomainError) -> Self {
        match error {
            DomainError::Incomplete {
                operation,
                limit,
                observed,
            } => Self::Incomplete {
                operation,
                limit,
                observed,
            },
            DomainError::EmptyField { field } => Self::EmptyField { field },
            DomainError::IdCollision { id } => Self::IdCollision { id },
            DomainError::DanglingReference {
                owner, reference, ..
            } => Self::DanglingReference { owner, reference },
            DomainError::IllegalTransition { axis, from, to, .. } => {
                Self::IllegalTransition { axis, from, to }
            }
            DomainError::CanonicalJson(reason) => Self::Canonical { reason },
            DomainError::InvalidId { value, reason } => Self::InvalidId { value, reason },
            DomainError::InvalidHash { value } => Self::InvalidHash { value },
            other => Self::InvalidRecord {
                reason: other.to_string(),
            },
        }
    }
}

pub const M4_PROPERTY_ID: &str = "payment.at_most_once";
pub const STATIC_DESCRIPTOR_ID: &str = "reviewgraphen.static_fact_verifier@1";
pub const STATIC_PROCEDURE_ID: &str = "reviewgraphen.static_fact.projection@1";
pub const FIXTURE_DESCRIPTOR_ID: &str = "reviewgraphen.fixture_test_verifier@1";
pub const FIXTURE_PROCEDURE_ID: &str = "reviewgraphen.fixture_test.duplicate_submit@1";
pub const FINDING_PROJECTION_ID: &str = "reviewgraphen.finding_projection@1";
pub const FIXTURE_HARNESS_ID: &str = "reviewgraphen.double_submit_harness@1";
pub const FIXTURE_HARNESS_REVISION: &str = "1";
pub const FIXTURE_HARNESS_SOURCE_HASH: &str =
    "sha256:74d708edd94103e3bab724c71df8c151a89ea613ad06fd8c6bd21ba78027848a";
pub const FIXTURE_TEST_ARTIFACT_ID: &str = "test:double-submit";
pub const FIXTURE_MEDIA_TYPE: &str = "application/vnd.reviewgraphen.test-witness+json;version=1";
pub const FIXTURE_WITNESS_HASH: &str =
    "sha256:8d673f965d089dfc08fb3e9c85453f4654894de71e0bd331a9f3cf97bcea5355";

pub(crate) const MAX_RECORD_BYTES: usize = 65_536;
const MAX_SET: usize = 64;
pub(crate) const MAX_EVIDENCE_SUBJECTS: usize = 128;
const MAX_VERIFICATION_EVIDENCE: usize = 128;
const MAX_DECISION_SOURCES: usize = 256;
const MAX_LIMITATIONS: usize = 32;
pub(crate) const MAX_LIMITATION_BYTES: usize = 2_048;
pub(crate) const MAX_TRACE_BYTES: usize = 256;
const MAX_RATIONALE_BYTES: usize = 8_192;
pub(crate) const MAX_RETAINED_WORKING_BYTES: u64 = 16_777_216;
const MAX_M4_OBJECT_MEMBERS: usize = 64;

#[cfg(test)]
std::thread_local! {
    static M4_SERDE_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static M4_TEST_WORKING_LIMIT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static M4_TEST_RETAINED_OVERRIDE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static M4_LAST_WORKING_PEAK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static M4_WORKING_PREFLIGHTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
struct KeySpan {
    start: usize,
    end: usize,
}

fn skip_json_ws(input: &[u8], index: &mut usize) {
    while input.get(*index).is_some_and(u8::is_ascii_whitespace) {
        *index += 1;
    }
}

fn hex_value(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some(u16::from(byte - b'0')),
        b'a'..=b'f' => Some(u16::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u16::from(byte - b'A' + 10)),
        _ => None,
    }
}

fn unicode_escape(input: &[u8], index: &mut usize) -> Result<u16> {
    let end = index
        .checked_add(4)
        .ok_or_else(|| validation("invalid JSON escape"))?;
    let digits = input
        .get(*index..end)
        .ok_or_else(|| validation("truncated JSON escape"))?;
    let mut value = 0_u16;
    for digit in digits {
        value = value
            .checked_mul(16)
            .and_then(|base| hex_value(*digit).and_then(|part| base.checked_add(part)))
            .ok_or_else(|| validation("invalid JSON unicode escape"))?;
    }
    *index = end;
    Ok(value)
}

/// Scans a JSON string without constructing an owned value and returns the
/// closing-quote offset plus its decoded UTF-8 byte count.
fn scan_m4_string(input: &[u8], start: usize, allow_escape: bool) -> Result<(usize, usize)> {
    if input.get(start) != Some(&b'"') {
        return Err(validation("expected JSON string"));
    }
    let mut index = start + 1;
    let mut decoded = 0_usize;
    while let Some(byte) = input.get(index).copied() {
        match byte {
            b'"' => return Ok((index + 1, decoded)),
            b'\\' => {
                if !allow_escape {
                    return Err(validation("escaped M4 object keys are forbidden"));
                }
                index += 1;
                match input.get(index).copied() {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                        decoded = decoded
                            .checked_add(1)
                            .ok_or_else(|| validation("decoded M4 JSON string length overflow"))?;
                        index += 1;
                    }
                    Some(b'u') => {
                        index += 1;
                        let first = unicode_escape(input, &mut index)?;
                        let scalar = if (0xD800..=0xDBFF).contains(&first) {
                            if input.get(index..index + 2) != Some(&b"\\u"[..]) {
                                return Err(validation("unpaired JSON high surrogate"));
                            }
                            index += 2;
                            let second = unicode_escape(input, &mut index)?;
                            if !(0xDC00..=0xDFFF).contains(&second) {
                                return Err(validation("unpaired JSON high surrogate"));
                            }
                            0x1_0000
                                + ((u32::from(first) - 0xD800) << 10)
                                + (u32::from(second) - 0xDC00)
                        } else if (0xDC00..=0xDFFF).contains(&first) {
                            return Err(validation("unpaired JSON low surrogate"));
                        } else {
                            u32::from(first)
                        };
                        decoded = decoded
                            .checked_add(char::from_u32(scalar).map_or(4, char::len_utf8))
                            .ok_or_else(|| validation("decoded M4 JSON string length overflow"))?;
                    }
                    _ => return Err(validation("invalid JSON escape")),
                }
            }
            0x00..=0x1f => return Err(validation("control byte in JSON string")),
            _ => {
                decoded = decoded
                    .checked_add(1)
                    .ok_or_else(|| validation("decoded M4 JSON string length overflow"))?;
                index += 1;
            }
        }
    }
    Err(validation("unterminated JSON string"))
}

fn decode_m4_string_into(input: &[u8], start: usize, output: &mut [u8]) -> Result<(usize, usize)> {
    let mut index = start + 1;
    let mut written = 0_usize;
    while let Some(byte) = input.get(index).copied() {
        match byte {
            b'"' => return Ok((index + 1, written)),
            b'\\' => {
                index += 1;
                match input.get(index).copied() {
                    Some(escaped @ (b'"' | b'\\' | b'/')) => {
                        *output
                            .get_mut(written)
                            .ok_or_else(|| validation("decoded M4 set scalar exceeds buffer"))? =
                            escaped;
                        written += 1;
                        index += 1;
                    }
                    Some(escape @ (b'b' | b'f' | b'n' | b'r' | b't')) => {
                        let decoded = match escape {
                            b'b' => 0x08,
                            b'f' => 0x0c,
                            b'n' => b'\n',
                            b'r' => b'\r',
                            b't' => b'\t',
                            _ => unreachable!(),
                        };
                        *output
                            .get_mut(written)
                            .ok_or_else(|| validation("decoded M4 set scalar exceeds buffer"))? =
                            decoded;
                        written += 1;
                        index += 1;
                    }
                    Some(b'u') => {
                        index += 1;
                        let first = unicode_escape(input, &mut index)?;
                        let scalar = if (0xD800..=0xDBFF).contains(&first) {
                            index += 2;
                            let second = unicode_escape(input, &mut index)?;
                            0x1_0000
                                + ((u32::from(first) - 0xD800) << 10)
                                + (u32::from(second) - 0xDC00)
                        } else {
                            u32::from(first)
                        };
                        let value = char::from_u32(scalar)
                            .ok_or_else(|| validation("invalid JSON unicode scalar"))?;
                        let mut encoded = [0_u8; 4];
                        let bytes = value.encode_utf8(&mut encoded).as_bytes();
                        let end = written + bytes.len();
                        output
                            .get_mut(written..end)
                            .ok_or_else(|| validation("decoded M4 set scalar exceeds buffer"))?
                            .copy_from_slice(bytes);
                        written = end;
                    }
                    _ => return Err(validation("invalid JSON escape")),
                }
            }
            _ => {
                *output
                    .get_mut(written)
                    .ok_or_else(|| validation("decoded M4 set scalar exceeds buffer"))? = byte;
                written += 1;
                index += 1;
            }
        }
    }
    Err(validation("unterminated JSON string"))
}

fn m4_string_limit(key: &[u8]) -> usize {
    match key {
        b"rationale" => MAX_RATIONALE_BYTES,
        b"limitations" => MAX_LIMITATION_BYTES,
        _ => MAX_TRACE_BYTES,
    }
}

fn m4_array_limit(operation: &str, key: &[u8]) -> usize {
    match key {
        b"limitations" => MAX_LIMITATIONS,
        b"evidence_ids" if operation == "finding JSON" => MAX_SET,
        b"evidence_ids" => MAX_VERIFICATION_EVIDENCE,
        b"verification_ids" => MAX_SET,
        b"source_ids" if operation == "decision JSON" => MAX_DECISION_SOURCES,
        b"source_ids"
        | b"target_refs"
        | b"subject_ids"
        | b"candidate_invariant_ids"
        | b"claim_source_ids"
        | b"obligation_context_ids"
        | b"obligation_source_ids"
        | b"obligation_target_refs"
        | b"selected_invariant_scope_ids" => MAX_EVIDENCE_SUBJECTS,
        _ => MAX_DECISION_SOURCES,
    }
}

fn is_m4_canonical_set_array(key: &[u8]) -> bool {
    matches!(
        key,
        b"limitations"
            | b"evidence_ids"
            | b"verification_ids"
            | b"source_ids"
            | b"target_refs"
            | b"subject_ids"
            | b"candidate_invariant_ids"
            | b"claim_source_ids"
            | b"obligation_context_ids"
            | b"obligation_source_ids"
            | b"obligation_target_refs"
            | b"selected_invariant_scope_ids"
    )
}

fn preflight_m4_json(input: &[u8], operation: &'static str) -> Result<()> {
    if input.len() > MAX_RECORD_BYTES {
        return Err(M4Error::Incomplete {
            operation,
            limit: MAX_RECORD_BYTES,
            observed: input.len(),
        });
    }
    let mut index = 0;
    skip_json_ws(input, &mut index);
    if input.get(index) != Some(&b'{') {
        return Err(validation("M4 JSON root must be an object"));
    }
    index += 1;
    let mut keys = [KeySpan { start: 0, end: 0 }; MAX_M4_OBJECT_MEMBERS];
    let mut key_count = 0_usize;
    loop {
        skip_json_ws(input, &mut index);
        if input.get(index) == Some(&b'}') {
            index += 1;
            break;
        }
        if key_count == MAX_M4_OBJECT_MEMBERS {
            return Err(M4Error::Incomplete {
                operation: "M4 JSON object members",
                limit: MAX_M4_OBJECT_MEMBERS,
                observed: key_count + 1,
            });
        }
        let key_quote = index;
        let (after_key, key_len) = scan_m4_string(input, index, false)?;
        if key_len > MAX_TRACE_BYTES {
            return Err(M4Error::Incomplete {
                operation: "M4 JSON object key",
                limit: MAX_TRACE_BYTES,
                observed: key_len,
            });
        }
        let span = KeySpan {
            start: key_quote + 1,
            end: after_key - 1,
        };
        if keys[..key_count]
            .iter()
            .any(|old| input[old.start..old.end] == input[span.start..span.end])
        {
            return Err(M4Error::Duplicate {
                field: "M4 JSON object key",
            });
        }
        keys[key_count] = span;
        key_count += 1;
        index = after_key;
        skip_json_ws(input, &mut index);
        if input.get(index) != Some(&b':') {
            return Err(validation("missing JSON object colon"));
        }
        index += 1;
        skip_json_ws(input, &mut index);
        let key = &input[span.start..span.end];
        match input.get(index).copied() {
            Some(b'"') => {
                let (end, decoded) = scan_m4_string(input, index, true)?;
                let limit = m4_string_limit(key);
                if decoded > limit {
                    return Err(M4Error::Incomplete {
                        operation: "M4 JSON scalar string",
                        limit,
                        observed: decoded,
                    });
                }
                index = end;
            }
            Some(b'[') => {
                index += 1;
                let mut count = 0_usize;
                let array_limit = m4_array_limit(operation, key);
                let canonical_set = is_m4_canonical_set_array(key);
                let mut previous = [0_u8; MAX_LIMITATION_BYTES];
                let mut previous_len = 0_usize;
                loop {
                    skip_json_ws(input, &mut index);
                    if input.get(index) == Some(&b']') {
                        index += 1;
                        break;
                    }
                    if count == array_limit {
                        return Err(M4Error::Incomplete {
                            operation: "M4 JSON array",
                            limit: array_limit,
                            observed: count + 1,
                        });
                    }
                    let scalar_start = index;
                    let (end, decoded) = scan_m4_string(input, index, true)?;
                    let limit = m4_string_limit(key);
                    if decoded > limit {
                        return Err(M4Error::Incomplete {
                            operation: "M4 JSON array string",
                            limit,
                            observed: decoded,
                        });
                    }
                    if canonical_set {
                        let mut current = [0_u8; MAX_LIMITATION_BYTES];
                        let (_, current_len) =
                            decode_m4_string_into(input, scalar_start, &mut current)?;
                        if count > 0 && current[..current_len] <= previous[..previous_len] {
                            return Err(if current[..current_len] == previous[..previous_len] {
                                M4Error::Duplicate {
                                    field: "M4 JSON set array",
                                }
                            } else {
                                M4Error::NonCanonicalOrder {
                                    field: "M4 JSON set array",
                                }
                            });
                        }
                        previous[..current_len].copy_from_slice(&current[..current_len]);
                        previous_len = current_len;
                    }
                    count += 1;
                    index = end;
                    skip_json_ws(input, &mut index);
                    match input.get(index) {
                        Some(b',') => index += 1,
                        Some(b']') => {}
                        _ => return Err(validation("invalid M4 JSON array")),
                    }
                }
            }
            Some(b'{') => return Err(validation("nested M4 JSON value is forbidden")),
            Some(_) => {
                let start = index;
                while input
                    .get(index)
                    .is_some_and(|byte| !matches!(byte, b',' | b'}') && !byte.is_ascii_whitespace())
                {
                    index += 1;
                }
                if start == index {
                    return Err(validation("empty M4 JSON value"));
                }
            }
            None => return Err(validation("truncated M4 JSON value")),
        }
        skip_json_ws(input, &mut index);
        match input.get(index) {
            Some(b',') => index += 1,
            Some(b'}') => {}
            _ => return Err(validation("invalid M4 JSON object")),
        }
    }
    skip_json_ws(input, &mut index);
    if index != input.len() {
        return Err(validation("trailing M4 JSON data"));
    }
    Ok(())
}

fn decode_m4_wire<T: DeserializeOwned>(input: &[u8], operation: &'static str) -> Result<T> {
    preflight_m4_json(input, operation)?;
    #[cfg(test)]
    M4_SERDE_ENTRIES.with(|count| count.set(count.get() + 1));
    serde_json::from_slice(input).map_err(|error| validation(error.to_string()))
}

#[cfg(test)]
fn checked_working_peak(retained: u64, additions: impl IntoIterator<Item = u64>) -> Result<u64> {
    checked_working_peak_with_limit(retained, additions, MAX_RETAINED_WORKING_BYTES)
}

fn checked_working_peak_with_limit(
    retained: u64,
    additions: impl IntoIterator<Item = u64>,
    limit: u64,
) -> Result<u64> {
    let mut peak = retained;
    for addition in additions {
        peak = peak.checked_add(addition).ok_or(M4Error::Incomplete {
            operation: "M4 verifier working bytes",
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
            observed: usize::MAX,
        })?;
    }
    if peak > limit {
        return Err(M4Error::Incomplete {
            operation: "M4 verifier working bytes",
            limit: usize::try_from(limit).unwrap_or(usize::MAX),
            observed: usize::try_from(peak).unwrap_or(usize::MAX),
        });
    }
    Ok(peak)
}

fn checked_memory_add(total: &mut u64, bytes: usize) -> Result<()> {
    *total = total
        .checked_add(u64::try_from(bytes).map_err(|_| M4Error::Incomplete {
            operation: "M4 retained verifier bytes",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?)
        .ok_or(M4Error::Incomplete {
            operation: "M4 retained verifier bytes",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    Ok(())
}

fn id_vec_allocated_bytes(values: &[StableId]) -> Result<u64> {
    let mut total = 0;
    checked_memory_add(&mut total, size_of_val(values))?;
    for value in values {
        checked_memory_add(&mut total, value.allocated_bytes())?;
    }
    Ok(total)
}

fn string_vec_allocated_bytes(values: &[String]) -> Result<u64> {
    let mut total = 0;
    checked_memory_add(&mut total, size_of_val(values))?;
    for value in values {
        checked_memory_add(&mut total, value.capacity())?;
    }
    Ok(total)
}

fn id_set_allocated_bytes(values: &BTreeSet<StableId>) -> Result<u64> {
    let mut total = 0;
    for value in values {
        checked_memory_add(&mut total, size_of::<StableId>())?;
        checked_memory_add(&mut total, value.allocated_bytes())?;
    }
    Ok(total)
}

fn cloned_ids_allocated_bytes<'a>(values: impl IntoIterator<Item = &'a StableId>) -> Result<u64> {
    let mut total = 0_u64;
    for value in values {
        checked_memory_add(&mut total, size_of::<StableId>())?;
        checked_memory_add(&mut total, value.allocated_bytes())?;
    }
    Ok(total)
}

fn string_set_allocated_bytes(values: &BTreeSet<String>) -> Result<u64> {
    let mut total = 0;
    for value in values {
        checked_memory_add(&mut total, size_of::<String>())?;
        checked_memory_add(&mut total, value.capacity())?;
    }
    Ok(total)
}

fn record_addition_bytes(
    id: &StableId,
    inline_value_bytes: usize,
    value_allocated_bytes: u64,
    canonical_scratch_request: usize,
) -> Result<u64> {
    let mut total = value_allocated_bytes;
    checked_memory_add(
        &mut total,
        size_of::<StableId>()
            + inline_value_bytes
            + id.allocated_bytes()
            + canonical_scratch_request,
    )?;
    Ok(total)
}

fn sorted_record_position<T>(
    values: &[T],
    id: &StableId,
    record_id: impl Fn(&T) -> &StableId,
) -> std::result::Result<usize, usize> {
    values.binary_search_by(|value| record_id(value).cmp(id))
}

fn find_sorted_record<'a, T>(
    values: &'a [T],
    id: &StableId,
    record_id: impl Fn(&T) -> &StableId,
) -> Option<&'a T> {
    sorted_record_position(values, id, &record_id)
        .ok()
        .map(|index| &values[index])
}

fn reserve_one<T>(values: &mut Vec<T>, operation: &'static str) -> Result<()> {
    values
        .try_reserve_exact(1)
        .map_err(|_| M4Error::Incomplete {
            operation,
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })
}

fn insert_sorted_reserved<T>(values: &mut Vec<T>, value: T, record_id: impl Fn(&T) -> &StableId) {
    let index = sorted_record_position(values, record_id(&value), &record_id)
        .expect_err("collision checked before reserved insertion");
    values.insert(index, value);
}

fn insert_id_sorted_reserved(values: &mut Vec<StableId>, value: StableId) {
    let index = values
        .binary_search(&value)
        .expect_err("ID collision checked before reserved insertion");
    values.insert(index, value);
}

fn clone_ids_requested<'a>(
    ids: impl Iterator<Item = &'a StableId>,
    requested: usize,
    operation: &'static str,
) -> Result<Vec<StableId>> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(requested)
        .map_err(|_| M4Error::Incomplete {
            operation,
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    values.extend(ids.cloned());
    values.sort_unstable();
    Ok(values)
}

fn checked_usize_add(total: &mut usize, addition: usize) -> Result<()> {
    *total = total.checked_add(addition).ok_or(M4Error::Incomplete {
        operation: "M4 canonical size arithmetic",
        limit: MAX_RECORD_BYTES,
        observed: usize::MAX,
    })?;
    Ok(())
}

fn json_id_array_len(count: usize, id_bytes: usize) -> Result<usize> {
    let mut total = 2_usize;
    checked_usize_add(&mut total, id_bytes)?;
    checked_usize_add(&mut total, count.saturating_mul(2))?;
    checked_usize_add(&mut total, count.saturating_sub(1))?;
    Ok(total)
}

struct FindingCanonicalShape<'a> {
    claim_id: &'a StableId,
    decision_id: Option<&'a StableId>,
    evidence_count: usize,
    evidence_id_bytes: usize,
    status: FindingStatusV3,
    supersedes_id: Option<&'a StableId>,
    verification_count: usize,
    verification_id_bytes: usize,
}

fn add_json_string_member(total: &mut usize, key: &str, value_len: usize) -> Result<()> {
    checked_usize_add(total, key.len() + value_len + 5)
}

fn finding_canonical_request(shape: FindingCanonicalShape<'_>) -> Result<usize> {
    let mut total = 2_usize + 8; // braces and eight commas for nine members
    add_json_string_member(&mut total, "claim_id", shape.claim_id.as_str().len())?;
    checked_usize_add(&mut total, "decision_id".len() + 3)?;
    checked_usize_add(
        &mut total,
        shape.decision_id.map_or(4, |id| id.as_str().len() + 2),
    )?;
    checked_usize_add(&mut total, "evidence_ids".len() + 3)?;
    checked_usize_add(
        &mut total,
        json_id_array_len(shape.evidence_count, shape.evidence_id_bytes)?,
    )?;
    checked_usize_add(&mut total, "id".len() + "finding:sha256:".len() + 64 + 5)?;
    checked_usize_add(
        &mut total,
        "projection_descriptor_id".len() + FINDING_PROJECTION_ID.len() + 5,
    )?;
    checked_usize_add(
        &mut total,
        "schema".len() + "reviewgraphen.finding.v3".len() + 5,
    )?;
    let status_len = match shape.status {
        FindingStatusV3::UnverifiedCandidate => "unverified_candidate".len(),
        FindingStatusV3::VerifiedCandidate => "verified_candidate".len(),
        FindingStatusV3::Accepted => "accepted".len(),
        FindingStatusV3::Rejected => "rejected".len(),
    };
    checked_usize_add(&mut total, "status".len() + status_len + 5)?;
    checked_usize_add(&mut total, "supersedes_finding_id".len() + 3)?;
    checked_usize_add(
        &mut total,
        shape.supersedes_id.map_or(4, |id| id.as_str().len() + 2),
    )?;
    checked_usize_add(&mut total, "verification_ids".len() + 3)?;
    checked_usize_add(
        &mut total,
        json_id_array_len(shape.verification_count, shape.verification_id_bytes)?,
    )?;
    Ok(total)
}

fn validation(message: impl Into<String>) -> M4Error {
    M4Error::InvalidRecord {
        reason: message.into(),
    }
}

fn require_kind(id: &StableId, kind: &str, field: &str) -> Result<()> {
    if id.kind() != kind {
        return Err(M4Error::InvalidIdKind {
            field: field.to_owned(),
            expected: kind.to_owned(),
            actual: id.kind().to_owned(),
        });
    }
    Ok(())
}

fn require_text(value: &str, limit: usize, field: &'static str) -> Result<()> {
    if value.is_empty() {
        return Err(M4Error::EmptyField { field });
    }
    if value.len() > limit {
        return Err(M4Error::Incomplete {
            operation: field,
            limit,
            observed: value.len(),
        });
    }
    Ok(())
}

pub(crate) fn validate_utc_seconds(value: &str, field: &'static str) -> Result<()> {
    let bytes = value.as_bytes();
    let structural = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes.iter().enumerate().all(|(index, byte)| {
            matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit()
        });
    if !structural {
        return Err(validation(format!(
            "{field} must be canonical RFC3339 UTC with second precision"
        )));
    }
    let number = |start: usize, end: usize| -> u32 {
        bytes[start..end]
            .iter()
            .fold(0, |value, digit| value * 10 + u32::from(digit - b'0'))
    };
    let year = number(0, 4);
    let month = number(5, 7);
    let day = number(8, 10);
    let hour = number(11, 13);
    let minute = number(14, 16);
    let second = number(17, 19);
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap_year => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 59 {
        return Err(validation(format!("{field} is not a valid UTC timestamp")));
    }
    Ok(())
}

fn require_count(len: usize, min: usize, max: usize, field: &'static str) -> Result<()> {
    if len < min {
        return Err(M4Error::EmptyField { field });
    }
    if len > max {
        return Err(M4Error::Incomplete {
            operation: field,
            limit: max,
            observed: len,
        });
    }
    Ok(())
}

fn canonical_len(value: &impl Serialize, operation: &'static str) -> Result<usize> {
    struct CountingWriter {
        count: usize,
        overflow: Option<usize>,
    }
    impl Write for CountingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let Some(next) = self.count.checked_add(bytes.len()) else {
                self.overflow = Some(usize::MAX);
                return Err(io::Error::other("M4 canonical size overflow"));
            };
            if next > MAX_RECORD_BYTES {
                self.overflow = Some(next);
                return Err(io::Error::other("M4 canonical size limit"));
            }
            self.count = next;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = CountingWriter {
        count: 0,
        overflow: None,
    };
    if let Err(error) = serde_json::to_writer(&mut counter, value) {
        if let Some(observed) = counter.overflow {
            return Err(M4Error::Incomplete {
                operation,
                limit: MAX_RECORD_BYTES,
                observed,
            });
        }
        return Err(M4Error::Canonical {
            reason: error.to_string(),
        });
    }
    Ok(counter.count)
}

fn bounded_bytes(value: &impl Serialize, operation: &'static str) -> Result<Vec<u8>> {
    let count = canonical_len(value, operation)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count)
        .map_err(|_| M4Error::Incomplete {
            operation,
            limit: MAX_RECORD_BYTES,
            observed: count,
        })?;
    serde_json::to_writer(&mut bytes, value).map_err(|error| M4Error::Canonical {
        reason: error.to_string(),
    })?;
    if bytes.len() != count {
        return Err(M4Error::Canonical {
            reason: "bounded M4 serialization size/capacity mismatch".to_owned(),
        });
    }
    Ok(bytes)
}

fn derived_id(kind: &str, identity: &impl Serialize) -> Result<StableId> {
    Ok(StableId::parse(format!(
        "{kind}:{}",
        ContentHash::sha256(&bounded_bytes(identity, "M4 identity")?)
    ))?)
}

fn body_hash(value: &impl Serialize) -> Result<ContentHash> {
    Ok(ContentHash::sha256(&bounded_bytes(value, "M4 record")?))
}

fn strict_ids(values: Vec<StableId>, field: &'static str) -> Result<BTreeSet<StableId>> {
    let set = values.iter().cloned().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(M4Error::Duplicate { field });
    }
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(M4Error::NonCanonicalOrder { field });
    }
    Ok(set)
}

fn strict_bounded_ids(
    values: Vec<BoundedStableId>,
    field: &'static str,
) -> Result<BTreeSet<StableId>> {
    strict_ids(values.into_iter().map(|value| value.0).collect(), field)
}

fn strict_strings(values: Vec<String>, field: &'static str) -> Result<BTreeSet<String>> {
    let set = values.iter().cloned().collect::<BTreeSet<_>>();
    if set.len() != values.len() {
        return Err(M4Error::Duplicate { field });
    }
    if !values.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(M4Error::NonCanonicalOrder { field });
    }
    Ok(set)
}

fn strict_bounded_strings<const MAX: usize>(
    values: Vec<BoundedString<MAX>>,
    field: &'static str,
) -> Result<BTreeSet<String>> {
    strict_strings(values.into_iter().map(|value| value.0).collect(), field)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BoundedSeq<T, const MAX: usize>(Vec<T>);

impl<'de, T: Deserialize<'de> + Ord, const MAX: usize> Deserialize<'de> for BoundedSeq<T, MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct BoundedVisitor<T, const MAX: usize>(PhantomData<T>);
        impl<'de, T: Deserialize<'de> + Ord, const MAX: usize> Visitor<'de> for BoundedVisitor<T, MAX> {
            type Value = BoundedSeq<T, MAX>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "an array of at most {MAX} values")
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                if sequence.size_hint().is_some_and(|hint| hint > MAX) {
                    return Err(serde::de::Error::custom("M4 array exceeds its fixed cap"));
                }
                let mut values = Vec::new();
                while values.len() < MAX {
                    values.try_reserve_exact(1).map_err(|_| {
                        serde::de::Error::custom("M4 array reservation exceeds capacity")
                    })?;
                    match sequence.next_element()? {
                        Some(value) => {
                            if values.last().is_some_and(|previous| previous >= &value) {
                                return Err(serde::de::Error::custom(
                                    "M4 array is duplicated or not canonically ordered",
                                ));
                            }
                            values.push(value);
                        }
                        None => return Ok(BoundedSeq(values)),
                    }
                }
                if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    return Err(serde::de::Error::custom("M4 array exceeds its fixed cap"));
                }
                Ok(BoundedSeq(values))
            }
        }
        deserializer.deserialize_seq(BoundedVisitor::<T, MAX>(PhantomData))
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct BoundedString<const MAX: usize>(String);

impl<'de, const MAX: usize> Deserialize<'de> for BoundedString<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct StringVisitor<const MAX: usize>;
        impl<'de, const MAX: usize> Visitor<'de> for StringVisitor<MAX> {
            type Value = BoundedString<MAX>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "a UTF-8 string of at most {MAX} bytes")
            }
            fn visit_borrowed_str<E: serde::de::Error>(
                self,
                value: &'de str,
            ) -> std::result::Result<Self::Value, E> {
                self.visit_str(value)
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("M4 string exceeds its fixed UTF-8 cap"));
                }
                let mut owned = String::new();
                owned
                    .try_reserve_exact(value.len())
                    .map_err(|_| E::custom("M4 string reservation failed"))?;
                owned.push_str(value);
                Ok(BoundedString(owned))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                if value.len() > MAX {
                    return Err(E::custom("M4 string exceeds its fixed UTF-8 cap"));
                }
                Ok(BoundedString(value))
            }
        }
        deserializer.deserialize_str(StringVisitor::<MAX>)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct BoundedStableId(StableId);

impl<'de> Deserialize<'de> for BoundedStableId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = BoundedString::<MAX_TRACE_BYTES>::deserialize(deserializer)?;
        StableId::parse(raw.0)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
struct BoundedContentHash(ContentHash);

impl<'de> Deserialize<'de> for BoundedContentHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = BoundedString::<MAX_TRACE_BYTES>::deserialize(deserializer)?;
        ContentHash::parse(raw.0)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum M4SensitivityV3 {
    CanonicalState,
}

/// Untrusted, strict wire description. Parsing this value grants no authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthorityScopeDescriptorV3 {
    claim_body_hash: BoundedContentHash,
    claim_id: BoundedStableId,
    descriptor: VerifierDescriptorV3,
    genesis_hash: BoundedContentHash,
    harness_id: BoundedString<MAX_TRACE_BYTES>,
    harness_revision: BoundedString<MAX_TRACE_BYTES>,
    harness_source_hash: BoundedContentHash,
    polarity: ClaimPolarity,
    policy_revision_hash: BoundedContentHash,
    procedure: VerifierProcedureV3,
    property_id: BoundedString<MAX_TRACE_BYTES>,
    repository_id: BoundedStableId,
    repository_source_hash: BoundedContentHash,
    result_hash: BoundedContentHash,
    result_media_type: BoundedString<MAX_TRACE_BYTES>,
    result_sensitivity: M4SensitivityV3,
    result_size: u64,
    run_id: BoundedStableId,
    snapshot_id: BoundedStableId,
    source_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    target_refs: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    test_artifact_id: BoundedStableId,
    universe_id: BoundedStableId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityScopeDescriptorWire {
    claim_body_hash: BoundedContentHash,
    claim_id: BoundedStableId,
    descriptor: VerifierDescriptorWire,
    genesis_hash: BoundedContentHash,
    harness_id: BoundedString<MAX_TRACE_BYTES>,
    harness_revision: BoundedString<MAX_TRACE_BYTES>,
    harness_source_hash: BoundedContentHash,
    polarity: ClaimPolarity,
    policy_revision_hash: BoundedContentHash,
    procedure: VerifierProcedureWire,
    property_id: BoundedString<MAX_TRACE_BYTES>,
    repository_id: BoundedStableId,
    repository_source_hash: BoundedContentHash,
    result_hash: BoundedContentHash,
    result_media_type: BoundedString<MAX_TRACE_BYTES>,
    result_sensitivity: M4SensitivityWire,
    result_size: u64,
    run_id: BoundedStableId,
    snapshot_id: BoundedStableId,
    source_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    target_refs: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    test_artifact_id: BoundedStableId,
    universe_id: BoundedStableId,
}

impl AuthorityScopeDescriptorV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let raw: AuthorityScopeDescriptorWire =
            decode_m4_wire(input, "M4 authority descriptor JSON")?;
        Ok(Self {
            claim_body_hash: raw.claim_body_hash,
            claim_id: raw.claim_id,
            descriptor: raw.descriptor.into(),
            genesis_hash: raw.genesis_hash,
            harness_id: raw.harness_id,
            harness_revision: raw.harness_revision,
            harness_source_hash: raw.harness_source_hash,
            polarity: raw.polarity,
            policy_revision_hash: raw.policy_revision_hash,
            procedure: raw.procedure.into(),
            property_id: raw.property_id,
            repository_id: raw.repository_id,
            repository_source_hash: raw.repository_source_hash,
            result_hash: raw.result_hash,
            result_media_type: raw.result_media_type,
            result_sensitivity: raw.result_sensitivity.into(),
            result_size: raw.result_size,
            run_id: raw.run_id,
            snapshot_id: raw.snapshot_id,
            source_ids: raw.source_ids,
            target_refs: raw.target_refs,
            test_artifact_id: raw.test_artifact_id,
            universe_id: raw.universe_id,
        })
    }
}

/// Sealed evidence that a trusted host checked the complete untrusted tuple.
/// A production constructor is intentionally absent in A1.
#[allow(dead_code)]
pub(crate) struct VerifiedHostScopeProofV3 {
    descriptor: AuthorityScopeDescriptorV3,
    descriptor_hash: ContentHash,
}

#[allow(dead_code)]
impl VerifiedHostScopeProofV3 {
    #[cfg(test)]
    fn for_test(raw: &AuthorityScopeDescriptorV3) -> Result<Self> {
        Ok(Self {
            descriptor: raw.clone(),
            descriptor_hash: ContentHash::sha256(&bounded_bytes(raw, "M4 authority descriptor")?),
        })
    }
}

#[allow(dead_code)]
pub(crate) struct LockedVerificationProofV3 {
    scope: AuthorityScopeV3,
    scope_hash: ContentHash,
}

#[allow(dead_code)]
impl LockedVerificationProofV3 {
    #[cfg(test)]
    fn for_test(scope: &AuthorityScopeV3) -> Result<Self> {
        Ok(Self {
            scope: scope.clone(),
            scope_hash: ContentHash::sha256(&bounded_bytes(scope, "M4 verification scope")?),
        })
    }
    fn validate(&self, scope: &AuthorityScopeV3) -> Result<()> {
        if self.scope != *scope
            || self.scope_hash
                != ContentHash::sha256(&bounded_bytes(scope, "M4 verification scope")?)
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        Ok(())
    }
}

#[allow(dead_code)]
pub(crate) struct TrustedHumanGrantProofV3 {
    scope: AuthorityScopeV3,
    scope_hash: ContentHash,
}

#[allow(dead_code)]
impl TrustedHumanGrantProofV3 {
    #[cfg(test)]
    fn for_test(scope: &AuthorityScopeV3) -> Result<Self> {
        Ok(Self {
            scope: scope.clone(),
            scope_hash: ContentHash::sha256(&bounded_bytes(scope, "M4 decision scope")?),
        })
    }
    fn validate(&self, scope: &AuthorityScopeV3) -> Result<()> {
        if self.scope != *scope
            || self.scope_hash != ContentHash::sha256(&bounded_bytes(scope, "M4 decision scope")?)
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        Ok(())
    }
}

#[allow(dead_code)]
pub(crate) struct LockedStaticProofV3 {
    scope: StaticScopeV3,
}

#[allow(dead_code)]
impl LockedStaticProofV3 {
    #[cfg(test)]
    fn for_test(scope: &StaticScopeV3) -> Self {
        Self {
            scope: scope.clone(),
        }
    }
    fn validate(&self, scope: &StaticScopeV3) -> Result<()> {
        if self.scope != *scope {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        Ok(())
    }
}

/// Exact claim-local authority scope. It is deliberately non-deserializable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuthorityScopeV3 {
    claim_body_hash: ContentHash,
    claim_id: StableId,
    descriptor: VerifierDescriptorV3,
    genesis_hash: ContentHash,
    harness_id: String,
    harness_revision: String,
    harness_source_hash: ContentHash,
    polarity: ClaimPolarity,
    policy_revision_hash: ContentHash,
    procedure: VerifierProcedureV3,
    property_id: String,
    repository_id: StableId,
    repository_source_hash: ContentHash,
    result_hash: ContentHash,
    result_media_type: String,
    result_sensitivity: M4SensitivityV3,
    result_size: u64,
    run_id: StableId,
    snapshot_id: StableId,
    source_ids: Vec<StableId>,
    target_refs: Vec<StableId>,
    test_artifact_id: StableId,
    universe_id: StableId,
}

#[allow(dead_code)]
impl AuthorityScopeV3 {
    #[cfg(test)]
    fn new(
        policy_revision_hash: ContentHash,
        run_id: StableId,
        snapshot_id: StableId,
        universe_id: StableId,
        claim: &ExecutionClaimV2,
    ) -> Result<Self> {
        require_kind(&run_id, "run", "authority run")?;
        require_kind(&snapshot_id, "snapshot", "authority snapshot")?;
        require_kind(&universe_id, "universe", "authority universe")?;
        let scope = Self {
            policy_revision_hash,
            repository_id: StableId::parse("repository:test")?,
            repository_source_hash: ContentHash::sha256(b"repository"),
            run_id,
            genesis_hash: ContentHash::sha256(b"genesis"),
            snapshot_id,
            universe_id,
            claim_id: claim.id().clone(),
            claim_body_hash: claim.body_hash()?,
            property_id: claim.property_id().to_owned(),
            polarity: claim.polarity(),
            target_refs: claim.target_refs().iter().cloned().collect(),
            source_ids: claim.source_ids().iter().cloned().collect(),
            harness_id: FIXTURE_HARNESS_ID.to_owned(),
            harness_revision: FIXTURE_HARNESS_REVISION.to_owned(),
            harness_source_hash: ContentHash::sha256(b"harness"),
            test_artifact_id: StableId::parse(FIXTURE_TEST_ARTIFACT_ID)?,
            result_hash: ContentHash::parse(FIXTURE_WITNESS_HASH)?,
            result_size: 145,
            result_media_type: FIXTURE_MEDIA_TYPE.to_owned(),
            result_sensitivity: M4SensitivityV3::CanonicalState,
            descriptor: VerifierDescriptorV3::FixedFixtureV1,
            procedure: VerifierProcedureV3::DuplicateSubmitV1,
        };
        scope.validate()?;
        Ok(scope)
    }

    pub(crate) fn admit(
        raw: AuthorityScopeDescriptorV3,
        claim: &ExecutionClaimV2,
        proof: VerifiedHostScopeProofV3,
    ) -> Result<Self> {
        if proof.descriptor != raw
            || proof.descriptor_hash
                != ContentHash::sha256(&bounded_bytes(&raw, "M4 authority descriptor")?)
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let scope = Self {
            policy_revision_hash: raw.policy_revision_hash.0,
            repository_id: raw.repository_id.0,
            repository_source_hash: raw.repository_source_hash.0,
            run_id: raw.run_id.0,
            genesis_hash: raw.genesis_hash.0,
            snapshot_id: raw.snapshot_id.0,
            universe_id: raw.universe_id.0,
            claim_id: raw.claim_id.0,
            claim_body_hash: raw.claim_body_hash.0,
            property_id: raw.property_id.0,
            polarity: raw.polarity,
            target_refs: strict_bounded_ids(raw.target_refs.0, "M4 targets")?
                .into_iter()
                .collect(),
            source_ids: strict_bounded_ids(raw.source_ids.0, "M4 sources")?
                .into_iter()
                .collect(),
            harness_id: raw.harness_id.0,
            harness_revision: raw.harness_revision.0,
            harness_source_hash: raw.harness_source_hash.0,
            test_artifact_id: raw.test_artifact_id.0,
            result_hash: raw.result_hash.0,
            result_size: raw.result_size,
            result_media_type: raw.result_media_type.0,
            result_sensitivity: raw.result_sensitivity,
            descriptor: raw.descriptor,
            procedure: raw.procedure,
        };
        scope.validate()?;
        scope.validate_claim(claim)?;
        Ok(scope)
    }

    fn validate(&self) -> Result<()> {
        require_kind(&self.run_id, "run", "authority run")?;
        require_kind(&self.repository_id, "repository", "authority repository")?;
        require_kind(&self.snapshot_id, "snapshot", "authority snapshot")?;
        require_kind(&self.universe_id, "universe", "authority universe")?;
        require_kind(&self.claim_id, "claim", "authority claim")?;
        require_kind(&self.test_artifact_id, "test", "authority test artifact")?;
        require_text(&self.property_id, MAX_TRACE_BYTES, "M4 property")?;
        require_count(
            self.target_refs.len(),
            1,
            MAX_EVIDENCE_SUBJECTS,
            "M4 targets",
        )?;
        for (value, field) in [
            (&self.harness_id, "authority harness"),
            (&self.harness_revision, "authority harness revision"),
            (&self.result_media_type, "authority result media type"),
        ] {
            require_text(value, MAX_TRACE_BYTES, field)?;
        }
        if self.property_id != M4_PROPERTY_ID
            || self.polarity != ClaimPolarity::IssuePresent
            || self.harness_id != FIXTURE_HARNESS_ID
            || self.harness_revision != FIXTURE_HARNESS_REVISION
            || self.test_artifact_id.as_str() != FIXTURE_TEST_ARTIFACT_ID
            || self.result_hash.as_str() != FIXTURE_WITNESS_HASH
            || self.result_size != 145
            || self.result_media_type != FIXTURE_MEDIA_TYPE
            || self.result_sensitivity != M4SensitivityV3::CanonicalState
            || self.descriptor != VerifierDescriptorV3::FixedFixtureV1
            || self.procedure != VerifierProcedureV3::DuplicateSubmitV1
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        require_count(
            self.source_ids.len(),
            1,
            MAX_EVIDENCE_SUBJECTS,
            "M4 sources",
        )?;
        let _ = bounded_bytes(self, "M4 authority scope")?;
        Ok(())
    }

    pub fn validate_claim(&self, claim: &ExecutionClaimV2) -> Result<()> {
        claim.validate_shape()?;
        if self.claim_id != *claim.id() {
            return Err(M4Error::ClaimMismatch {
                expected: self.claim_id.clone(),
                actual: claim.id().clone(),
            });
        }
        if self.claim_body_hash != claim.body_hash()? {
            return Err(M4Error::ClaimBodyMismatch);
        }
        if self.property_id != claim.property_id() {
            return Err(M4Error::PropertyMismatch {
                expected: self.property_id.clone(),
                actual: claim.property_id().to_owned(),
            });
        }
        if self.polarity != claim.polarity()
            || self.target_refs.len() != claim.target_refs().len()
            || !self
                .target_refs
                .iter()
                .all(|id| claim.target_refs().contains(id))
            || self.source_ids.len() != claim.source_ids().len()
            || !self
                .source_ids
                .iter()
                .all(|id| claim.source_ids().contains(id))
        {
            return Err(M4Error::ClaimBodyMismatch);
        }
        Ok(())
    }

    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub fn claim_body_hash(&self) -> &ContentHash {
        &self.claim_body_hash
    }
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    pub const fn polarity(&self) -> ClaimPolarity {
        self.polarity
    }
    pub fn target_refs(&self) -> &[StableId] {
        &self.target_refs
    }
    pub fn source_ids(&self) -> &[StableId] {
        &self.source_ids
    }
    pub fn policy_revision_hash(&self) -> &ContentHash {
        &self.policy_revision_hash
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.target_refs)?
            .checked_add(id_vec_allocated_bytes(&self.source_ids)?)
            .ok_or(M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        for bytes in [
            self.policy_revision_hash.allocated_bytes(),
            self.repository_id.allocated_bytes(),
            self.repository_source_hash.allocated_bytes(),
            self.run_id.allocated_bytes(),
            self.genesis_hash.allocated_bytes(),
            self.snapshot_id.allocated_bytes(),
            self.universe_id.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.claim_body_hash.allocated_bytes(),
            self.property_id.capacity(),
            self.harness_id.capacity(),
            self.harness_revision.capacity(),
            self.harness_source_hash.allocated_bytes(),
            self.test_artifact_id.allocated_bytes(),
            self.result_hash.allocated_bytes(),
            self.result_media_type.capacity(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierDescriptorV3 {
    #[serde(rename = "reviewgraphen.static_fact_verifier@1")]
    StaticFactV1,
    #[serde(rename = "reviewgraphen.fixture_test_verifier@1")]
    FixedFixtureV1,
}

impl VerifierDescriptorV3 {
    pub const fn id(self) -> &'static str {
        match self {
            Self::StaticFactV1 => STATIC_DESCRIPTOR_ID,
            Self::FixedFixtureV1 => FIXTURE_DESCRIPTOR_ID,
        }
    }
    pub const fn procedure(self) -> VerifierProcedureV3 {
        match self {
            Self::StaticFactV1 => VerifierProcedureV3::StaticProjectionV1,
            Self::FixedFixtureV1 => VerifierProcedureV3::DuplicateSubmitV1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum VerifierProcedureV3 {
    #[serde(rename = "reviewgraphen.static_fact.projection@1")]
    StaticProjectionV1,
    #[serde(rename = "reviewgraphen.fixture_test.duplicate_submit@1")]
    DuplicateSubmitV1,
}

impl VerifierProcedureV3 {
    pub const fn id(self) -> &'static str {
        match self {
            Self::StaticProjectionV1 => STATIC_PROCEDURE_ID,
            Self::DuplicateSubmitV1 => FIXTURE_PROCEDURE_ID,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationOutcomeV3 {
    Passed,
    Inconclusive,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StaticApplicabilityV1 {
    Absent,
    Unique,
    Ambiguous,
    UnsupportedProperty,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKindV3 {
    StaticFact,
    TestWitness,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceObservationV3 {
    FactPresent,
    Witnessed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelationV3 {
    Qualifies,
    Reproduces,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaticFactInputV1 {
    candidate_invariant_ids: BTreeSet<StableId>,
    claim_id: StableId,
    claim_source_ids: BTreeSet<StableId>,
    obligation_context_ids: BTreeSet<StableId>,
    obligation_id: StableId,
    obligation_source_ids: BTreeSet<StableId>,
    obligation_target_refs: BTreeSet<StableId>,
    property_id: String,
    schema: String,
    selected_invariant_id: Option<StableId>,
    selected_invariant_scope_ids: BTreeSet<StableId>,
    snapshot_id: StableId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticFactInputWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    claim_id: BoundedStableId,
    snapshot_id: BoundedStableId,
    property_id: BoundedString<MAX_TRACE_BYTES>,
    obligation_id: BoundedStableId,
    obligation_target_refs: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    obligation_context_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    obligation_source_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    claim_source_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    candidate_invariant_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    selected_invariant_id: Option<BoundedStableId>,
    selected_invariant_scope_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
}

impl StaticFactInputV1 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let raw: StaticFactInputWire = decode_m4_wire(input, "static fact input JSON")?;
        let value = Self {
            schema: raw.schema.0,
            claim_id: raw.claim_id.0,
            snapshot_id: raw.snapshot_id.0,
            property_id: raw.property_id.0,
            obligation_id: raw.obligation_id.0,
            obligation_target_refs: strict_bounded_ids(
                raw.obligation_target_refs.0,
                "static obligation targets",
            )?,
            obligation_context_ids: strict_bounded_ids(
                raw.obligation_context_ids.0,
                "static obligation contexts",
            )?,
            obligation_source_ids: strict_bounded_ids(
                raw.obligation_source_ids.0,
                "static obligation sources",
            )?,
            claim_source_ids: strict_bounded_ids(raw.claim_source_ids.0, "static claim sources")?,
            candidate_invariant_ids: strict_bounded_ids(
                raw.candidate_invariant_ids.0,
                "static candidate invariants",
            )?,
            selected_invariant_id: raw.selected_invariant_id.map(|value| value.0),
            selected_invariant_scope_ids: strict_bounded_ids(
                raw.selected_invariant_scope_ids.0,
                "static selected invariant scope",
            )?,
        };
        value.validate()?;
        Ok(value)
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "static fact input")
    }

    pub(crate) fn canonical_size(&self) -> Result<u64> {
        u64::try_from(canonical_len(self, "static fact input")?).map_err(|_| M4Error::Incomplete {
            operation: "static fact input canonical size",
            limit: MAX_RECORD_BYTES,
            observed: usize::MAX,
        })
    }

    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = 0_u64;
        for values in [
            &self.candidate_invariant_ids,
            &self.claim_source_ids,
            &self.obligation_context_ids,
            &self.obligation_source_ids,
            &self.obligation_target_refs,
            &self.selected_invariant_scope_ids,
        ] {
            total =
                total
                    .checked_add(id_set_allocated_bytes(values)?)
                    .ok_or(M4Error::Incomplete {
                        operation: "M4 retained verifier bytes",
                        limit: MAX_RETAINED_WORKING_BYTES as usize,
                        observed: usize::MAX,
                    })?;
        }
        for bytes in [
            self.claim_id.allocated_bytes(),
            self.obligation_id.allocated_bytes(),
            self.property_id.capacity(),
            self.schema.capacity(),
            self.selected_invariant_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
            self.snapshot_id.allocated_bytes(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
}

impl StaticFactInputV1 {
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.static_fact_input.v1" {
            return Err(validation("invalid static input schema"));
        }
        require_kind(&self.claim_id, "claim", "static input claim")?;
        require_kind(&self.snapshot_id, "snapshot", "static input snapshot")?;
        require_kind(&self.obligation_id, "obligation", "static input obligation")?;
        let selected = self.selected_invariant_id.is_some();
        if selected != (self.candidate_invariant_ids.len() == 1)
            || selected == self.selected_invariant_scope_ids.is_empty()
            || self
                .selected_invariant_id
                .as_ref()
                .is_some_and(|id| !self.candidate_invariant_ids.contains(id))
        {
            return Err(validation("invalid static input candidate selection"));
        }
        let _ = bounded_bytes(self, "StaticFactInputV1")?;
        Ok(())
    }

    fn reconstructed_applicability(&self) -> StaticApplicabilityV1 {
        if self.property_id != M4_PROPERTY_ID {
            StaticApplicabilityV1::UnsupportedProperty
        } else {
            match self.candidate_invariant_ids.len() {
                0 => StaticApplicabilityV1::Absent,
                1 => StaticApplicabilityV1::Unique,
                _ => StaticApplicabilityV1::Ambiguous,
            }
        }
    }

    pub(crate) fn projected_result_accounting(&self) -> Result<(u64, u64)> {
        #[derive(Serialize)]
        struct ProjectedResult<'a> {
            applicability: StaticApplicabilityV1,
            claim_id: &'a StableId,
            limitations: [&'static str; 1],
            observation: Option<EvidenceObservationV3>,
            outcome: VerificationOutcomeV3,
            schema: &'static str,
        }
        let applicability = self.reconstructed_applicability();
        let (outcome, observation, limitation) = static_result_parts(applicability);
        let projection = ProjectedResult {
            applicability,
            claim_id: &self.claim_id,
            limitations: [limitation],
            observation,
            outcome,
            schema: "reviewgraphen.static_fact_result.v1",
        };
        let canonical = u64::try_from(canonical_len(&projection, "static projected result")?)
            .map_err(|_| M4Error::Incomplete {
                operation: "static projected result canonical size",
                limit: MAX_RECORD_BYTES,
                observed: usize::MAX,
            })?;
        let mut retained = u64::try_from(size_of::<StaticFactResultV1>()).unwrap_or(u64::MAX);
        for bytes in [
            self.claim_id.as_str().len(),
            size_of::<String>(),
            limitation.len(),
            "reviewgraphen.static_fact_result.v1".len(),
        ] {
            checked_memory_add(&mut retained, bytes)?;
        }
        Ok((retained, canonical))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaticScopeV3 {
    candidate_invariant_ids: BTreeSet<StableId>,
    claim_body_hash: ContentHash,
    claim_id: StableId,
    descriptor: VerifierDescriptorV3,
    obligation_id: StableId,
    procedure: VerifierProcedureV3,
    property_id: String,
    selected_invariant_id: Option<StableId>,
    snapshot_id: StableId,
    source_ids: BTreeSet<StableId>,
    target_refs: BTreeSet<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaticFactEvaluationV1 {
    evidence_subject_ids: Option<BTreeSet<StableId>>,
    input: StaticFactInputV1,
    result: StaticFactResultV1,
    scope: StaticScopeV3,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaticRecordProposalV1 {
    binding: Option<EvidenceBindingV3>,
    evidence: Option<EvidenceV3>,
    scope: StaticScopeV3,
    verification: VerificationV3,
}

impl StaticRecordProposalV1 {
    pub(crate) fn evidence(&self) -> Option<&EvidenceV3> {
        self.evidence.as_ref()
    }
    pub(crate) fn binding(&self) -> Option<&EvidenceBindingV3> {
        self.binding.as_ref()
    }
    pub(crate) fn verification(&self) -> &VerificationV3 {
        &self.verification
    }
}

impl StaticFactEvaluationV1 {
    pub fn scope(&self) -> &StaticScopeV3 {
        &self.scope
    }
    pub fn input(&self) -> &StaticFactInputV1 {
        &self.input
    }
    pub fn result(&self) -> &StaticFactResultV1 {
        &self.result
    }
    pub fn evidence_subject_ids(&self) -> Option<&BTreeSet<StableId>> {
        self.evidence_subject_ids.as_ref()
    }

    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = self.input.allocated_bytes()?;
        let scope = self.scope.allocated_bytes()?;
        total = total
            .checked_add(self.result.allocated_bytes()?)
            .and_then(|value| value.checked_add(scope))
            .ok_or(M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        if let Some(subject_ids) = &self.evidence_subject_ids {
            total = total
                .checked_add(id_set_allocated_bytes(subject_ids)?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        Ok(total)
    }
    pub fn materialize(
        &self,
        input_registration_id: StableId,
        output_registration_id: StableId,
    ) -> Result<StaticRecordProposalV1> {
        require_kind(
            &input_registration_id,
            "registration",
            "static input registration",
        )?;
        require_kind(
            &output_registration_id,
            "registration",
            "static output registration",
        )?;
        let (evidence, binding, evidence_ids) = if let Some(subjects) = &self.evidence_subject_ids {
            let evidence = EvidenceV3::build_static(
                &self.scope,
                subjects.clone(),
                input_registration_id.clone(),
                output_registration_id.clone(),
            )?;
            let binding = EvidenceBindingV3::build_static(&self.scope, &evidence)?;
            let ids = BTreeSet::from([evidence.id.clone()]);
            (Some(evidence), Some(binding), ids)
        } else {
            (None, None, BTreeSet::new())
        };
        let verification = VerificationV3::build_static(
            &self.scope,
            input_registration_id,
            output_registration_id,
            evidence_ids,
            self.result.outcome,
            self.result.limitations.clone(),
        )?;
        Ok(StaticRecordProposalV1 {
            scope: self.scope.clone(),
            evidence,
            binding,
            verification,
        })
    }
}

#[cfg(test)]
std::thread_local! {
    static STATIC_EVALUATOR_CALLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static STATIC_RECONSTRUCTION_ALLOCATION_SEAMS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_static_evaluator_calls() {
    STATIC_EVALUATOR_CALLS.with(|calls| calls.set(0));
}

#[cfg(test)]
pub(crate) fn static_evaluator_calls() -> u64 {
    STATIC_EVALUATOR_CALLS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_static_reconstruction_allocation_seams() {
    STATIC_RECONSTRUCTION_ALLOCATION_SEAMS.with(|value| value.set(0));
}

#[cfg(test)]
pub(crate) fn static_reconstruction_allocation_seams() -> u64 {
    STATIC_RECONSTRUCTION_ALLOCATION_SEAMS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn mark_static_reconstruction_allocation_seam() {
    STATIC_RECONSTRUCTION_ALLOCATION_SEAMS.with(|value| value.set(value.get() + 1));
}

#[cfg(not(test))]
fn mark_static_reconstruction_allocation_seam() {}

/// Pure static applicability evaluation. Candidate cardinality is computed by
/// core and cannot be selected by a caller.
pub fn evaluate_static_fact_v1(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> Result<StaticFactEvaluationV1> {
    #[cfg(test)]
    STATIC_EVALUATOR_CALLS.with(|calls| calls.set(calls.get() + 1));
    derive_static_fact_evaluation_v1(program, obligation, claim)
}

pub(crate) fn reconstruct_static_evaluation_v1(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> Result<StaticFactEvaluationV1> {
    derive_static_fact_evaluation_v1(program, obligation, claim)
}

fn derive_static_fact_evaluation_v1(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> Result<StaticFactEvaluationV1> {
    claim.validate_shape()?;
    if obligation.version().snapshot() != program.snapshot_id() {
        return Err(M4Error::SnapshotMismatch {
            expected: program.snapshot_id().clone(),
            actual: obligation.version().snapshot().clone(),
        });
    }
    if claim.obligation_ids().len() != 1 || !claim.obligation_ids().contains(obligation.id()) {
        return Err(M4Error::ClaimMismatch {
            expected: obligation.id().clone(),
            actual: claim.id().clone(),
        });
    }
    let supported = claim.property_id() == M4_PROPERTY_ID
        && obligation.property_id() == M4_PROPERTY_ID
        && claim.property_id() == obligation.property_id();
    let mut closure = obligation.normalized_target_refs().clone();
    closure.extend(obligation.normalized_context_ids().iter().cloned());
    closure.extend(obligation.normalized_source_ids().iter().cloned());
    closure.extend(claim.source_ids().iter().cloned());
    let relevance = claim
        .target_refs()
        .iter()
        .chain(obligation.normalized_context_ids())
        .chain(claim.source_ids())
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidates = if supported {
        program
            .invariants()
            .iter()
            .filter(|invariant| {
                invariant.property_id == claim.property_id()
                    && !invariant.scope_ids.is_empty()
                    && invariant.scope_ids.is_subset(&closure)
                    && !invariant.scope_ids.is_disjoint(&relevance)
            })
            .map(|invariant| invariant.id.clone())
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let applicability = if !supported {
        StaticApplicabilityV1::UnsupportedProperty
    } else {
        match candidates.len() {
            0 => StaticApplicabilityV1::Absent,
            1 => StaticApplicabilityV1::Unique,
            _ => StaticApplicabilityV1::Ambiguous,
        }
    };
    let selected_invariant_id = (candidates.len() == 1)
        .then(|| candidates.first().cloned())
        .flatten();
    let selected_invariant_scope_ids = selected_invariant_id
        .as_ref()
        .and_then(|id| program.invariant(id))
        .map_or_else(BTreeSet::new, |invariant| invariant.scope_ids.clone());
    let evidence_subject_ids = selected_invariant_id.as_ref().map(|id| {
        let mut subjects = claim.target_refs().clone();
        subjects.insert(id.clone());
        subjects
    });
    let input = StaticFactInputV1 {
        schema: "reviewgraphen.static_fact_input.v1".to_owned(),
        claim_id: claim.id().clone(),
        snapshot_id: program.snapshot_id().clone(),
        property_id: claim.property_id().to_owned(),
        obligation_id: obligation.id().clone(),
        obligation_target_refs: obligation.normalized_target_refs().clone(),
        obligation_context_ids: obligation.normalized_context_ids().clone(),
        obligation_source_ids: obligation.normalized_source_ids().clone(),
        claim_source_ids: claim.source_ids().clone(),
        candidate_invariant_ids: candidates,
        selected_invariant_id,
        selected_invariant_scope_ids,
    };
    input.validate()?;
    let result = StaticFactResultV1::from_applicability(claim.id().clone(), applicability)?;
    let scope = StaticScopeV3 {
        snapshot_id: program.snapshot_id().clone(),
        claim_id: claim.id().clone(),
        claim_body_hash: claim.body_hash()?,
        property_id: claim.property_id().to_owned(),
        obligation_id: obligation.id().clone(),
        target_refs: claim.target_refs().clone(),
        source_ids: claim.source_ids().clone(),
        candidate_invariant_ids: input.candidate_invariant_ids.clone(),
        selected_invariant_id: input.selected_invariant_id.clone(),
        descriptor: VerifierDescriptorV3::StaticFactV1,
        procedure: VerifierProcedureV3::StaticProjectionV1,
    };
    Ok(StaticFactEvaluationV1 {
        scope,
        input,
        result,
        evidence_subject_ids,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StaticFactResultV1 {
    applicability: StaticApplicabilityV1,
    claim_id: StableId,
    limitations: BTreeSet<String>,
    observation: Option<EvidenceObservationV3>,
    outcome: VerificationOutcomeV3,
    schema: String,
}

impl StaticScopeV3 {
    fn allocated_bytes(&self) -> Result<u64> {
        let mut total = 0_u64;
        for values in [
            &self.candidate_invariant_ids,
            &self.source_ids,
            &self.target_refs,
        ] {
            total =
                total
                    .checked_add(id_set_allocated_bytes(values)?)
                    .ok_or(M4Error::Incomplete {
                        operation: "M4 retained verifier bytes",
                        limit: MAX_RETAINED_WORKING_BYTES as usize,
                        observed: usize::MAX,
                    })?;
        }
        for bytes in [
            self.claim_body_hash.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.obligation_id.allocated_bytes(),
            self.property_id.capacity(),
            self.selected_invariant_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
            self.snapshot_id.allocated_bytes(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
}

impl StaticFactResultV1 {
    fn from_applicability(
        claim_id: StableId,
        applicability: StaticApplicabilityV1,
    ) -> Result<Self> {
        let (outcome, observation, limitation) = static_result_parts(applicability);
        let value = Self {
            schema: "reviewgraphen.static_fact_result.v1".to_owned(),
            claim_id,
            applicability,
            outcome,
            observation,
            limitations: BTreeSet::from([limitation.to_owned()]),
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn canonical_size(&self) -> Result<u64> {
        u64::try_from(canonical_len(self, "static fact result")?).map_err(|_| M4Error::Incomplete {
            operation: "static fact result canonical size",
            limit: MAX_RECORD_BYTES,
            observed: usize::MAX,
        })
    }

    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = string_set_allocated_bytes(&self.limitations)?;
        for bytes in [self.claim_id.allocated_bytes(), self.schema.capacity()] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    #[cfg(test)]
    fn new(scope: &AuthorityScopeV3, applicability: StaticApplicabilityV1) -> Result<Self> {
        Self::from_applicability(scope.claim_id.clone(), applicability)
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.static_fact_result.v1" {
            return Err(validation("invalid static result schema"));
        }
        require_kind(&self.claim_id, "claim", "static result claim")?;
        require_count(self.limitations.len(), 1, 1, "static limitations")?;
        for item in &self.limitations {
            require_text(item, MAX_LIMITATION_BYTES, "M4 limitation")?;
        }
        let expected = match self.applicability {
            StaticApplicabilityV1::Absent => (
                VerificationOutcomeV3::Inconclusive,
                None,
                "required declared invariant is absent",
            ),
            StaticApplicabilityV1::Unique => (
                VerificationOutcomeV3::Inconclusive,
                Some(EvidenceObservationV3::FactPresent),
                "declared invariant is not an observed violation",
            ),
            StaticApplicabilityV1::Ambiguous => (
                VerificationOutcomeV3::Inconclusive,
                None,
                "multiple applicable declared invariants",
            ),
            StaticApplicabilityV1::UnsupportedProperty => (
                VerificationOutcomeV3::Unsupported,
                None,
                "unsupported property: reviewgraphen.static_fact_verifier@1 supports only payment.at_most_once",
            ),
        };
        if self.outcome != expected.0
            || self.observation != expected.1
            || self.limitations != BTreeSet::from([expected.2.to_owned()])
        {
            return Err(validation("static result is not the exact closed result"));
        }
        let _ = bounded_bytes(self, "static result")?;
        Ok(())
    }
    pub const fn outcome(&self) -> VerificationOutcomeV3 {
        self.outcome
    }
    pub const fn applicability(&self) -> StaticApplicabilityV1 {
        self.applicability
    }
    pub const fn observation(&self) -> Option<EvidenceObservationV3> {
        self.observation
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "static result")
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
}

fn static_result_parts(
    applicability: StaticApplicabilityV1,
) -> (
    VerificationOutcomeV3,
    Option<EvidenceObservationV3>,
    &'static str,
) {
    match applicability {
        StaticApplicabilityV1::Absent => (
            VerificationOutcomeV3::Inconclusive,
            None,
            "required declared invariant is absent",
        ),
        StaticApplicabilityV1::Unique => (
            VerificationOutcomeV3::Inconclusive,
            Some(EvidenceObservationV3::FactPresent),
            "declared invariant is not an observed violation",
        ),
        StaticApplicabilityV1::Ambiguous => (
            VerificationOutcomeV3::Inconclusive,
            None,
            "multiple applicable declared invariants",
        ),
        StaticApplicabilityV1::UnsupportedProperty => (
            VerificationOutcomeV3::Unsupported,
            None,
            "unsupported property: reviewgraphen.static_fact_verifier@1 supports only payment.at_most_once",
        ),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StaticFactResultWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    claim_id: BoundedStableId,
    applicability: StaticApplicabilityWire,
    outcome: VerificationOutcomeWire,
    observation: Option<EvidenceObservationWire>,
    limitations: BoundedSeq<BoundedString<MAX_LIMITATION_BYTES>, MAX_LIMITATIONS>,
}

impl StaticFactResultV1 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let raw: StaticFactResultWire = decode_m4_wire(input, "static fact result JSON")?;
        let value = Self {
            schema: raw.schema.0,
            claim_id: raw.claim_id.0,
            applicability: raw.applicability.into(),
            outcome: raw.outcome.into(),
            observation: raw.observation.map(Into::into),
            limitations: strict_bounded_strings(raw.limitations.0, "static limitations")?,
        };
        value.validate()?;
        Ok(value)
    }
}

pub(crate) fn static_input_structural_upper_bound_bytes() -> Result<u64> {
    let mut total = u64::try_from(size_of::<StaticFactInputV1>()).unwrap_or(u64::MAX);
    let set_entry =
        size_of::<StableId>()
            .checked_add(MAX_TRACE_BYTES)
            .ok_or(M4Error::Incomplete {
                operation: "static input structural bound",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
    checked_memory_add(
        &mut total,
        6_usize
            .checked_mul(MAX_EVIDENCE_SUBJECTS)
            .and_then(|value| value.checked_mul(set_entry))
            .ok_or(M4Error::Incomplete {
                operation: "static input structural bound",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?,
    )?;
    checked_memory_add(
        &mut total,
        4_usize
            .checked_mul(MAX_TRACE_BYTES)
            .and_then(|value| value.checked_add(2 * MAX_TRACE_BYTES))
            .ok_or(M4Error::Incomplete {
                operation: "static input structural bound",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?,
    )?;
    Ok(total)
}

pub(crate) fn static_result_structural_upper_bound_bytes() -> Result<u64> {
    let mut total = u64::try_from(size_of::<StaticFactResultV1>()).unwrap_or(u64::MAX);
    checked_memory_add(
        &mut total,
        MAX_TRACE_BYTES
            .checked_mul(2)
            .and_then(|value| value.checked_add(size_of::<String>()))
            .and_then(|value| value.checked_add(MAX_LIMITATION_BYTES))
            .ok_or(M4Error::Incomplete {
                operation: "static result structural bound",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?,
    )?;
    Ok(total)
}

pub(crate) fn static_reconstruction_scratch_bytes(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> Result<u64> {
    let mut total = 0_u64;
    // closure and relevance coexist. Count every source occurrence so overlap
    // can only make this conservative; each cloned BTreeSet value owns both
    // the inline StableId and its backing String.
    for values in [
        obligation.normalized_target_refs(),
        obligation.normalized_context_ids(),
        obligation.normalized_source_ids(),
        claim.source_ids(),
        claim.target_refs(),
        obligation.normalized_context_ids(),
        claim.source_ids(),
    ] {
        total = total
            .checked_add(cloned_ids_allocated_bytes(values)?)
            .ok_or(M4Error::Incomplete {
                operation: "static reconstruction scratch",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
    }
    let candidate_ids =
        cloned_ids_allocated_bytes(program.invariants().iter().map(|value| &value.id))?;
    total = total
        .checked_add(candidate_ids)
        .ok_or(M4Error::Incomplete {
            operation: "static reconstruction scratch",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    let mut selected_scope = 0_u64;
    for invariant in program.invariants() {
        selected_scope = selected_scope.max(cloned_ids_allocated_bytes(&invariant.scope_ids)?);
    }
    total = total
        .checked_add(selected_scope)
        .ok_or(M4Error::Incomplete {
            operation: "static reconstruction scratch",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    let selected_id = program
        .invariants()
        .iter()
        .map(|value| size_of::<StableId>() + value.id.allocated_bytes())
        .max()
        .unwrap_or(0);
    checked_memory_add(&mut total, selected_id)?;

    // The comparison DTO owns another copy of the immutable input closure.
    checked_memory_add(&mut total, size_of::<StaticFactInputV1>())?;
    for values in [
        obligation.normalized_target_refs(),
        obligation.normalized_context_ids(),
        obligation.normalized_source_ids(),
        claim.source_ids(),
    ] {
        total = total
            .checked_add(cloned_ids_allocated_bytes(values)?)
            .ok_or(M4Error::Incomplete {
                operation: "static reconstruction comparison input",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
    }
    total = total
        .checked_add(candidate_ids)
        .and_then(|value| value.checked_add(selected_scope))
        .ok_or(M4Error::Incomplete {
            operation: "static reconstruction comparison input",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    for bytes in [
        claim.id().allocated_bytes(),
        program.snapshot_id().allocated_bytes(),
        claim.property_id().len(),
        obligation.id().allocated_bytes(),
        selected_id,
        "reviewgraphen.static_fact_input.v1".len(),
    ] {
        checked_memory_add(&mut total, bytes)?;
    }
    total = total
        .checked_add(static_result_structural_upper_bound_bytes()?)
        .and_then(|value| value.checked_add(MAX_RECORD_BYTES as u64))
        .ok_or(M4Error::Incomplete {
            operation: "static reconstruction result/scratch",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    Ok(total)
}

#[cfg(test)]
pub(crate) fn static_evaluation_test_oracle_bytes(value: &StaticFactEvaluationV1) -> u64 {
    fn ids(values: &BTreeSet<StableId>) -> u64 {
        values
            .iter()
            .map(|id| (size_of::<StableId>() + id.allocated_bytes()) as u64)
            .sum()
    }
    let input = &value.input;
    let mut total = size_of::<StaticFactEvaluationV1>() as u64;
    for values in [
        &input.candidate_invariant_ids,
        &input.claim_source_ids,
        &input.obligation_context_ids,
        &input.obligation_source_ids,
        &input.obligation_target_refs,
        &input.selected_invariant_scope_ids,
        &value.scope.candidate_invariant_ids,
        &value.scope.source_ids,
        &value.scope.target_refs,
    ] {
        total += ids(values);
    }
    total += [
        input.claim_id.allocated_bytes(),
        input.obligation_id.allocated_bytes(),
        input.property_id.capacity(),
        input.schema.capacity(),
        input
            .selected_invariant_id
            .as_ref()
            .map_or(0, StableId::allocated_bytes),
        input.snapshot_id.allocated_bytes(),
        value.result.claim_id.allocated_bytes(),
        value.result.schema.capacity(),
        value.scope.claim_body_hash.allocated_bytes(),
        value.scope.claim_id.allocated_bytes(),
        value.scope.obligation_id.allocated_bytes(),
        value.scope.property_id.capacity(),
        value
            .scope
            .selected_invariant_id
            .as_ref()
            .map_or(0, StableId::allocated_bytes),
        value.scope.snapshot_id.allocated_bytes(),
    ]
    .into_iter()
    .map(|bytes| bytes as u64)
    .sum::<u64>();
    total += value
        .result
        .limitations
        .iter()
        .map(|text| (size_of::<String>() + text.capacity()) as u64)
        .sum::<u64>();
    total += value.evidence_subject_ids.as_ref().map_or(0, ids);
    total
}

#[cfg(test)]
pub(crate) fn static_reconstruction_test_oracle_bytes(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
) -> u64 {
    fn ids<'a>(values: impl IntoIterator<Item = &'a StableId>) -> u64 {
        values
            .into_iter()
            .map(|id| (size_of::<StableId>() + id.allocated_bytes()) as u64)
            .sum()
    }
    let mut total = 0_u64;
    for values in [
        obligation.normalized_target_refs(),
        obligation.normalized_context_ids(),
        obligation.normalized_source_ids(),
        claim.source_ids(),
        claim.target_refs(),
        obligation.normalized_context_ids(),
        claim.source_ids(),
    ] {
        total += ids(values);
    }
    let candidates = ids(program.invariants().iter().map(|value| &value.id));
    let selected_scope = program
        .invariants()
        .iter()
        .map(|value| ids(&value.scope_ids))
        .max()
        .unwrap_or(0);
    let selected_id = program
        .invariants()
        .iter()
        .map(|value| (size_of::<StableId>() + value.id.allocated_bytes()) as u64)
        .max()
        .unwrap_or(0);
    total += candidates + selected_scope + selected_id + size_of::<StaticFactInputV1>() as u64;
    for values in [
        obligation.normalized_target_refs(),
        obligation.normalized_context_ids(),
        obligation.normalized_source_ids(),
        claim.source_ids(),
    ] {
        total += ids(values);
    }
    total += candidates
        + selected_scope
        + claim.id().allocated_bytes() as u64
        + program.snapshot_id().allocated_bytes() as u64
        + claim.property_id().len() as u64
        + obligation.id().allocated_bytes() as u64
        + selected_id
        + "reviewgraphen.static_fact_input.v1".len() as u64
        + size_of::<StaticFactResultV1>() as u64
        + (2 * MAX_TRACE_BYTES + size_of::<String>() + MAX_LIMITATION_BYTES) as u64
        + MAX_RECORD_BYTES as u64;
    total
}

pub(crate) fn reconstruct_static_result_from_durable_input_v1(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
    input: &StaticFactInputV1,
) -> Result<StaticFactResultV1> {
    claim.validate_shape()?;
    if obligation.version().snapshot() != program.snapshot_id() {
        return Err(M4Error::SnapshotMismatch {
            expected: program.snapshot_id().clone(),
            actual: obligation.version().snapshot().clone(),
        });
    }
    if claim.obligation_ids().len() != 1 || !claim.obligation_ids().contains(obligation.id()) {
        return Err(M4Error::ClaimMismatch {
            expected: obligation.id().clone(),
            actual: claim.id().clone(),
        });
    }
    let supported = claim.property_id() == M4_PROPERTY_ID
        && obligation.property_id() == M4_PROPERTY_ID
        && claim.property_id() == obligation.property_id();
    mark_static_reconstruction_allocation_seam();
    let mut closure = obligation.normalized_target_refs().clone();
    closure.extend(obligation.normalized_context_ids().iter().cloned());
    closure.extend(obligation.normalized_source_ids().iter().cloned());
    closure.extend(claim.source_ids().iter().cloned());
    let relevance = claim
        .target_refs()
        .iter()
        .chain(obligation.normalized_context_ids())
        .chain(claim.source_ids())
        .cloned()
        .collect::<BTreeSet<_>>();
    let candidates = if supported {
        program
            .invariants()
            .iter()
            .filter(|invariant| {
                invariant.property_id == claim.property_id()
                    && !invariant.scope_ids.is_empty()
                    && invariant.scope_ids.is_subset(&closure)
                    && !invariant.scope_ids.is_disjoint(&relevance)
            })
            .map(|invariant| invariant.id.clone())
            .collect::<BTreeSet<_>>()
    } else {
        BTreeSet::new()
    };
    let selected_invariant_id = (candidates.len() == 1)
        .then(|| candidates.first().cloned())
        .flatten();
    let selected_invariant_scope_ids = selected_invariant_id
        .as_ref()
        .and_then(|id| program.invariant(id))
        .map_or_else(BTreeSet::new, |invariant| invariant.scope_ids.clone());
    let expected = StaticFactInputV1 {
        schema: "reviewgraphen.static_fact_input.v1".to_owned(),
        claim_id: claim.id().clone(),
        snapshot_id: program.snapshot_id().clone(),
        property_id: claim.property_id().to_owned(),
        obligation_id: obligation.id().clone(),
        obligation_target_refs: obligation.normalized_target_refs().clone(),
        obligation_context_ids: obligation.normalized_context_ids().clone(),
        obligation_source_ids: obligation.normalized_source_ids().clone(),
        claim_source_ids: claim.source_ids().clone(),
        candidate_invariant_ids: candidates,
        selected_invariant_id,
        selected_invariant_scope_ids,
    };
    expected.validate()?;
    if input != &expected {
        return Err(validation(
            "durable static input does not match the immutable fact closure",
        ));
    }
    StaticFactResultV1::from_applicability(claim.id().clone(), input.reconstructed_applicability())
}

pub(crate) fn materialize_static_from_durable_input_v1(
    program: &ProgramSpace,
    obligation: &Obligation,
    claim: &ExecutionClaimV2,
    input: StaticFactInputV1,
    input_registration_id: StableId,
    output_registration_id: StableId,
) -> Result<StaticRecordProposalV1> {
    let result =
        reconstruct_static_result_from_durable_input_v1(program, obligation, claim, &input)?;
    let evidence_subject_ids = input.selected_invariant_id.as_ref().map(|id| {
        let mut subjects = claim.target_refs().clone();
        subjects.insert(id.clone());
        subjects
    });
    let scope = StaticScopeV3 {
        snapshot_id: program.snapshot_id().clone(),
        claim_id: claim.id().clone(),
        claim_body_hash: claim.body_hash()?,
        property_id: claim.property_id().to_owned(),
        obligation_id: obligation.id().clone(),
        target_refs: claim.target_refs().clone(),
        source_ids: claim.source_ids().clone(),
        candidate_invariant_ids: input.candidate_invariant_ids.clone(),
        selected_invariant_id: input.selected_invariant_id.clone(),
        descriptor: VerifierDescriptorV3::StaticFactV1,
        procedure: VerifierProcedureV3::StaticProjectionV1,
    };
    StaticFactEvaluationV1 {
        scope,
        input,
        result,
        evidence_subject_ids,
    }
    .materialize(input_registration_id, output_registration_id)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FixedFixtureResultV1 {
    claim_id: StableId,
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorV3,
    outcome: VerificationOutcomeV3,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureV3,
    property_id: String,
    schema: String,
    subject_ids: Vec<StableId>,
    witness_hash: ContentHash,
}

impl FixedFixtureResultV1 {
    #[allow(dead_code)]
    pub(crate) fn new_admitted(
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
    ) -> Result<Self> {
        admission.validate(scope)?;
        scope.validate()?;
        if scope.property_id != M4_PROPERTY_ID || scope.polarity != ClaimPolarity::IssuePresent {
            return Err(M4Error::UnsupportedProperty {
                property_id: scope.property_id.clone(),
            });
        }
        let mut subject_ids = scope.target_refs.clone();
        subject_ids.push(StableId::parse("test:double-submit")?);
        subject_ids.sort_unstable();
        let value = Self {
            schema: "reviewgraphen.fixture_test_result.v1".to_owned(),
            claim_id: scope.claim_id.clone(),
            property_id: scope.property_id.clone(),
            descriptor: VerifierDescriptorV3::FixedFixtureV1,
            procedure: VerifierProcedureV3::DuplicateSubmitV1,
            witness_hash: ContentHash::parse(FIXTURE_WITNESS_HASH)?,
            subject_ids,
            outcome: VerificationOutcomeV3::Passed,
        };
        value.validate()?;
        Ok(value)
    }
    #[cfg(test)]
    fn new(scope: &AuthorityScopeV3) -> Result<Self> {
        Self::new_admitted(scope, &LockedVerificationProofV3::for_test(scope)?)
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.fixture_test_result.v1"
            || self.property_id != M4_PROPERTY_ID
            || self.descriptor != VerifierDescriptorV3::FixedFixtureV1
            || self.procedure != VerifierProcedureV3::DuplicateSubmitV1
            || self.outcome != VerificationOutcomeV3::Passed
            || self.witness_hash.as_str() != FIXTURE_WITNESS_HASH
        {
            return Err(validation("invalid fixed fixture result"));
        }
        require_kind(&self.claim_id, "claim", "fixture result claim")?;
        require_count(
            self.subject_ids.len(),
            1,
            MAX_EVIDENCE_SUBJECTS,
            "fixture subjects",
        )?;
        let _ = bounded_bytes(self, "fixture result")?;
        Ok(())
    }
    pub fn subject_ids(&self) -> &[StableId] {
        &self.subject_ids
    }
    pub(crate) fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub(crate) fn property_id(&self) -> &str {
        &self.property_id
    }
    pub(crate) const fn descriptor(&self) -> VerifierDescriptorV3 {
        self.descriptor
    }
    pub(crate) const fn procedure(&self) -> VerifierProcedureV3 {
        self.procedure
    }
    pub(crate) const fn outcome(&self) -> VerificationOutcomeV3 {
        self.outcome
    }
    pub fn witness_hash(&self) -> &ContentHash {
        &self.witness_hash
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "fixture result")
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixedFixtureResultWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    claim_id: BoundedStableId,
    property_id: BoundedString<MAX_TRACE_BYTES>,
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorWire,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureWire,
    witness_hash: BoundedContentHash,
    subject_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    outcome: VerificationOutcomeWire,
}

impl FixedFixtureResultV1 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let raw: FixedFixtureResultWire = decode_m4_wire(input, "fixed fixture result JSON")?;
        let value = Self {
            schema: raw.schema.0,
            claim_id: raw.claim_id.0,
            property_id: raw.property_id.0,
            descriptor: raw.descriptor.into(),
            procedure: raw.procedure.into(),
            witness_hash: raw.witness_hash.0,
            subject_ids: strict_bounded_ids(raw.subject_ids.0, "fixture subjects")?
                .into_iter()
                .collect(),
            outcome: raw.outcome.into(),
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceV3 {
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorV3,
    id: StableId,
    input_registration_id: StableId,
    kind: EvidenceKindV3,
    observation: EvidenceObservationV3,
    output_registration_id: StableId,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureV3,
    schema: String,
    snapshot_id: StableId,
    subject_ids: Vec<StableId>,
}

#[derive(Serialize)]
struct EvidenceIdentity<'a> {
    descriptor_id: VerifierDescriptorV3,
    input_registration_id: &'a StableId,
    kind: EvidenceKindV3,
    observation: EvidenceObservationV3,
    output_registration_id: &'a StableId,
    procedure_version: VerifierProcedureV3,
    snapshot_id: &'a StableId,
    subject_ids: &'a Vec<StableId>,
}

impl EvidenceV3 {
    fn build_static(
        scope: &StaticScopeV3,
        subject_ids: BTreeSet<StableId>,
        input_registration_id: StableId,
        output_registration_id: StableId,
    ) -> Result<Self> {
        if scope.descriptor != VerifierDescriptorV3::StaticFactV1
            || scope.procedure != VerifierProcedureV3::StaticProjectionV1
            || scope.selected_invariant_id.is_none()
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let subject_ids = subject_ids.into_iter().collect::<Vec<_>>();
        let identity = EvidenceIdentity {
            descriptor_id: scope.descriptor,
            input_registration_id: &input_registration_id,
            kind: EvidenceKindV3::StaticFact,
            observation: EvidenceObservationV3::FactPresent,
            output_registration_id: &output_registration_id,
            procedure_version: scope.procedure,
            snapshot_id: &scope.snapshot_id,
            subject_ids: &subject_ids,
        };
        let value = Self {
            schema: "reviewgraphen.evidence.v3".to_owned(),
            id: derived_id("evidence", &identity)?,
            kind: EvidenceKindV3::StaticFact,
            snapshot_id: scope.snapshot_id.clone(),
            subject_ids,
            descriptor: scope.descriptor,
            procedure: scope.procedure,
            input_registration_id,
            output_registration_id,
            observation: EvidenceObservationV3::FactPresent,
        };
        value.validate()?;
        Ok(value)
    }
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn new_admitted(
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
        kind: EvidenceKindV3,
        subject_ids: BTreeSet<StableId>,
        descriptor: VerifierDescriptorV3,
        input_registration_id: StableId,
        output_registration_id: StableId,
        observation: EvidenceObservationV3,
    ) -> Result<Self> {
        admission.validate(scope)?;
        scope.validate()?;
        if descriptor != scope.descriptor || descriptor.procedure() != scope.procedure {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        require_kind(
            &input_registration_id,
            "registration",
            "evidence input registration",
        )?;
        require_kind(
            &output_registration_id,
            "registration",
            "evidence output registration",
        )?;
        let procedure = descriptor.procedure();
        let subject_ids = subject_ids.into_iter().collect::<Vec<_>>();
        let identity = EvidenceIdentity {
            descriptor_id: descriptor,
            input_registration_id: &input_registration_id,
            kind,
            observation,
            output_registration_id: &output_registration_id,
            procedure_version: procedure,
            snapshot_id: &scope.snapshot_id,
            subject_ids: &subject_ids,
        };
        let value = Self {
            schema: "reviewgraphen.evidence.v3".to_owned(),
            id: derived_id("evidence", &identity)?,
            kind,
            snapshot_id: scope.snapshot_id.clone(),
            subject_ids,
            descriptor,
            procedure,
            input_registration_id,
            output_registration_id,
            observation,
        };
        value.validate()?;
        Ok(value)
    }
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn new(
        scope: &AuthorityScopeV3,
        kind: EvidenceKindV3,
        subject_ids: BTreeSet<StableId>,
        descriptor: VerifierDescriptorV3,
        input_registration_id: StableId,
        output_registration_id: StableId,
        observation: EvidenceObservationV3,
    ) -> Result<Self> {
        Self::new_admitted(
            scope,
            &LockedVerificationProofV3::for_test(scope)?,
            kind,
            subject_ids,
            descriptor,
            input_registration_id,
            output_registration_id,
            observation,
        )
    }
    fn identity(&self) -> EvidenceIdentity<'_> {
        EvidenceIdentity {
            descriptor_id: self.descriptor,
            input_registration_id: &self.input_registration_id,
            kind: self.kind,
            observation: self.observation,
            output_registration_id: &self.output_registration_id,
            procedure_version: self.procedure,
            snapshot_id: &self.snapshot_id,
            subject_ids: &self.subject_ids,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.evidence.v3"
            || self.id != derived_id("evidence", &self.identity())?
        {
            return Err(validation("invalid EvidenceV3 identity"));
        }
        require_count(
            self.subject_ids.len(),
            1,
            MAX_EVIDENCE_SUBJECTS,
            "evidence subjects",
        )?;
        require_kind(&self.snapshot_id, "snapshot", "evidence snapshot")?;
        require_kind(
            &self.input_registration_id,
            "registration",
            "evidence input registration",
        )?;
        require_kind(
            &self.output_registration_id,
            "registration",
            "evidence output registration",
        )?;
        let exact = matches!(
            (self.kind, self.observation, self.descriptor, self.procedure),
            (
                EvidenceKindV3::StaticFact,
                EvidenceObservationV3::FactPresent,
                VerifierDescriptorV3::StaticFactV1,
                VerifierProcedureV3::StaticProjectionV1
            ) | (
                EvidenceKindV3::TestWitness,
                EvidenceObservationV3::Witnessed,
                VerifierDescriptorV3::FixedFixtureV1,
                VerifierProcedureV3::DuplicateSubmitV1
            )
        );
        if !exact {
            return Err(validation(
                "evidence kind/observation/descriptor/procedure splice",
            ));
        }
        if self.descriptor == VerifierDescriptorV3::FixedFixtureV1 {
            let Some(test_id) = self.subject_ids.iter().find(|id| id.kind() == "test") else {
                return Err(M4Error::DanglingReference {
                    owner: "fixture evidence",
                    reference: StableId::parse("test:double-submit")?,
                });
            };
            if test_id.as_str() != "test:double-submit"
                || self
                    .subject_ids
                    .iter()
                    .filter(|id| id.kind() == "test")
                    .count()
                    != 1
            {
                return Err(M4Error::InvalidRecord {
                    reason: "fixture evidence requires exact test:double-submit subject".to_owned(),
                });
            }
        }
        let _ = bounded_bytes(self, "EvidenceV3")?;
        Ok(())
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.subject_ids)?;
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.snapshot_id.allocated_bytes(),
            self.input_registration_id.allocated_bytes(),
            self.output_registration_id.allocated_bytes(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn subject_ids(&self) -> &[StableId] {
        &self.subject_ids
    }
    pub const fn descriptor(&self) -> VerifierDescriptorV3 {
        self.descriptor
    }
    pub const fn procedure(&self) -> VerifierProcedureV3 {
        self.procedure
    }
    pub fn input_registration_id(&self) -> &StableId {
        &self.input_registration_id
    }
    pub fn output_registration_id(&self) -> &StableId {
        &self.output_registration_id
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "EvidenceV3")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    id: BoundedStableId,
    kind: EvidenceKindWire,
    snapshot_id: BoundedStableId,
    subject_ids: BoundedSeq<BoundedStableId, MAX_EVIDENCE_SUBJECTS>,
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorWire,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureWire,
    input_registration_id: BoundedStableId,
    output_registration_id: BoundedStableId,
    observation: EvidenceObservationWire,
}
impl EvidenceV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let r: EvidenceWire = decode_m4_wire(input, "evidence JSON")?;
        let v = Self {
            schema: r.schema.0,
            id: r.id.0,
            kind: r.kind.into(),
            snapshot_id: r.snapshot_id.0,
            subject_ids: strict_bounded_ids(r.subject_ids.0, "evidence subjects")?
                .into_iter()
                .collect(),
            descriptor: r.descriptor.into(),
            procedure: r.procedure.into(),
            input_registration_id: r.input_registration_id.0,
            output_registration_id: r.output_registration_id.0,
            observation: r.observation.into(),
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EvidenceBindingV3 {
    claim_id: StableId,
    evidence_id: StableId,
    id: StableId,
    property_id: String,
    relation: EvidenceRelationV3,
    schema: String,
}
#[derive(Serialize)]
struct BindingIdentity<'a> {
    claim_id: &'a StableId,
    evidence_id: &'a StableId,
    property_id: &'a str,
    relation: EvidenceRelationV3,
}
impl EvidenceBindingV3 {
    fn build_static(scope: &StaticScopeV3, evidence: &EvidenceV3) -> Result<Self> {
        let identity = BindingIdentity {
            claim_id: &scope.claim_id,
            evidence_id: &evidence.id,
            property_id: &scope.property_id,
            relation: EvidenceRelationV3::Qualifies,
        };
        let value = Self {
            schema: "reviewgraphen.evidence_binding.v3".to_owned(),
            id: derived_id("binding", &identity)?,
            claim_id: scope.claim_id.clone(),
            evidence_id: evidence.id.clone(),
            relation: EvidenceRelationV3::Qualifies,
            property_id: scope.property_id.clone(),
        };
        value.validate()?;
        Ok(value)
    }
    #[allow(dead_code)]
    pub(crate) fn new_admitted(
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
        evidence: &EvidenceV3,
        relation: EvidenceRelationV3,
    ) -> Result<Self> {
        admission.validate(scope)?;
        scope.validate()?;
        if evidence.snapshot_id != scope.snapshot_id {
            return Err(M4Error::SnapshotMismatch {
                expected: scope.snapshot_id.clone(),
                actual: evidence.snapshot_id.clone(),
            });
        }
        if !evidence
            .subject_ids
            .iter()
            .any(|id| scope.target_refs.contains(id) || scope.source_ids.contains(id))
        {
            return Err(validation("evidence does not intersect claim scope"));
        }
        if relation == EvidenceRelationV3::Reproduces
            && evidence.descriptor != VerifierDescriptorV3::FixedFixtureV1
        {
            return Err(validation("only fixed fixture evidence reproduces in M4"));
        }
        if relation == EvidenceRelationV3::Qualifies
            && evidence.descriptor != VerifierDescriptorV3::StaticFactV1
        {
            return Err(validation("only static evidence qualifies in M4"));
        }
        let identity = BindingIdentity {
            claim_id: &scope.claim_id,
            evidence_id: &evidence.id,
            property_id: &scope.property_id,
            relation,
        };
        let v = Self {
            schema: "reviewgraphen.evidence_binding.v3".to_owned(),
            id: derived_id("binding", &identity)?,
            claim_id: scope.claim_id.clone(),
            evidence_id: evidence.id.clone(),
            relation,
            property_id: scope.property_id.clone(),
        };
        v.validate()?;
        Ok(v)
    }
    #[cfg(test)]
    fn new(
        scope: &AuthorityScopeV3,
        evidence: &EvidenceV3,
        relation: EvidenceRelationV3,
    ) -> Result<Self> {
        Self::new_admitted(
            scope,
            &LockedVerificationProofV3::for_test(scope)?,
            evidence,
            relation,
        )
    }
    fn identity(&self) -> BindingIdentity<'_> {
        BindingIdentity {
            claim_id: &self.claim_id,
            evidence_id: &self.evidence_id,
            property_id: &self.property_id,
            relation: self.relation,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.evidence_binding.v3"
            || self.id != derived_id("binding", &self.identity())?
            || self.property_id != M4_PROPERTY_ID
        {
            return Err(validation("invalid EvidenceBindingV3"));
        }
        require_kind(&self.claim_id, "claim", "binding claim")?;
        require_kind(&self.evidence_id, "evidence", "binding evidence")?;
        let _ = bounded_bytes(self, "EvidenceBindingV3")?;
        Ok(())
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = 0;
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.evidence_id.allocated_bytes(),
            self.property_id.capacity(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn evidence_id(&self) -> &StableId {
        &self.evidence_id
    }
    pub const fn relation(&self) -> EvidenceRelationV3 {
        self.relation
    }
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "EvidenceBindingV3")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    id: BoundedStableId,
    claim_id: BoundedStableId,
    evidence_id: BoundedStableId,
    relation: EvidenceRelationWire,
    property_id: BoundedString<MAX_TRACE_BYTES>,
}
impl EvidenceBindingV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let r: BindingWire = decode_m4_wire(input, "evidence binding JSON")?;
        let v = Self {
            schema: r.schema.0,
            id: r.id.0,
            claim_id: r.claim_id.0,
            evidence_id: r.evidence_id.0,
            relation: r.relation.into(),
            property_id: r.property_id.0,
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerificationV3 {
    claim_id: StableId,
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorV3,
    evidence_ids: Vec<StableId>,
    id: StableId,
    input_registration_id: StableId,
    limitations: Vec<String>,
    outcome: VerificationOutcomeV3,
    output_registration_id: StableId,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureV3,
    schema: String,
}
#[derive(Serialize)]
struct VerificationIdentity<'a> {
    claim_id: &'a StableId,
    descriptor_id: VerifierDescriptorV3,
    evidence_ids: &'a Vec<StableId>,
    input_registration_id: &'a StableId,
    limitations: &'a Vec<String>,
    outcome: VerificationOutcomeV3,
    output_registration_id: &'a StableId,
    procedure_version: VerifierProcedureV3,
}
impl VerificationV3 {
    fn build_static(
        scope: &StaticScopeV3,
        input_registration_id: StableId,
        output_registration_id: StableId,
        evidence_ids: BTreeSet<StableId>,
        outcome: VerificationOutcomeV3,
        limitations: BTreeSet<String>,
    ) -> Result<Self> {
        if scope.descriptor != VerifierDescriptorV3::StaticFactV1
            || scope.procedure != VerifierProcedureV3::StaticProjectionV1
            || outcome == VerificationOutcomeV3::Passed
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let evidence_ids = evidence_ids.into_iter().collect::<Vec<_>>();
        let limitations = limitations.into_iter().collect::<Vec<_>>();
        let identity = VerificationIdentity {
            claim_id: &scope.claim_id,
            descriptor_id: scope.descriptor,
            evidence_ids: &evidence_ids,
            input_registration_id: &input_registration_id,
            limitations: &limitations,
            outcome,
            output_registration_id: &output_registration_id,
            procedure_version: scope.procedure,
        };
        let value = Self {
            schema: "reviewgraphen.verification.v3".to_owned(),
            id: derived_id("verification", &identity)?,
            claim_id: scope.claim_id.clone(),
            descriptor: scope.descriptor,
            procedure: scope.procedure,
            input_registration_id,
            output_registration_id,
            evidence_ids,
            outcome,
            limitations,
        };
        value.validate()?;
        Ok(value)
    }
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn new_admitted(
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
        descriptor: VerifierDescriptorV3,
        input_registration_id: StableId,
        output_registration_id: StableId,
        evidence_ids: BTreeSet<StableId>,
        outcome: VerificationOutcomeV3,
        limitations: BTreeSet<String>,
    ) -> Result<Self> {
        admission.validate(scope)?;
        scope.validate()?;
        if descriptor != scope.descriptor || descriptor.procedure() != scope.procedure {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let procedure = descriptor.procedure();
        let evidence_ids = evidence_ids.into_iter().collect::<Vec<_>>();
        let limitations = limitations.into_iter().collect::<Vec<_>>();
        let identity = VerificationIdentity {
            claim_id: &scope.claim_id,
            descriptor_id: descriptor,
            evidence_ids: &evidence_ids,
            input_registration_id: &input_registration_id,
            limitations: &limitations,
            outcome,
            output_registration_id: &output_registration_id,
            procedure_version: procedure,
        };
        let v = Self {
            schema: "reviewgraphen.verification.v3".to_owned(),
            id: derived_id("verification", &identity)?,
            claim_id: scope.claim_id.clone(),
            descriptor,
            procedure,
            input_registration_id,
            output_registration_id,
            evidence_ids,
            outcome,
            limitations,
        };
        v.validate()?;
        Ok(v)
    }
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn new(
        scope: &AuthorityScopeV3,
        descriptor: VerifierDescriptorV3,
        input_registration_id: StableId,
        output_registration_id: StableId,
        evidence_ids: BTreeSet<StableId>,
        outcome: VerificationOutcomeV3,
        limitations: BTreeSet<String>,
    ) -> Result<Self> {
        Self::new_admitted(
            scope,
            &LockedVerificationProofV3::for_test(scope)?,
            descriptor,
            input_registration_id,
            output_registration_id,
            evidence_ids,
            outcome,
            limitations,
        )
    }
    fn identity(&self) -> VerificationIdentity<'_> {
        VerificationIdentity {
            claim_id: &self.claim_id,
            descriptor_id: self.descriptor,
            evidence_ids: &self.evidence_ids,
            input_registration_id: &self.input_registration_id,
            limitations: &self.limitations,
            outcome: self.outcome,
            output_registration_id: &self.output_registration_id,
            procedure_version: self.procedure,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.verification.v3"
            || self.id != derived_id("verification", &self.identity())?
        {
            return Err(validation("invalid VerificationV3 identity"));
        }
        require_kind(&self.claim_id, "claim", "verification claim")?;
        require_kind(
            &self.input_registration_id,
            "registration",
            "verification input",
        )?;
        require_kind(
            &self.output_registration_id,
            "registration",
            "verification output",
        )?;
        require_count(
            self.evidence_ids.len(),
            usize::from(self.outcome == VerificationOutcomeV3::Passed),
            MAX_VERIFICATION_EVIDENCE,
            "verification evidence",
        )?;
        require_count(
            self.limitations.len(),
            usize::from(self.outcome != VerificationOutcomeV3::Passed),
            MAX_LIMITATIONS,
            "verification limitations",
        )?;
        for x in &self.limitations {
            require_text(x, MAX_LIMITATION_BYTES, "M4 limitation")?;
        }
        if self.outcome == VerificationOutcomeV3::Passed
            && (self.descriptor != VerifierDescriptorV3::FixedFixtureV1
                || !self.limitations.is_empty())
        {
            return Err(validation(
                "Passed requires fixed fixture evidence and no limitations",
            ));
        }
        if self.descriptor == VerifierDescriptorV3::StaticFactV1
            && self.outcome == VerificationOutcomeV3::Passed
        {
            return Err(validation("static verifier cannot pass"));
        }
        if self.descriptor == VerifierDescriptorV3::FixedFixtureV1
            && self.outcome != VerificationOutcomeV3::Passed
        {
            return Err(validation(
                "fixed fixture verifier has only the exact Passed result",
            ));
        }
        if self.outcome == VerificationOutcomeV3::Unsupported && !self.evidence_ids.is_empty() {
            return Err(validation("Unsupported verification cannot cite evidence"));
        }
        if self.descriptor == VerifierDescriptorV3::StaticFactV1 {
            let exact = [
                "required declared invariant is absent",
                "declared invariant is not an observed violation",
                "multiple applicable declared invariants",
                "unsupported property: reviewgraphen.static_fact_verifier@1 supports only payment.at_most_once",
            ];
            if self.limitations.len() != 1
                || !self
                    .limitations
                    .iter()
                    .all(|item| exact.contains(&item.as_str()))
            {
                return Err(validation("static verification limitation is not exact"));
            }
        }
        let _ = bounded_bytes(self, "VerificationV3")?;
        Ok(())
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.evidence_ids)?
            .checked_add(string_vec_allocated_bytes(&self.limitations)?)
            .ok_or(M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.input_registration_id.allocated_bytes(),
            self.output_registration_id.allocated_bytes(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn evidence_ids(&self) -> &[StableId] {
        &self.evidence_ids
    }
    pub const fn outcome(&self) -> VerificationOutcomeV3 {
        self.outcome
    }
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub const fn descriptor(&self) -> VerifierDescriptorV3 {
        self.descriptor
    }
    pub const fn procedure(&self) -> VerifierProcedureV3 {
        self.procedure
    }
    pub fn input_registration_id(&self) -> &StableId {
        &self.input_registration_id
    }
    pub fn output_registration_id(&self) -> &StableId {
        &self.output_registration_id
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "VerificationV3")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    id: BoundedStableId,
    claim_id: BoundedStableId,
    #[serde(rename = "descriptor_id")]
    descriptor: VerifierDescriptorWire,
    #[serde(rename = "procedure_version")]
    procedure: VerifierProcedureWire,
    input_registration_id: BoundedStableId,
    output_registration_id: BoundedStableId,
    evidence_ids: BoundedSeq<BoundedStableId, MAX_VERIFICATION_EVIDENCE>,
    outcome: VerificationOutcomeWire,
    limitations: BoundedSeq<BoundedString<MAX_LIMITATION_BYTES>, MAX_LIMITATIONS>,
}
impl VerificationV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let r: VerificationWire = decode_m4_wire(input, "verification JSON")?;
        let v = Self {
            schema: r.schema.0,
            id: r.id.0,
            claim_id: r.claim_id.0,
            descriptor: r.descriptor.into(),
            procedure: r.procedure.into(),
            input_registration_id: r.input_registration_id.0,
            output_registration_id: r.output_registration_id.0,
            evidence_ids: strict_bounded_ids(r.evidence_ids.0, "verification evidence")?
                .into_iter()
                .collect(),
            outcome: r.outcome.into(),
            limitations: strict_bounded_strings(r.limitations.0, "verification limitations")?
                .into_iter()
                .collect(),
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FixtureRecordProposalV1 {
    pub(crate) evidence: EvidenceV3,
    pub(crate) binding: EvidenceBindingV3,
    pub(crate) verification: VerificationV3,
}

pub(crate) fn materialize_fixture_replay_v1(
    scope: &ClaimAssessmentScopeV3,
    result: &FixedFixtureResultV1,
    input_registration_id: StableId,
    output_registration_id: StableId,
) -> Result<FixtureRecordProposalV1> {
    let mut subject_ids = scope.target_refs.clone();
    subject_ids.push(StableId::parse(FIXTURE_TEST_ARTIFACT_ID)?);
    subject_ids.sort_unstable();
    if scope.property_id != M4_PROPERTY_ID
        || scope.polarity != ClaimPolarity::IssuePresent
        || result.claim_id != scope.claim_id
        || result.property_id != scope.property_id
        || result.subject_ids != subject_ids
        || result.descriptor != VerifierDescriptorV3::FixedFixtureV1
        || result.procedure != VerifierProcedureV3::DuplicateSubmitV1
        || result.outcome != VerificationOutcomeV3::Passed
    {
        return Err(M4Error::AuthorityScopeMismatch);
    }
    let evidence_identity = EvidenceIdentity {
        descriptor_id: VerifierDescriptorV3::FixedFixtureV1,
        input_registration_id: &input_registration_id,
        kind: EvidenceKindV3::TestWitness,
        observation: EvidenceObservationV3::Witnessed,
        output_registration_id: &output_registration_id,
        procedure_version: VerifierProcedureV3::DuplicateSubmitV1,
        snapshot_id: &scope.snapshot_id,
        subject_ids: &subject_ids,
    };
    let evidence = EvidenceV3 {
        schema: "reviewgraphen.evidence.v3".to_owned(),
        id: derived_id("evidence", &evidence_identity)?,
        kind: EvidenceKindV3::TestWitness,
        snapshot_id: scope.snapshot_id.clone(),
        subject_ids,
        descriptor: VerifierDescriptorV3::FixedFixtureV1,
        procedure: VerifierProcedureV3::DuplicateSubmitV1,
        input_registration_id: input_registration_id.clone(),
        output_registration_id: output_registration_id.clone(),
        observation: EvidenceObservationV3::Witnessed,
    };
    evidence.validate()?;
    let binding_identity = BindingIdentity {
        claim_id: &scope.claim_id,
        evidence_id: &evidence.id,
        property_id: &scope.property_id,
        relation: EvidenceRelationV3::Reproduces,
    };
    let binding = EvidenceBindingV3 {
        schema: "reviewgraphen.evidence_binding.v3".to_owned(),
        id: derived_id("binding", &binding_identity)?,
        claim_id: scope.claim_id.clone(),
        evidence_id: evidence.id.clone(),
        relation: EvidenceRelationV3::Reproduces,
        property_id: scope.property_id.clone(),
    };
    binding.validate()?;
    let evidence_ids = vec![evidence.id.clone()];
    let limitations = Vec::new();
    let verification_identity = VerificationIdentity {
        claim_id: &scope.claim_id,
        descriptor_id: VerifierDescriptorV3::FixedFixtureV1,
        evidence_ids: &evidence_ids,
        input_registration_id: &input_registration_id,
        limitations: &limitations,
        outcome: VerificationOutcomeV3::Passed,
        output_registration_id: &output_registration_id,
        procedure_version: VerifierProcedureV3::DuplicateSubmitV1,
    };
    let verification = VerificationV3 {
        schema: "reviewgraphen.verification.v3".to_owned(),
        id: derived_id("verification", &verification_identity)?,
        claim_id: scope.claim_id.clone(),
        descriptor: VerifierDescriptorV3::FixedFixtureV1,
        procedure: VerifierProcedureV3::DuplicateSubmitV1,
        input_registration_id,
        output_registration_id,
        evidence_ids,
        outcome: VerificationOutcomeV3::Passed,
        limitations,
    };
    verification.validate()?;
    Ok(FixtureRecordProposalV1 {
        evidence,
        binding,
        verification,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcomeV3 {
    Accept,
    Reject,
    Defer,
    Exception,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DecisionV3 {
    actor: String,
    authority_id: String,
    claim_id: StableId,
    expires_at: Option<String>,
    id: StableId,
    issued_at: String,
    outcome: DecisionOutcomeV3,
    policy_revision_hash: ContentHash,
    property_id: String,
    rationale: String,
    run_id: StableId,
    schema: String,
    snapshot_id: StableId,
    source_ids: Vec<StableId>,
    universe_id: StableId,
}
#[derive(Serialize)]
struct DecisionIdentity<'a> {
    actor: &'a str,
    authority_id: &'a str,
    claim_id: &'a StableId,
    expires_at: &'a Option<String>,
    issued_at: &'a str,
    outcome: DecisionOutcomeV3,
    policy_revision_hash: &'a ContentHash,
    property_id: &'a str,
    rationale: &'a str,
    run_id: &'a StableId,
    snapshot_id: &'a StableId,
    source_ids: &'a Vec<StableId>,
    universe_id: &'a StableId,
}
impl DecisionV3 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_from_claim_scope(
        scope: &ClaimAssessmentScopeV3,
        policy_revision_hash: ContentHash,
        outcome: DecisionOutcomeV3,
        actor: String,
        authority_id: String,
        source_ids: BTreeSet<StableId>,
        rationale: String,
        issued_at: String,
        expires_at: Option<String>,
    ) -> Result<Self> {
        let source_ids = source_ids.into_iter().collect::<Vec<_>>();
        let identity = DecisionIdentity {
            actor: &actor,
            authority_id: &authority_id,
            claim_id: &scope.claim_id,
            expires_at: &expires_at,
            issued_at: &issued_at,
            outcome,
            policy_revision_hash: &policy_revision_hash,
            property_id: &scope.property_id,
            rationale: &rationale,
            run_id: &scope.run_id,
            snapshot_id: &scope.snapshot_id,
            source_ids: &source_ids,
            universe_id: &scope.universe_id,
        };
        let value = Self {
            schema: "reviewgraphen.human_decision.v3".to_owned(),
            id: derived_id("decision", &identity)?,
            policy_revision_hash,
            run_id: scope.run_id.clone(),
            universe_id: scope.universe_id.clone(),
            claim_id: scope.claim_id.clone(),
            property_id: scope.property_id.clone(),
            outcome,
            actor,
            authority_id,
            snapshot_id: scope.snapshot_id.clone(),
            source_ids,
            rationale,
            issued_at,
            expires_at,
        };
        value.validate()?;
        Ok(value)
    }
    #[allow(clippy::too_many_arguments, dead_code)]
    pub(crate) fn new_admitted(
        scope: &AuthorityScopeV3,
        admission: &TrustedHumanGrantProofV3,
        outcome: DecisionOutcomeV3,
        actor: impl Into<String>,
        authority_id: impl Into<String>,
        source_ids: BTreeSet<StableId>,
        rationale: impl Into<String>,
        issued_at: impl Into<String>,
        expires_at: Option<String>,
    ) -> Result<Self> {
        admission.validate(scope)?;
        scope.validate()?;
        let actor = actor.into();
        let authority_id = authority_id.into();
        let rationale = rationale.into();
        let issued_at = issued_at.into();
        let source_ids = source_ids.into_iter().collect::<Vec<_>>();
        let identity = DecisionIdentity {
            actor: &actor,
            authority_id: &authority_id,
            claim_id: &scope.claim_id,
            expires_at: &expires_at,
            issued_at: &issued_at,
            outcome,
            policy_revision_hash: &scope.policy_revision_hash,
            property_id: &scope.property_id,
            rationale: &rationale,
            run_id: &scope.run_id,
            snapshot_id: &scope.snapshot_id,
            source_ids: &source_ids,
            universe_id: &scope.universe_id,
        };
        let v = Self {
            schema: "reviewgraphen.human_decision.v3".to_owned(),
            id: derived_id("decision", &identity)?,
            policy_revision_hash: scope.policy_revision_hash.clone(),
            run_id: scope.run_id.clone(),
            universe_id: scope.universe_id.clone(),
            claim_id: scope.claim_id.clone(),
            property_id: scope.property_id.clone(),
            outcome,
            actor,
            authority_id,
            snapshot_id: scope.snapshot_id.clone(),
            source_ids,
            rationale,
            issued_at,
            expires_at,
        };
        v.validate()?;
        Ok(v)
    }
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    fn new(
        scope: &AuthorityScopeV3,
        outcome: DecisionOutcomeV3,
        actor: impl Into<String>,
        authority_id: impl Into<String>,
        source_ids: BTreeSet<StableId>,
        rationale: impl Into<String>,
        issued_at: impl Into<String>,
        expires_at: Option<String>,
    ) -> Result<Self> {
        Self::new_admitted(
            scope,
            &TrustedHumanGrantProofV3::for_test(scope)?,
            outcome,
            actor,
            authority_id,
            source_ids,
            rationale,
            issued_at,
            expires_at,
        )
    }
    fn identity(&self) -> DecisionIdentity<'_> {
        DecisionIdentity {
            actor: &self.actor,
            authority_id: &self.authority_id,
            claim_id: &self.claim_id,
            expires_at: &self.expires_at,
            issued_at: &self.issued_at,
            outcome: self.outcome,
            policy_revision_hash: &self.policy_revision_hash,
            property_id: &self.property_id,
            rationale: &self.rationale,
            run_id: &self.run_id,
            snapshot_id: &self.snapshot_id,
            source_ids: &self.source_ids,
            universe_id: &self.universe_id,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.human_decision.v3"
            || self.id != derived_id("decision", &self.identity())?
            || self.property_id != M4_PROPERTY_ID
        {
            return Err(validation("invalid DecisionV3 identity/scope"));
        }
        require_kind(&self.run_id, "run", "decision run")?;
        require_kind(&self.universe_id, "universe", "decision universe")?;
        require_kind(&self.snapshot_id, "snapshot", "decision snapshot")?;
        require_kind(&self.claim_id, "claim", "decision claim")?;
        require_text(&self.actor, MAX_TRACE_BYTES, "decision actor")?;
        if self.actor.strip_prefix("human:").is_none_or(str::is_empty) {
            return Err(validation("decision actor must be human:<nonempty>"));
        }
        require_text(&self.authority_id, MAX_TRACE_BYTES, "decision authority")?;
        require_text(&self.rationale, MAX_RATIONALE_BYTES, "decision rationale")?;
        require_text(&self.issued_at, MAX_TRACE_BYTES, "decision issued_at")?;
        validate_utc_seconds(&self.issued_at, "decision issued_at")?;
        require_count(
            self.source_ids.len(),
            1,
            MAX_DECISION_SOURCES,
            "decision sources",
        )?;
        if (self.outcome == DecisionOutcomeV3::Exception) != self.expires_at.is_some() {
            return Err(validation("only Exception requires expires_at"));
        }
        if let Some(x) = &self.expires_at {
            require_text(x, MAX_TRACE_BYTES, "decision expires_at")?;
            validate_utc_seconds(x, "decision expires_at")?;
            if x <= &self.issued_at {
                return Err(validation(
                    "decision expires_at must be strictly later than issued_at",
                ));
            }
        }
        let _ = bounded_bytes(self, "DecisionV3")?;
        Ok(())
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.source_ids)?;
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.policy_revision_hash.allocated_bytes(),
            self.run_id.allocated_bytes(),
            self.universe_id.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.property_id.capacity(),
            self.actor.capacity(),
            self.authority_id.capacity(),
            self.snapshot_id.allocated_bytes(),
            self.rationale.capacity(),
            self.issued_at.capacity(),
            self.expires_at.as_ref().map_or(0, String::capacity),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub fn source_ids(&self) -> &[StableId] {
        &self.source_ids
    }
    pub const fn outcome(&self) -> DecisionOutcomeV3 {
        self.outcome
    }
    pub fn actor(&self) -> &str {
        &self.actor
    }
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub fn policy_revision_hash(&self) -> &ContentHash {
        &self.policy_revision_hash
    }
    pub fn issued_at(&self) -> &str {
        &self.issued_at
    }
    pub fn expires_at(&self) -> Option<&str> {
        self.expires_at.as_deref()
    }
    pub fn authority_id(&self) -> &str {
        &self.authority_id
    }
    pub fn run_id(&self) -> &StableId {
        &self.run_id
    }
    pub fn snapshot_id(&self) -> &StableId {
        &self.snapshot_id
    }
    pub fn universe_id(&self) -> &StableId {
        &self.universe_id
    }
    pub fn property_id(&self) -> &str {
        &self.property_id
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "DecisionV3")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    id: BoundedStableId,
    policy_revision_hash: BoundedContentHash,
    run_id: BoundedStableId,
    universe_id: BoundedStableId,
    claim_id: BoundedStableId,
    property_id: BoundedString<MAX_TRACE_BYTES>,
    outcome: DecisionOutcomeWire,
    actor: BoundedString<MAX_TRACE_BYTES>,
    authority_id: BoundedString<MAX_TRACE_BYTES>,
    snapshot_id: BoundedStableId,
    source_ids: BoundedSeq<BoundedStableId, MAX_DECISION_SOURCES>,
    rationale: BoundedString<MAX_RATIONALE_BYTES>,
    issued_at: BoundedString<MAX_TRACE_BYTES>,
    expires_at: Option<BoundedString<MAX_TRACE_BYTES>>,
}
impl DecisionV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let r: DecisionWire = decode_m4_wire(input, "decision JSON")?;
        let v = Self {
            schema: r.schema.0,
            id: r.id.0,
            policy_revision_hash: r.policy_revision_hash.0,
            run_id: r.run_id.0,
            universe_id: r.universe_id.0,
            claim_id: r.claim_id.0,
            property_id: r.property_id.0,
            outcome: r.outcome.into(),
            actor: r.actor.0,
            authority_id: r.authority_id.0,
            snapshot_id: r.snapshot_id.0,
            source_ids: strict_bounded_ids(r.source_ids.0, "decision sources")?
                .into_iter()
                .collect(),
            rationale: r.rationale.0,
            issued_at: r.issued_at.0,
            expires_at: r.expires_at.map(|value| value.0),
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatusV3 {
    UnverifiedCandidate,
    VerifiedCandidate,
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum M4SensitivityWire {
    CanonicalState,
}
impl From<M4SensitivityWire> for M4SensitivityV3 {
    fn from(_: M4SensitivityWire) -> Self {
        Self::CanonicalState
    }
}

#[derive(Clone, Copy, Deserialize)]
enum VerifierDescriptorWire {
    #[serde(rename = "reviewgraphen.static_fact_verifier@1")]
    StaticFactV1,
    #[serde(rename = "reviewgraphen.fixture_test_verifier@1")]
    FixedFixtureV1,
}
impl From<VerifierDescriptorWire> for VerifierDescriptorV3 {
    fn from(value: VerifierDescriptorWire) -> Self {
        match value {
            VerifierDescriptorWire::StaticFactV1 => Self::StaticFactV1,
            VerifierDescriptorWire::FixedFixtureV1 => Self::FixedFixtureV1,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
enum VerifierProcedureWire {
    #[serde(rename = "reviewgraphen.static_fact.projection@1")]
    StaticProjectionV1,
    #[serde(rename = "reviewgraphen.fixture_test.duplicate_submit@1")]
    DuplicateSubmitV1,
}
impl From<VerifierProcedureWire> for VerifierProcedureV3 {
    fn from(value: VerifierProcedureWire) -> Self {
        match value {
            VerifierProcedureWire::StaticProjectionV1 => Self::StaticProjectionV1,
            VerifierProcedureWire::DuplicateSubmitV1 => Self::DuplicateSubmitV1,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum VerificationOutcomeWire {
    Passed,
    Inconclusive,
    Unsupported,
}
impl From<VerificationOutcomeWire> for VerificationOutcomeV3 {
    fn from(value: VerificationOutcomeWire) -> Self {
        match value {
            VerificationOutcomeWire::Passed => Self::Passed,
            VerificationOutcomeWire::Inconclusive => Self::Inconclusive,
            VerificationOutcomeWire::Unsupported => Self::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StaticApplicabilityWire {
    Absent,
    Unique,
    Ambiguous,
    UnsupportedProperty,
}
impl From<StaticApplicabilityWire> for StaticApplicabilityV1 {
    fn from(value: StaticApplicabilityWire) -> Self {
        match value {
            StaticApplicabilityWire::Absent => Self::Absent,
            StaticApplicabilityWire::Unique => Self::Unique,
            StaticApplicabilityWire::Ambiguous => Self::Ambiguous,
            StaticApplicabilityWire::UnsupportedProperty => Self::UnsupportedProperty,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceKindWire {
    StaticFact,
    TestWitness,
}
impl From<EvidenceKindWire> for EvidenceKindV3 {
    fn from(value: EvidenceKindWire) -> Self {
        match value {
            EvidenceKindWire::StaticFact => Self::StaticFact,
            EvidenceKindWire::TestWitness => Self::TestWitness,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceObservationWire {
    FactPresent,
    Witnessed,
}
impl From<EvidenceObservationWire> for EvidenceObservationV3 {
    fn from(value: EvidenceObservationWire) -> Self {
        match value {
            EvidenceObservationWire::FactPresent => Self::FactPresent,
            EvidenceObservationWire::Witnessed => Self::Witnessed,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EvidenceRelationWire {
    Qualifies,
    Reproduces,
}
impl From<EvidenceRelationWire> for EvidenceRelationV3 {
    fn from(value: EvidenceRelationWire) -> Self {
        match value {
            EvidenceRelationWire::Qualifies => Self::Qualifies,
            EvidenceRelationWire::Reproduces => Self::Reproduces,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DecisionOutcomeWire {
    Accept,
    Reject,
    Defer,
    Exception,
}
impl From<DecisionOutcomeWire> for DecisionOutcomeV3 {
    fn from(value: DecisionOutcomeWire) -> Self {
        match value {
            DecisionOutcomeWire::Accept => Self::Accept,
            DecisionOutcomeWire::Reject => Self::Reject,
            DecisionOutcomeWire::Defer => Self::Defer,
            DecisionOutcomeWire::Exception => Self::Exception,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FindingStatusWire {
    UnverifiedCandidate,
    VerifiedCandidate,
    Accepted,
    Rejected,
}
impl From<FindingStatusWire> for FindingStatusV3 {
    fn from(value: FindingStatusWire) -> Self {
        match value {
            FindingStatusWire::UnverifiedCandidate => Self::UnverifiedCandidate,
            FindingStatusWire::VerifiedCandidate => Self::VerifiedCandidate,
            FindingStatusWire::Accepted => Self::Accepted,
            FindingStatusWire::Rejected => Self::Rejected,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FindingV3 {
    claim_id: StableId,
    decision_id: Option<StableId>,
    evidence_ids: Vec<StableId>,
    id: StableId,
    projection_descriptor_id: String,
    schema: String,
    status: FindingStatusV3,
    supersedes_finding_id: Option<StableId>,
    verification_ids: Vec<StableId>,
}
#[derive(Serialize)]
struct FindingIdentity<'a> {
    claim_id: &'a StableId,
    decision_id: &'a Option<StableId>,
    evidence_ids: &'a Vec<StableId>,
    projection_descriptor_id: &'a str,
    status: FindingStatusV3,
    supersedes_finding_id: &'a Option<StableId>,
    verification_ids: &'a Vec<StableId>,
}
impl FindingV3 {
    #[allow(dead_code)]
    fn build(
        claim_id: StableId,
        status: FindingStatusV3,
        evidence_ids: Vec<StableId>,
        verification_ids: Vec<StableId>,
        decision_id: Option<StableId>,
        supersedes_finding_id: Option<StableId>,
    ) -> Result<Self> {
        let projection_descriptor_id = FINDING_PROJECTION_ID.to_owned();
        let identity = FindingIdentity {
            claim_id: &claim_id,
            decision_id: &decision_id,
            evidence_ids: &evidence_ids,
            projection_descriptor_id: &projection_descriptor_id,
            status,
            supersedes_finding_id: &supersedes_finding_id,
            verification_ids: &verification_ids,
        };
        let v = Self {
            schema: "reviewgraphen.finding.v3".to_owned(),
            id: derived_id("finding", &identity)?,
            projection_descriptor_id,
            claim_id,
            status,
            evidence_ids,
            verification_ids,
            decision_id,
            supersedes_finding_id,
        };
        v.validate()?;
        Ok(v)
    }
    fn identity(&self) -> FindingIdentity<'_> {
        FindingIdentity {
            claim_id: &self.claim_id,
            decision_id: &self.decision_id,
            evidence_ids: &self.evidence_ids,
            projection_descriptor_id: &self.projection_descriptor_id,
            status: self.status,
            supersedes_finding_id: &self.supersedes_finding_id,
            verification_ids: &self.verification_ids,
        }
    }
    fn validate(&self) -> Result<()> {
        if self.schema != "reviewgraphen.finding.v3"
            || self.projection_descriptor_id != FINDING_PROJECTION_ID
            || self.id != derived_id("finding", &self.identity())?
        {
            return Err(validation("invalid FindingV3"));
        }
        require_kind(&self.claim_id, "claim", "finding claim")?;
        require_count(self.evidence_ids.len(), 0, MAX_SET, "finding evidence")?;
        require_count(
            self.verification_ids.len(),
            0,
            MAX_SET,
            "finding verification",
        )?;
        if !self.evidence_ids.windows(2).all(|pair| pair[0] < pair[1])
            || !self
                .verification_ids
                .windows(2)
                .all(|pair| pair[0] < pair[1])
        {
            return Err(M4Error::NonCanonicalOrder {
                field: "finding source IDs",
            });
        }
        if matches!(
            self.status,
            FindingStatusV3::Accepted | FindingStatusV3::Rejected
        ) != self.decision_id.is_some()
        {
            return Err(validation("finding decision/status mismatch"));
        }
        if let Some(x) = &self.decision_id {
            require_kind(x, "decision", "finding decision")?;
        }
        if let Some(x) = &self.supersedes_finding_id {
            require_kind(x, "finding", "superseded finding")?;
        }
        let _ = bounded_bytes(self, "FindingV3")?;
        Ok(())
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.evidence_ids)?
            .checked_add(id_vec_allocated_bytes(&self.verification_ids)?)
            .ok_or(M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        for bytes in [
            self.schema.capacity(),
            self.id.allocated_bytes(),
            self.projection_descriptor_id.capacity(),
            self.claim_id.allocated_bytes(),
            self.decision_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
            self.supersedes_finding_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
    pub fn id(&self) -> &StableId {
        &self.id
    }
    pub const fn status(&self) -> FindingStatusV3 {
        self.status
    }
    pub fn decision_id(&self) -> Option<&StableId> {
        self.decision_id.as_ref()
    }
    pub fn claim_id(&self) -> &StableId {
        &self.claim_id
    }
    pub fn evidence_ids(&self) -> &[StableId] {
        &self.evidence_ids
    }
    pub fn verification_ids(&self) -> &[StableId] {
        &self.verification_ids
    }
    pub fn supersedes_finding_id(&self) -> Option<&StableId> {
        self.supersedes_finding_id.as_ref()
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "FindingV3")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindingWire {
    schema: BoundedString<MAX_TRACE_BYTES>,
    id: BoundedStableId,
    projection_descriptor_id: BoundedString<MAX_TRACE_BYTES>,
    claim_id: BoundedStableId,
    status: FindingStatusWire,
    evidence_ids: BoundedSeq<BoundedStableId, MAX_SET>,
    verification_ids: BoundedSeq<BoundedStableId, MAX_SET>,
    decision_id: Option<BoundedStableId>,
    supersedes_finding_id: Option<BoundedStableId>,
}
impl FindingV3 {
    pub fn from_json_bytes(input: &[u8]) -> Result<Self> {
        let r: FindingWire = decode_m4_wire(input, "finding JSON")?;
        let v = Self {
            schema: r.schema.0,
            id: r.id.0,
            projection_descriptor_id: r.projection_descriptor_id.0,
            claim_id: r.claim_id.0,
            status: r.status.into(),
            evidence_ids: strict_bounded_ids(r.evidence_ids.0, "finding evidence")?
                .into_iter()
                .collect(),
            verification_ids: strict_bounded_ids(r.verification_ids.0, "finding verification")?
                .into_iter()
                .collect(),
            decision_id: r.decision_id.map(|value| value.0),
            supersedes_finding_id: r.supersedes_finding_id.map(|value| value.0),
        };
        v.validate()?;
        Ok(v)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentDispositionV3 {
    Proposed,
    Supported,
    Accepted,
    Rejected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentReviewStatusV3 {
    Unreviewed,
    HumanReviewed,
    Accepted,
    Rejected,
}

/// Internal claim/run scope shared by static, fixture, human, and replay
/// assessment paths. Unlike `AuthorityScopeV3`, it carries no fixture harness
/// tuple, so pure static verification never depends on unrelated harness
/// authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClaimAssessmentScopeV3 {
    run_id: StableId,
    genesis_hash: ContentHash,
    snapshot_id: StableId,
    universe_id: StableId,
    claim_id: StableId,
    claim_body_hash: ContentHash,
    property_id: String,
    polarity: ClaimPolarity,
    target_refs: Vec<StableId>,
    source_ids: Vec<StableId>,
}

/// Allocation-free prediction of the dynamic backing retained by a freshly
/// initialized D2 assessment scope and assessment. It mirrors
/// `ClaimAssessmentScopeV3::new` plus `ClaimAssessmentV3::new_from_scope`
/// using borrowed scalar lengths only; notably it never computes a canonical
/// claim body or constructs either M4 value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InitialClaimAssessmentBackingV3 {
    scope_dynamic_bytes: u64,
    assessment_dynamic_bytes: u64,
}

impl InitialClaimAssessmentBackingV3 {
    pub(crate) const fn scope_dynamic_bytes(self) -> u64 {
        self.scope_dynamic_bytes
    }

    pub(crate) const fn assessment_dynamic_bytes(self) -> u64 {
        self.assessment_dynamic_bytes
    }
}

pub(crate) fn predicted_initial_claim_assessment_backing_v3(
    run_id: &StableId,
    genesis_hash: &ContentHash,
    snapshot_id: &StableId,
    universe_id: &StableId,
    claim: &ExecutionClaimV2,
) -> Result<InitialClaimAssessmentBackingV3> {
    fn add(total: &mut u64, value: usize) -> Result<()> {
        checked_memory_add(total, value)
    }

    fn id_set_backing(values: &BTreeSet<StableId>) -> Result<u64> {
        let mut total = 0_u64;
        let slots = values
            .len()
            .checked_mul(std::mem::size_of::<StableId>())
            .ok_or(M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        add(&mut total, slots)?;
        for value in values {
            add(&mut total, value.as_str().len())?;
        }
        Ok(total)
    }

    let mut scope_dynamic_bytes = id_set_backing(claim.target_refs())?
        .checked_add(id_set_backing(claim.source_ids())?)
        .ok_or(M4Error::Incomplete {
            operation: "M4 retained verifier bytes",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    for value in [
        run_id.as_str().len(),
        genesis_hash.as_str().len(),
        snapshot_id.as_str().len(),
        universe_id.as_str().len(),
        claim.id().as_str().len(),
        "sha256:".len() + 64,
        claim.property_id().len(),
    ] {
        add(&mut scope_dynamic_bytes, value)?;
    }
    let assessment_dynamic_bytes = scope_dynamic_bytes
        .checked_add(
            u64::try_from(claim.id().as_str().len()).map_err(|_| M4Error::Incomplete {
                operation: "M4 retained verifier bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?,
        )
        .ok_or(M4Error::Incomplete {
            operation: "M4 retained verifier bytes",
            limit: MAX_RETAINED_WORKING_BYTES as usize,
            observed: usize::MAX,
        })?;
    Ok(InitialClaimAssessmentBackingV3 {
        scope_dynamic_bytes,
        assessment_dynamic_bytes,
    })
}

impl ClaimAssessmentScopeV3 {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        run_id: StableId,
        genesis_hash: ContentHash,
        snapshot_id: StableId,
        universe_id: StableId,
        claim: &ExecutionClaimV2,
    ) -> Result<Self> {
        let value = Self {
            run_id,
            genesis_hash,
            snapshot_id,
            universe_id,
            claim_id: claim.id().clone(),
            claim_body_hash: claim.body_hash()?,
            property_id: claim.property_id().to_owned(),
            polarity: claim.polarity(),
            target_refs: claim.target_refs().iter().cloned().collect(),
            source_ids: claim.source_ids().iter().cloned().collect(),
        };
        value.validate(claim)?;
        Ok(value)
    }

    fn from_authority(scope: &AuthorityScopeV3) -> Self {
        Self {
            run_id: scope.run_id.clone(),
            genesis_hash: scope.genesis_hash.clone(),
            snapshot_id: scope.snapshot_id.clone(),
            universe_id: scope.universe_id.clone(),
            claim_id: scope.claim_id.clone(),
            claim_body_hash: scope.claim_body_hash.clone(),
            property_id: scope.property_id.clone(),
            polarity: scope.polarity,
            target_refs: scope.target_refs.clone(),
            source_ids: scope.source_ids.clone(),
        }
    }

    fn validate(&self, claim: &ExecutionClaimV2) -> Result<()> {
        require_kind(&self.run_id, "run", "assessment run")?;
        require_kind(&self.snapshot_id, "snapshot", "assessment snapshot")?;
        require_kind(&self.universe_id, "universe", "assessment universe")?;
        require_kind(&self.claim_id, "claim", "assessment claim")?;
        if self.claim_id != *claim.id()
            || self.claim_body_hash != claim.body_hash()?
            || self.property_id != claim.property_id()
            || self.polarity != claim.polarity()
            || self.target_refs != claim.target_refs().iter().cloned().collect::<Vec<_>>()
            || self.source_ids != claim.source_ids().iter().cloned().collect::<Vec<_>>()
        {
            return Err(M4Error::ClaimBodyMismatch);
        }
        Ok(())
    }

    pub(crate) fn allocated_bytes(&self) -> Result<u64> {
        let mut total = id_vec_allocated_bytes(&self.target_refs)?
            .checked_add(id_vec_allocated_bytes(&self.source_ids)?)
            .ok_or(M4Error::Incomplete {
                operation: "M4 assessment scope bytes",
                limit: MAX_RETAINED_WORKING_BYTES as usize,
                observed: usize::MAX,
            })?;
        for bytes in [
            self.run_id.allocated_bytes(),
            self.genesis_hash.allocated_bytes(),
            self.snapshot_id.allocated_bytes(),
            self.universe_id.allocated_bytes(),
            self.claim_id.allocated_bytes(),
            self.claim_body_hash.allocated_bytes(),
            self.property_id.capacity(),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        Ok(total)
    }
}

/// Claim-local reducer state. Mutable retained collections are canonical
/// sorted vectors. The 16 MiB contract charges the exact requested element
/// slots and owned string capacities before `try_reserve_exact`; an allocator
/// may return a larger physical capacity without changing domain validity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClaimAssessmentV3 {
    #[serde(skip)]
    scope: ClaimAssessmentScopeV3,
    active_decision_id: Option<StableId>,
    binding_ids: Vec<StableId>,
    claim_id: StableId,
    current_finding_id: Option<StableId>,
    decision_conflict: bool,
    decision_ids: Vec<StableId>,
    disposition: AssessmentDispositionV3,
    evidence_ids: Vec<StableId>,
    finding_ids: Vec<StableId>,
    review_status: AssessmentReviewStatusV3,
    verification_ids: Vec<StableId>,
    #[serde(skip)]
    bindings: Vec<EvidenceBindingV3>,
    #[serde(skip)]
    evidence: Vec<EvidenceV3>,
    #[serde(skip)]
    verifications: Vec<VerificationV3>,
    #[serde(skip)]
    decisions: Vec<DecisionV3>,
    #[serde(skip)]
    findings: Vec<FindingV3>,
    #[serde(skip)]
    last_finding_id: Option<StableId>,
}
#[allow(dead_code)]
impl ClaimAssessmentV3 {
    pub(crate) fn new(scope: &AuthorityScopeV3) -> Self {
        Self::new_from_scope(ClaimAssessmentScopeV3::from_authority(scope))
    }

    pub(crate) fn new_from_scope(scope: ClaimAssessmentScopeV3) -> Self {
        Self {
            claim_id: scope.claim_id.clone(),
            scope,
            disposition: AssessmentDispositionV3::Proposed,
            review_status: AssessmentReviewStatusV3::Unreviewed,
            binding_ids: Vec::new(),
            evidence_ids: Vec::new(),
            verification_ids: Vec::new(),
            decision_ids: Vec::new(),
            finding_ids: Vec::new(),
            active_decision_id: None,
            current_finding_id: None,
            decision_conflict: false,
            bindings: Vec::new(),
            evidence: Vec::new(),
            verifications: Vec::new(),
            decisions: Vec::new(),
            findings: Vec::new(),
            last_finding_id: None,
        }
    }
    fn baseline(&self) -> AssessmentDispositionV3 {
        if self
            .bindings
            .iter()
            .any(|b| b.relation == EvidenceRelationV3::Reproduces)
        {
            AssessmentDispositionV3::Supported
        } else {
            AssessmentDispositionV3::Proposed
        }
    }

    fn validate_claim_scope(&self, scope: &ClaimAssessmentScopeV3) -> Result<()> {
        if &self.scope != scope {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        Ok(())
    }

    pub(crate) fn record_binding_replayed(
        &mut self,
        scope: &ClaimAssessmentScopeV3,
        evidence: EvidenceV3,
        binding: EvidenceBindingV3,
    ) -> Result<()> {
        self.validate_claim_scope(scope)?;
        if find_sorted_record(&self.bindings, &binding.id, |value| &value.id).is_some() {
            return Err(M4Error::IdCollision {
                id: binding.id.clone(),
            });
        }
        if binding.claim_id != scope.claim_id || binding.property_id != scope.property_id {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        if binding.evidence_id != evidence.id || evidence.snapshot_id != scope.snapshot_id {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let exact = matches!(
            (binding.relation, evidence.descriptor),
            (
                EvidenceRelationV3::Qualifies,
                VerifierDescriptorV3::StaticFactV1
            ) | (
                EvidenceRelationV3::Reproduces,
                VerifierDescriptorV3::FixedFixtureV1
            )
        );
        if !exact
            || !evidence
                .subject_ids
                .iter()
                .any(|id| scope.target_refs.contains(id) || scope.source_ids.contains(id))
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        if self.bindings.len() >= MAX_SET || self.evidence.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment bindings",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        self.ensure_working_additions(&[
            record_addition_bytes(
                &evidence.id,
                size_of::<EvidenceV3>(),
                evidence.allocated_bytes()?,
                canonical_len(&evidence, "assessment replay evidence scratch")?,
            )?,
            record_addition_bytes(
                &binding.id,
                size_of::<EvidenceBindingV3>(),
                binding.allocated_bytes()?,
                canonical_len(&binding, "assessment replay binding scratch")?,
            )?,
        ])?;
        reserve_one(&mut self.evidence, "assessment evidence reservation")?;
        reserve_one(&mut self.evidence_ids, "assessment evidence ID reservation")?;
        reserve_one(&mut self.bindings, "assessment binding reservation")?;
        reserve_one(&mut self.binding_ids, "assessment binding ID reservation")?;
        self.invalidate_trace();
        insert_id_sorted_reserved(&mut self.evidence_ids, evidence.id.clone());
        insert_sorted_reserved(&mut self.evidence, evidence, |value| &value.id);
        insert_id_sorted_reserved(&mut self.binding_ids, binding.id.clone());
        insert_sorted_reserved(&mut self.bindings, binding, |value| &value.id);
        self.disposition = self.baseline();
        Ok(())
    }

    pub(crate) fn record_verification_replayed(
        &mut self,
        scope: &ClaimAssessmentScopeV3,
        verification: VerificationV3,
    ) -> Result<()> {
        self.validate_claim_scope(scope)?;
        if find_sorted_record(&self.verifications, &verification.id, |value| &value.id).is_some() {
            return Err(M4Error::IdCollision {
                id: verification.id.clone(),
            });
        }
        if verification.claim_id != scope.claim_id {
            return Err(M4Error::ClaimMismatch {
                expected: scope.claim_id.clone(),
                actual: verification.claim_id.clone(),
            });
        }
        for id in &verification.evidence_ids {
            let evidence =
                find_sorted_record(&self.evidence, id, |value| &value.id).ok_or_else(|| {
                    M4Error::DanglingReference {
                        owner: "verification",
                        reference: id.clone(),
                    }
                })?;
            if evidence.descriptor != verification.descriptor
                || evidence.input_registration_id != verification.input_registration_id
                || evidence.output_registration_id != verification.output_registration_id
                || (verification.outcome == VerificationOutcomeV3::Passed
                    && !self.bindings.iter().any(|binding| {
                        binding.evidence_id == *id
                            && binding.claim_id == scope.claim_id
                            && binding.relation == EvidenceRelationV3::Reproduces
                    }))
            {
                return Err(M4Error::AuthorityScopeMismatch);
            }
        }
        if self.verifications.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment verifications",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        self.ensure_working_additions(&[record_addition_bytes(
            &verification.id,
            size_of::<VerificationV3>(),
            verification.allocated_bytes()?,
            canonical_len(&verification, "assessment replay verification scratch")?,
        )?])?;
        reserve_one(
            &mut self.verifications,
            "assessment verification reservation",
        )?;
        reserve_one(
            &mut self.verification_ids,
            "assessment verification ID reservation",
        )?;
        self.invalidate_trace();
        insert_id_sorted_reserved(&mut self.verification_ids, verification.id.clone());
        insert_sorted_reserved(&mut self.verifications, verification, |value| &value.id);
        Ok(())
    }
    fn invalidate_trace(&mut self) {
        self.current_finding_id = None;
        if self.active_decision_id.take().is_some() {
            self.disposition = self.baseline();
            self.review_status = AssessmentReviewStatusV3::HumanReviewed;
            self.decision_conflict = true;
        }
    }
    pub(crate) fn retained_bytes(&self) -> Result<u64> {
        // Requested-capacity accounting is intentionally derived from logical
        // sorted-vector slots. No BTree node-layout estimate or allocator-
        // specific spare capacity participates in domain validity.
        let mut total = self.scope.allocated_bytes()?;
        for values in [
            &self.binding_ids,
            &self.evidence_ids,
            &self.verification_ids,
            &self.decision_ids,
            &self.finding_ids,
        ] {
            checked_memory_add(&mut total, values.len() * size_of::<StableId>())?;
            for value in values {
                checked_memory_add(&mut total, value.allocated_bytes())?;
            }
        }
        for bytes in [
            self.claim_id.allocated_bytes(),
            self.active_decision_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
            self.current_finding_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
            self.last_finding_id
                .as_ref()
                .map_or(0, StableId::allocated_bytes),
        ] {
            checked_memory_add(&mut total, bytes)?;
        }
        checked_memory_add(&mut total, self.evidence.len() * size_of::<EvidenceV3>())?;
        for value in &self.evidence {
            total = total
                .checked_add(value.allocated_bytes()?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        checked_memory_add(
            &mut total,
            self.bindings.len() * size_of::<EvidenceBindingV3>(),
        )?;
        for value in &self.bindings {
            total = total
                .checked_add(value.allocated_bytes()?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        checked_memory_add(
            &mut total,
            self.verifications.len() * size_of::<VerificationV3>(),
        )?;
        for value in &self.verifications {
            total = total
                .checked_add(value.allocated_bytes()?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        checked_memory_add(&mut total, self.decisions.len() * size_of::<DecisionV3>())?;
        for value in &self.decisions {
            total = total
                .checked_add(value.allocated_bytes()?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        checked_memory_add(&mut total, self.findings.len() * size_of::<FindingV3>())?;
        for value in &self.findings {
            total = total
                .checked_add(value.allocated_bytes()?)
                .ok_or(M4Error::Incomplete {
                    operation: "M4 retained verifier bytes",
                    limit: MAX_RETAINED_WORKING_BYTES as usize,
                    observed: usize::MAX,
                })?;
        }
        Ok(total)
    }
    fn ensure_working_additions(&self, additions: &[u64]) -> Result<()> {
        #[cfg(test)]
        M4_WORKING_PREFLIGHTS.with(|value| value.set(value.get() + 1));
        let retained = self.retained_bytes()?;
        #[cfg(test)]
        let retained = M4_TEST_RETAINED_OVERRIDE.with(|value| value.get().unwrap_or(retained));
        #[cfg(test)]
        let limit =
            M4_TEST_WORKING_LIMIT.with(|value| value.get().unwrap_or(MAX_RETAINED_WORKING_BYTES));
        #[cfg(not(test))]
        let limit = MAX_RETAINED_WORKING_BYTES;
        let _peak = checked_working_peak_with_limit(retained, additions.iter().copied(), limit)?;
        #[cfg(test)]
        M4_LAST_WORKING_PEAK.with(|value| value.set(Some(_peak)));
        Ok(())
    }
    pub(crate) fn record_static_admitted(
        &mut self,
        scope: &StaticScopeV3,
        proof: LockedStaticProofV3,
        proposal: StaticRecordProposalV1,
    ) -> Result<()> {
        proof.validate(scope)?;
        if let Some(evidence) = &proposal.evidence
            && find_sorted_record(&self.evidence, &evidence.id, |value| &value.id).is_some()
        {
            return Err(M4Error::IdCollision {
                id: evidence.id.clone(),
            });
        }
        if let Some(binding) = &proposal.binding
            && find_sorted_record(&self.bindings, &binding.id, |value| &value.id).is_some()
        {
            return Err(M4Error::IdCollision {
                id: binding.id.clone(),
            });
        }
        if find_sorted_record(&self.verifications, &proposal.verification.id, |value| {
            &value.id
        })
        .is_some()
        {
            return Err(M4Error::IdCollision {
                id: proposal.verification.id.clone(),
            });
        }
        if proposal.scope != *scope
            || scope.snapshot_id != self.scope.snapshot_id
            || scope.claim_id != self.scope.claim_id
            || scope.claim_body_hash != self.scope.claim_body_hash
            || scope.property_id != self.scope.property_id
            || scope.target_refs.len() != self.scope.target_refs.len()
            || !scope
                .target_refs
                .iter()
                .all(|id| self.scope.target_refs.contains(id))
            || scope.source_ids.len() != self.scope.source_ids.len()
            || !scope
                .source_ids
                .iter()
                .all(|id| self.scope.source_ids.contains(id))
            || scope.descriptor != VerifierDescriptorV3::StaticFactV1
            || scope.procedure != VerifierProcedureV3::StaticProjectionV1
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        let unique = scope.selected_invariant_id.is_some();
        if unique != proposal.evidence.is_some() || unique != proposal.binding.is_some() {
            return Err(M4Error::InvalidRecord {
                reason: "static proposal cardinality splice".to_owned(),
            });
        }
        if let (Some(evidence), Some(binding)) = (&proposal.evidence, &proposal.binding) {
            let selected = scope
                .selected_invariant_id
                .as_ref()
                .ok_or(M4Error::InvalidRecord {
                    reason: "static selected invariant missing".to_owned(),
                })?;
            if evidence.snapshot_id != scope.snapshot_id
                || evidence.subject_ids.len() != scope.target_refs.len() + 1
                || !evidence.subject_ids.contains(selected)
                || !scope
                    .target_refs
                    .iter()
                    .all(|target| evidence.subject_ids.contains(target))
                || evidence.descriptor != scope.descriptor
                || evidence.procedure != scope.procedure
                || binding.claim_id != scope.claim_id
                || binding.evidence_id != evidence.id
                || binding.property_id != scope.property_id
                || binding.relation != EvidenceRelationV3::Qualifies
                || proposal.verification.evidence_ids.len() != 1
                || !proposal.verification.evidence_ids.contains(&evidence.id)
            {
                return Err(M4Error::AuthorityScopeMismatch);
            }
        } else if !proposal.verification.evidence_ids.is_empty() {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        if proposal.verification.claim_id != scope.claim_id
            || proposal.verification.descriptor != scope.descriptor
            || proposal.verification.procedure != scope.procedure
            || proposal.verification.outcome == VerificationOutcomeV3::Passed
        {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        if self.evidence.len() + usize::from(unique) > MAX_SET
            || self.bindings.len() + usize::from(unique) > MAX_SET
            || self.verifications.len() >= MAX_SET
        {
            return Err(M4Error::Incomplete {
                operation: "assessment static records",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        let verification_bytes = record_addition_bytes(
            &proposal.verification.id,
            size_of::<VerificationV3>(),
            proposal.verification.allocated_bytes()?,
            canonical_len(&proposal.verification, "assessment verification scratch")?,
        )?;
        let evidence_bytes = match &proposal.evidence {
            Some(evidence) => record_addition_bytes(
                &evidence.id,
                size_of::<EvidenceV3>(),
                evidence.allocated_bytes()?,
                canonical_len(evidence, "assessment evidence scratch")?,
            )?,
            None => 0,
        };
        let binding_bytes = match &proposal.binding {
            Some(binding) => record_addition_bytes(
                &binding.id,
                size_of::<EvidenceBindingV3>(),
                binding.allocated_bytes()?,
                canonical_len(binding, "assessment binding scratch")?,
            )?,
            None => 0,
        };
        self.ensure_working_additions(&[verification_bytes, evidence_bytes, binding_bytes])?;
        reserve_one(
            &mut self.verifications,
            "assessment verification reservation",
        )?;
        reserve_one(
            &mut self.verification_ids,
            "assessment verification ID reservation",
        )?;
        if proposal.evidence.is_some() {
            reserve_one(&mut self.evidence, "assessment evidence reservation")?;
            reserve_one(&mut self.evidence_ids, "assessment evidence ID reservation")?;
        }
        if proposal.binding.is_some() {
            reserve_one(&mut self.bindings, "assessment binding reservation")?;
            reserve_one(&mut self.binding_ids, "assessment binding ID reservation")?;
        }
        self.invalidate_trace();
        if let Some(evidence) = proposal.evidence {
            insert_id_sorted_reserved(&mut self.evidence_ids, evidence.id.clone());
            insert_sorted_reserved(&mut self.evidence, evidence, |value| &value.id);
        }
        if let Some(binding) = proposal.binding {
            insert_id_sorted_reserved(&mut self.binding_ids, binding.id.clone());
            insert_sorted_reserved(&mut self.bindings, binding, |value| &value.id);
        }
        insert_id_sorted_reserved(&mut self.verification_ids, proposal.verification.id.clone());
        insert_sorted_reserved(&mut self.verifications, proposal.verification, |value| {
            &value.id
        });
        self.disposition = self.baseline();
        Ok(())
    }
    pub(crate) fn record_evidence_admitted(
        &mut self,
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
        evidence: EvidenceV3,
        binding: EvidenceBindingV3,
    ) -> Result<()> {
        admission.validate(scope)?;
        let evidence_collision =
            find_sorted_record(&self.evidence, &evidence.id, |value| &value.id).is_some();
        let binding_collision =
            find_sorted_record(&self.bindings, &binding.id, |value| &value.id).is_some();
        if evidence_collision || binding_collision {
            return Err(M4Error::IdCollision {
                id: if evidence_collision {
                    evidence.id.clone()
                } else {
                    binding.id.clone()
                },
            });
        }
        self.validate_scope(scope)?;
        if evidence.snapshot_id != scope.snapshot_id
            || binding.claim_id != self.claim_id
            || binding.evidence_id != evidence.id
            || binding.property_id != scope.property_id
        {
            if evidence.snapshot_id != scope.snapshot_id {
                return Err(M4Error::SnapshotMismatch {
                    expected: scope.snapshot_id.clone(),
                    actual: evidence.snapshot_id.clone(),
                });
            }
            if binding.claim_id != self.claim_id {
                return Err(M4Error::ClaimMismatch {
                    expected: self.claim_id.clone(),
                    actual: binding.claim_id.clone(),
                });
            }
            if binding.property_id != scope.property_id {
                return Err(M4Error::PropertyMismatch {
                    expected: scope.property_id.clone(),
                    actual: binding.property_id.clone(),
                });
            }
            return Err(M4Error::DanglingReference {
                owner: "binding",
                reference: binding.evidence_id.clone(),
            });
        }
        if self.evidence.len() >= MAX_SET || self.bindings.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment evidence/bindings",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        let relation_matches = matches!(
            (binding.relation, evidence.descriptor),
            (
                EvidenceRelationV3::Qualifies,
                VerifierDescriptorV3::StaticFactV1
            ) | (
                EvidenceRelationV3::Reproduces,
                VerifierDescriptorV3::FixedFixtureV1
            )
        );
        if !relation_matches {
            return Err(M4Error::InvalidRecord {
                reason: "binding relation does not match evidence descriptor".to_owned(),
            });
        }
        self.ensure_working_additions(&[
            record_addition_bytes(
                &evidence.id,
                size_of::<EvidenceV3>(),
                evidence.allocated_bytes()?,
                canonical_len(&evidence, "assessment evidence scratch")?,
            )?,
            record_addition_bytes(
                &binding.id,
                size_of::<EvidenceBindingV3>(),
                binding.allocated_bytes()?,
                canonical_len(&binding, "assessment binding scratch")?,
            )?,
        ])?;
        if evidence.descriptor == VerifierDescriptorV3::FixedFixtureV1
            && (evidence.subject_ids.len() != scope.target_refs.len() + 1
                || !evidence
                    .subject_ids
                    .iter()
                    .any(|id| id.as_str() == FIXTURE_TEST_ARTIFACT_ID)
                || !scope
                    .target_refs
                    .iter()
                    .all(|target| evidence.subject_ids.contains(target)))
        {
            return Err(M4Error::ClaimBodyMismatch);
        }
        reserve_one(&mut self.evidence, "assessment evidence reservation")?;
        reserve_one(&mut self.evidence_ids, "assessment evidence ID reservation")?;
        reserve_one(&mut self.bindings, "assessment binding reservation")?;
        reserve_one(&mut self.binding_ids, "assessment binding ID reservation")?;
        self.invalidate_trace();
        insert_id_sorted_reserved(&mut self.evidence_ids, evidence.id.clone());
        insert_sorted_reserved(&mut self.evidence, evidence, |value| &value.id);
        insert_id_sorted_reserved(&mut self.binding_ids, binding.id.clone());
        insert_sorted_reserved(&mut self.bindings, binding, |value| &value.id);
        self.disposition = self.baseline();
        Ok(())
    }
    #[cfg(test)]
    fn record_evidence(
        &mut self,
        scope: &AuthorityScopeV3,
        evidence: EvidenceV3,
        binding: EvidenceBindingV3,
    ) -> Result<()> {
        self.record_evidence_admitted(
            scope,
            &LockedVerificationProofV3::for_test(scope)?,
            evidence,
            binding,
        )
    }
    pub(crate) fn record_verification_admitted(
        &mut self,
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
        verification: VerificationV3,
    ) -> Result<()> {
        admission.validate(scope)?;
        if find_sorted_record(&self.verifications, &verification.id, |value| &value.id).is_some() {
            return Err(M4Error::IdCollision {
                id: verification.id.clone(),
            });
        }
        self.validate_scope(scope)?;
        if verification.claim_id != self.claim_id {
            return Err(M4Error::ClaimMismatch {
                expected: self.claim_id.clone(),
                actual: verification.claim_id.clone(),
            });
        }
        for id in &verification.evidence_ids {
            let Some(e) = find_sorted_record(&self.evidence, id, |value| &value.id) else {
                return Err(M4Error::DanglingReference {
                    owner: "verification",
                    reference: id.clone(),
                });
            };
            if e.snapshot_id != scope.snapshot_id {
                return Err(M4Error::SnapshotMismatch {
                    expected: scope.snapshot_id.clone(),
                    actual: e.snapshot_id.clone(),
                });
            }
            if e.descriptor != verification.descriptor
                || e.input_registration_id != verification.input_registration_id
                || e.output_registration_id != verification.output_registration_id
            {
                return Err(M4Error::InvalidRecord {
                    reason: "verification descriptor/registration splice".to_owned(),
                });
            }
            if verification.outcome == VerificationOutcomeV3::Passed
                && !self.bindings.iter().any(|b| {
                    b.evidence_id == *id
                        && b.claim_id == self.claim_id
                        && b.relation == EvidenceRelationV3::Reproduces
                })
            {
                return Err(M4Error::DanglingReference {
                    owner: "passed verification binding",
                    reference: id.clone(),
                });
            }
        }
        if self.verifications.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment verifications",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        self.ensure_working_additions(&[record_addition_bytes(
            &verification.id,
            size_of::<VerificationV3>(),
            verification.allocated_bytes()?,
            canonical_len(&verification, "assessment verification scratch")?,
        )?])?;
        reserve_one(
            &mut self.verifications,
            "assessment verification reservation",
        )?;
        reserve_one(
            &mut self.verification_ids,
            "assessment verification ID reservation",
        )?;
        self.invalidate_trace();
        insert_id_sorted_reserved(&mut self.verification_ids, verification.id.clone());
        insert_sorted_reserved(&mut self.verifications, verification, |value| &value.id);
        Ok(())
    }
    #[cfg(test)]
    fn record_verification(
        &mut self,
        scope: &AuthorityScopeV3,
        verification: VerificationV3,
    ) -> Result<()> {
        self.record_verification_admitted(
            scope,
            &LockedVerificationProofV3::for_test(scope)?,
            verification,
        )
    }
    fn exact_sources(&self, outcome: DecisionOutcomeV3) -> Result<BTreeSet<StableId>> {
        let mut ids = BTreeSet::from([self.claim_id.clone()]);
        match outcome {
            DecisionOutcomeV3::Accept => {
                for b in self
                    .bindings
                    .iter()
                    .filter(|b| b.relation == EvidenceRelationV3::Reproduces)
                {
                    ids.insert(b.id.clone());
                    ids.insert(b.evidence_id.clone());
                    for v in self.verifications.iter().filter(|v| {
                        v.outcome == VerificationOutcomeV3::Passed
                            && v.evidence_ids.contains(&b.evidence_id)
                    }) {
                        ids.insert(v.id.clone());
                    }
                }
                if !ids.iter().any(|id| id.kind() == "verification") {
                    return Err(validation("Accept requires a Passed reproducing trace"));
                }
            }
            DecisionOutcomeV3::Reject | DecisionOutcomeV3::Exception => {}
            DecisionOutcomeV3::Defer => {
                for v in self
                    .verifications
                    .iter()
                    .filter(|v| v.outcome != VerificationOutcomeV3::Passed)
                {
                    ids.insert(v.id.clone());
                    for eid in &v.evidence_ids {
                        ids.insert(eid.clone());
                        for b in self.bindings.iter().filter(|b| b.evidence_id == *eid) {
                            ids.insert(b.id.clone());
                        }
                    }
                }
            }
        }
        Ok(ids)
    }
    fn validate_decision_sources(
        &self,
        outcome: DecisionOutcomeV3,
        actual: &[StableId],
    ) -> Result<()> {
        if !actual.contains(&self.claim_id) {
            return Err(M4Error::DecisionSourceMismatch);
        }
        let allowed = |id: &StableId| -> bool {
            if id == &self.claim_id {
                return true;
            }
            match outcome {
                DecisionOutcomeV3::Reject | DecisionOutcomeV3::Exception => false,
                DecisionOutcomeV3::Accept => {
                    self.bindings.iter().any(|binding| {
                        binding.relation == EvidenceRelationV3::Reproduces
                            && (&binding.id == id || &binding.evidence_id == id)
                    }) || self.verifications.iter().any(|verification| {
                        verification.outcome == VerificationOutcomeV3::Passed
                            && &verification.id == id
                            && verification.evidence_ids.iter().any(|evidence_id| {
                                self.bindings.iter().any(|binding| {
                                    binding.relation == EvidenceRelationV3::Reproduces
                                        && binding.evidence_id == *evidence_id
                                })
                            })
                    })
                }
                DecisionOutcomeV3::Defer => self.verifications.iter().any(|verification| {
                    verification.outcome != VerificationOutcomeV3::Passed
                        && (&verification.id == id
                            || verification.evidence_ids.contains(id)
                            || verification.evidence_ids.iter().any(|evidence_id| {
                                self.bindings.iter().any(|binding| {
                                    binding.evidence_id == *evidence_id && &binding.id == id
                                })
                            }))
                }),
            }
        };
        if !actual.iter().all(allowed) {
            return Err(M4Error::DecisionSourceMismatch);
        }
        match outcome {
            DecisionOutcomeV3::Reject | DecisionOutcomeV3::Exception => {
                if actual.len() != 1 {
                    return Err(M4Error::DecisionSourceMismatch);
                }
            }
            DecisionOutcomeV3::Accept => {
                let mut has_passed = false;
                for binding in self
                    .bindings
                    .iter()
                    .filter(|binding| binding.relation == EvidenceRelationV3::Reproduces)
                {
                    if !actual.contains(&binding.id) || !actual.contains(&binding.evidence_id) {
                        return Err(M4Error::DecisionSourceMismatch);
                    }
                    for verification in self.verifications.iter().filter(|verification| {
                        verification.outcome == VerificationOutcomeV3::Passed
                            && verification.evidence_ids.contains(&binding.evidence_id)
                    }) {
                        has_passed = true;
                        if !actual.contains(&verification.id) {
                            return Err(M4Error::DecisionSourceMismatch);
                        }
                    }
                }
                if !has_passed {
                    return Err(validation("Accept requires a Passed reproducing trace"));
                }
            }
            DecisionOutcomeV3::Defer => {
                for verification in self
                    .verifications
                    .iter()
                    .filter(|verification| verification.outcome != VerificationOutcomeV3::Passed)
                {
                    if !actual.contains(&verification.id)
                        || !verification.evidence_ids.iter().all(|evidence_id| {
                            actual.contains(evidence_id)
                                && self
                                    .bindings
                                    .iter()
                                    .filter(|binding| binding.evidence_id == *evidence_id)
                                    .all(|binding| actual.contains(&binding.id))
                        })
                    {
                        return Err(M4Error::DecisionSourceMismatch);
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn record_decision_admitted(
        &mut self,
        scope: &AuthorityScopeV3,
        admission: &TrustedHumanGrantProofV3,
        decision: DecisionV3,
    ) -> Result<()> {
        admission.validate(scope)?;
        self.record_decision_replayed(&ClaimAssessmentScopeV3::from_authority(scope), decision)
    }

    pub(crate) fn record_decision_replayed(
        &mut self,
        scope: &ClaimAssessmentScopeV3,
        decision: DecisionV3,
    ) -> Result<()> {
        if find_sorted_record(&self.decisions, &decision.id, |value| &value.id).is_some() {
            return Err(M4Error::IdCollision {
                id: decision.id.clone(),
            });
        }
        self.validate_claim_scope(scope)?;
        if decision.run_id != scope.run_id
            || decision.snapshot_id != scope.snapshot_id
            || decision.universe_id != scope.universe_id
            || decision.claim_id != scope.claim_id
            || decision.property_id != scope.property_id
        {
            if decision.run_id != scope.run_id {
                return Err(M4Error::RunMismatch {
                    expected: scope.run_id.clone(),
                    actual: decision.run_id.clone(),
                });
            }
            if decision.snapshot_id != scope.snapshot_id {
                return Err(M4Error::SnapshotMismatch {
                    expected: scope.snapshot_id.clone(),
                    actual: decision.snapshot_id.clone(),
                });
            }
            if decision.universe_id != scope.universe_id {
                return Err(M4Error::UniverseMismatch {
                    expected: scope.universe_id.clone(),
                    actual: decision.universe_id.clone(),
                });
            }
            if decision.claim_id != scope.claim_id {
                return Err(M4Error::ClaimMismatch {
                    expected: scope.claim_id.clone(),
                    actual: decision.claim_id.clone(),
                });
            }
            if decision.property_id != scope.property_id {
                return Err(M4Error::PropertyMismatch {
                    expected: scope.property_id.clone(),
                    actual: decision.property_id.clone(),
                });
            }
            return Err(M4Error::AuthorityScopeMismatch);
        }
        self.validate_decision_sources(decision.outcome, &decision.source_ids)?;
        if self.decisions.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment decisions",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        let (next_disposition, next_review_status) = match decision.outcome {
            DecisionOutcomeV3::Accept => {
                if self.baseline() != AssessmentDispositionV3::Supported {
                    return Err(M4Error::IllegalTransition {
                        axis: "disposition",
                        from: "proposed".to_owned(),
                        to: "accepted".to_owned(),
                    });
                }
                (
                    AssessmentDispositionV3::Accepted,
                    AssessmentReviewStatusV3::Accepted,
                )
            }
            DecisionOutcomeV3::Reject => (
                AssessmentDispositionV3::Rejected,
                AssessmentReviewStatusV3::Rejected,
            ),
            DecisionOutcomeV3::Defer | DecisionOutcomeV3::Exception => {
                (self.baseline(), AssessmentReviewStatusV3::HumanReviewed)
            }
        };
        let mut decision_bytes = record_addition_bytes(
            &decision.id,
            size_of::<DecisionV3>(),
            decision.allocated_bytes()?,
            canonical_len(&decision, "assessment decision scratch")?,
        )?;
        checked_memory_add(
            &mut decision_bytes,
            size_of::<StableId>() + decision.id.allocated_bytes(),
        )?;
        self.ensure_working_additions(&[decision_bytes])?;
        reserve_one(&mut self.decisions, "assessment decision reservation")?;
        reserve_one(&mut self.decision_ids, "assessment decision ID reservation")?;
        let decision_id = decision.id.clone();
        insert_sorted_reserved(&mut self.decisions, decision, |value| &value.id);
        insert_id_sorted_reserved(&mut self.decision_ids, decision_id.clone());
        self.active_decision_id = Some(decision_id);
        self.current_finding_id = None;
        self.decision_conflict = false;
        self.disposition = next_disposition;
        self.review_status = next_review_status;
        Ok(())
    }
    #[cfg(test)]
    fn record_decision(&mut self, scope: &AuthorityScopeV3, decision: DecisionV3) -> Result<()> {
        self.record_decision_admitted(scope, &TrustedHumanGrantProofV3::for_test(scope)?, decision)
    }
    pub(crate) fn expected_decision_sources(
        &self,
        outcome: DecisionOutcomeV3,
    ) -> Result<BTreeSet<StableId>> {
        self.exact_sources(outcome)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn mint_decision_from_scope(
        &self,
        scope: &ClaimAssessmentScopeV3,
        policy_revision_hash: ContentHash,
        outcome: DecisionOutcomeV3,
        actor: String,
        authority_id: String,
        rationale: String,
        issued_at: String,
        expires_at: Option<String>,
    ) -> Result<DecisionV3> {
        self.validate_claim_scope(scope)?;
        let sources = self.expected_decision_sources(outcome)?;
        let decision = DecisionV3::build_from_claim_scope(
            scope,
            policy_revision_hash,
            outcome,
            actor,
            authority_id,
            sources,
            rationale,
            issued_at,
            expires_at,
        )?;
        let mut candidate = self.clone();
        candidate.record_decision_replayed(scope, decision.clone())?;
        Ok(decision)
    }
    pub(crate) fn project_finding_admitted(
        &mut self,
        scope: &AuthorityScopeV3,
        admission: &LockedVerificationProofV3,
    ) -> Result<FindingV3> {
        admission.validate(scope)?;
        self.project_finding_from_scope(&ClaimAssessmentScopeV3::from_authority(scope))
    }

    pub(crate) fn project_finding_from_scope(
        &mut self,
        scope: &ClaimAssessmentScopeV3,
    ) -> Result<FindingV3> {
        if scope.property_id != M4_PROPERTY_ID || scope.polarity != ClaimPolarity::IssuePresent {
            return Err(M4Error::FindingNotProjectable);
        }
        if self.current_finding_id.is_some() {
            return Err(M4Error::RedundantFinding);
        }
        self.validate_claim_scope(scope)?;
        let mut projection_request = 0;
        let mut charge_id = |id: &StableId| -> Result<()> {
            checked_memory_add(
                &mut projection_request,
                size_of::<StableId>() + id.allocated_bytes(),
            )
        };
        let has_passed = self
            .verifications
            .iter()
            .any(|value| value.outcome == VerificationOutcomeV3::Passed);
        match self.disposition {
            AssessmentDispositionV3::Accepted => {
                for verification in self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                {
                    charge_id(&verification.id)?;
                }
                for binding in self
                    .bindings
                    .iter()
                    .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                {
                    charge_id(&binding.evidence_id)?;
                }
            }
            AssessmentDispositionV3::Supported if has_passed => {
                for verification in self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                {
                    charge_id(&verification.id)?;
                }
                for binding in self
                    .bindings
                    .iter()
                    .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                {
                    charge_id(&binding.evidence_id)?;
                }
            }
            AssessmentDispositionV3::Proposed | AssessmentDispositionV3::Supported => {
                for evidence in &self.evidence {
                    charge_id(&evidence.id)?;
                }
                for verification in self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome != VerificationOutcomeV3::Passed)
                {
                    charge_id(&verification.id)?;
                }
            }
            AssessmentDispositionV3::Rejected => {}
        }
        let future_status = match self.disposition {
            AssessmentDispositionV3::Accepted => FindingStatusV3::Accepted,
            AssessmentDispositionV3::Rejected => FindingStatusV3::Rejected,
            AssessmentDispositionV3::Supported if has_passed => FindingStatusV3::VerifiedCandidate,
            _ => FindingStatusV3::UnverifiedCandidate,
        };
        let (evidence_count, evidence_id_bytes, verification_count, verification_id_bytes) =
            match future_status {
                FindingStatusV3::Accepted | FindingStatusV3::VerifiedCandidate => {
                    let evidence = self
                        .bindings
                        .iter()
                        .filter(|value| value.relation == EvidenceRelationV3::Reproduces);
                    let verification = self
                        .verifications
                        .iter()
                        .filter(|value| value.outcome == VerificationOutcomeV3::Passed);
                    (
                        evidence.clone().count(),
                        evidence.map(|value| value.evidence_id.as_str().len()).sum(),
                        verification.clone().count(),
                        verification.map(|value| value.id.as_str().len()).sum(),
                    )
                }
                FindingStatusV3::Rejected => (0, 0, 0, 0),
                FindingStatusV3::UnverifiedCandidate => {
                    let verification = self
                        .verifications
                        .iter()
                        .filter(|value| value.outcome != VerificationOutcomeV3::Passed);
                    (
                        self.evidence.len(),
                        self.evidence
                            .iter()
                            .map(|value| value.id.as_str().len())
                            .sum(),
                        verification.clone().count(),
                        verification.map(|value| value.id.as_str().len()).sum(),
                    )
                }
            };
        let previous = self
            .last_finding_id
            .as_ref()
            .and_then(|id| find_sorted_record(&self.findings, id, |value| &value.id));
        let future_decision = matches!(
            future_status,
            FindingStatusV3::Accepted | FindingStatusV3::Rejected
        )
        .then_some(self.active_decision_id.as_ref())
        .flatten();
        let canonical_request = finding_canonical_request(FindingCanonicalShape {
            claim_id: &self.claim_id,
            decision_id: future_decision,
            evidence_count,
            evidence_id_bytes,
            status: future_status,
            supersedes_id: previous.map(|value| &value.id),
            verification_count,
            verification_id_bytes,
        })?;
        let future_id_capacity = "finding:sha256:".len() + 64;
        let mut future_allocated = projection_request;
        for bytes in [
            "reviewgraphen.finding.v3".len(),
            future_id_capacity,
            FINDING_PROJECTION_ID.len(),
            self.claim_id.allocated_bytes(),
            future_decision.map_or(0, StableId::allocated_bytes),
            previous.map_or(0, |value| value.id.allocated_bytes()),
        ] {
            checked_memory_add(&mut future_allocated, bytes)?;
        }
        let mut retained_record_request = future_allocated;
        checked_memory_add(
            &mut retained_record_request,
            size_of::<FindingV3>() + size_of::<StableId>() + future_id_capacity + canonical_request,
        )?;
        let mut mutable_id_clones = 0;
        for _ in 0..2 {
            checked_memory_add(&mut mutable_id_clones, future_id_capacity)?;
        }
        self.ensure_working_additions(&[
            retained_record_request,
            future_allocated,
            mutable_id_clones,
        ])?;
        let (status, eids, vids, did) = match self.disposition {
            AssessmentDispositionV3::Accepted => {
                let evidence_count = self
                    .bindings
                    .iter()
                    .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                    .count();
                let verification_count = self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                    .count();
                (
                    FindingStatusV3::Accepted,
                    clone_ids_requested(
                        self.bindings
                            .iter()
                            .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                            .map(|value| &value.evidence_id),
                        evidence_count,
                        "finding evidence projection",
                    )?,
                    clone_ids_requested(
                        self.verifications
                            .iter()
                            .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                            .map(|value| &value.id),
                        verification_count,
                        "finding verification projection",
                    )?,
                    self.active_decision_id.clone(),
                )
            }
            AssessmentDispositionV3::Rejected => (
                FindingStatusV3::Rejected,
                Vec::new(),
                Vec::new(),
                self.active_decision_id.clone(),
            ),
            AssessmentDispositionV3::Supported if has_passed => {
                let evidence_count = self
                    .bindings
                    .iter()
                    .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                    .count();
                let verification_count = self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                    .count();
                (
                    FindingStatusV3::VerifiedCandidate,
                    clone_ids_requested(
                        self.bindings
                            .iter()
                            .filter(|value| value.relation == EvidenceRelationV3::Reproduces)
                            .map(|value| &value.evidence_id),
                        evidence_count,
                        "finding evidence projection",
                    )?,
                    clone_ids_requested(
                        self.verifications
                            .iter()
                            .filter(|value| value.outcome == VerificationOutcomeV3::Passed)
                            .map(|value| &value.id),
                        verification_count,
                        "finding verification projection",
                    )?,
                    None,
                )
            }
            _ => {
                let verification_count = self
                    .verifications
                    .iter()
                    .filter(|value| value.outcome != VerificationOutcomeV3::Passed)
                    .count();
                (
                    FindingStatusV3::UnverifiedCandidate,
                    clone_ids_requested(
                        self.evidence.iter().map(|value| &value.id),
                        self.evidence.len(),
                        "finding evidence projection",
                    )?,
                    clone_ids_requested(
                        self.verifications
                            .iter()
                            .filter(|value| value.outcome != VerificationOutcomeV3::Passed)
                            .map(|value| &value.id),
                        verification_count,
                        "finding verification projection",
                    )?,
                    None,
                )
            }
        };
        if let Some(old) = previous {
            let grown = old
                .evidence_ids
                .iter()
                .all(|id| eids.binary_search(id).is_ok())
                && old
                    .verification_ids
                    .iter()
                    .all(|id| vids.binary_search(id).is_ok())
                && (eids.len() > old.evidence_ids.len() || vids.len() > old.verification_ids.len());
            let decision_changed = did.is_some() && did != old.decision_id;
            if old.status == status && !grown && !decision_changed {
                return Err(M4Error::RedundantFinding);
            }
        }
        if self.findings.len() >= MAX_SET {
            return Err(M4Error::Incomplete {
                operation: "assessment findings",
                limit: MAX_SET,
                observed: MAX_SET + 1,
            });
        }
        let finding = FindingV3::build(
            self.claim_id.clone(),
            status,
            eids,
            vids,
            did,
            previous.map(|x| x.id.clone()),
        )?;
        if find_sorted_record(&self.findings, &finding.id, |value| &value.id).is_some() {
            return Err(M4Error::IdCollision {
                id: finding.id.clone(),
            });
        }
        reserve_one(&mut self.findings, "assessment finding reservation")?;
        reserve_one(&mut self.finding_ids, "assessment finding ID reservation")?;
        self.current_finding_id = Some(finding.id.clone());
        self.last_finding_id = Some(finding.id.clone());
        insert_id_sorted_reserved(&mut self.finding_ids, finding.id.clone());
        insert_sorted_reserved(&mut self.findings, finding.clone(), |value| &value.id);
        Ok(finding)
    }

    pub(crate) fn record_finding_replayed(
        &mut self,
        scope: &ClaimAssessmentScopeV3,
        finding: FindingV3,
    ) -> Result<()> {
        let mut next = self.clone();
        let expected = next.project_finding_from_scope(scope)?;
        if expected != finding {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        *self = next;
        Ok(())
    }
    #[cfg(test)]
    fn project_finding(&mut self, scope: &AuthorityScopeV3) -> Result<FindingV3> {
        self.project_finding_admitted(scope, &LockedVerificationProofV3::for_test(scope)?)
    }
    fn validate_scope(&self, scope: &AuthorityScopeV3) -> Result<()> {
        scope.validate()?;
        if self.scope != ClaimAssessmentScopeV3::from_authority(scope) {
            return Err(M4Error::AuthorityScopeMismatch);
        }
        Ok(())
    }
    pub const fn disposition(&self) -> AssessmentDispositionV3 {
        self.disposition
    }
    pub const fn review_status(&self) -> AssessmentReviewStatusV3 {
        self.review_status
    }
    pub fn active_decision_id(&self) -> Option<&StableId> {
        self.active_decision_id.as_ref()
    }
    pub fn current_finding_id(&self) -> Option<&StableId> {
        self.current_finding_id.as_ref()
    }
    #[cfg(test)]
    pub(crate) fn configure_active_human_pointers_for_test(
        &mut self,
        active_decision: bool,
        current_finding: bool,
    ) {
        self.active_decision_id = active_decision
            .then(|| self.decision_ids.last().cloned())
            .flatten();
        self.current_finding_id = current_finding
            .then(|| self.finding_ids.last().cloned())
            .flatten();
    }
    pub const fn decision_conflict(&self) -> bool {
        self.decision_conflict
    }
    pub fn evidence_count(&self) -> usize {
        self.evidence.len()
    }
    pub fn binding_ids(&self) -> &[StableId] {
        &self.binding_ids
    }
    pub fn evidence_ids(&self) -> &[StableId] {
        &self.evidence_ids
    }
    pub fn verification_ids(&self) -> &[StableId] {
        &self.verification_ids
    }
    pub fn decision_ids(&self) -> &[StableId] {
        &self.decision_ids
    }
    pub fn finding_ids(&self) -> &[StableId] {
        &self.finding_ids
    }
    pub(crate) fn claim_id(&self) -> &StableId {
        &self.scope.claim_id
    }
    pub(crate) fn run_id(&self) -> &StableId {
        &self.scope.run_id
    }
    pub(crate) fn snapshot_id(&self) -> &StableId {
        &self.scope.snapshot_id
    }
    pub(crate) fn universe_id(&self) -> &StableId {
        &self.scope.universe_id
    }
    pub(crate) fn bindings(&self) -> &[EvidenceBindingV3] {
        &self.bindings
    }
    pub(crate) fn evidence(&self) -> &[EvidenceV3] {
        &self.evidence
    }
    pub(crate) fn verifications(&self) -> &[VerificationV3] {
        &self.verifications
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        bounded_bytes(self, "ClaimAssessmentV3")
    }
    pub fn body_hash(&self) -> Result<ContentHash> {
        body_hash(self)
    }
}

fn evidence_kind_projection_text(value: EvidenceKindV3) -> &'static str {
    match value {
        EvidenceKindV3::StaticFact => "static_fact",
        EvidenceKindV3::TestWitness => "test_witness",
    }
}

impl<'a> crate::BorrowedClaimAssessmentProjectionV4<'a> {
    #[must_use]
    pub fn claim_id(&self) -> &'a StableId {
        self.value.claim_id()
    }
    #[must_use]
    pub const fn disposition(&self) -> AssessmentDispositionV3 {
        self.value.disposition()
    }
    #[must_use]
    pub const fn review_status(&self) -> AssessmentReviewStatusV3 {
        self.value.review_status()
    }
    #[must_use]
    pub fn binding_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.binding_ids())
    }
    #[must_use]
    pub fn evidence_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.evidence_ids())
    }
    #[must_use]
    pub fn verification_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.verification_ids())
    }
    #[must_use]
    pub fn decision_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.decision_ids())
    }
    #[must_use]
    pub fn finding_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.finding_ids())
    }
    #[must_use]
    pub fn active_decision_id(&self) -> Option<&'a StableId> {
        self.value.active_decision_id()
    }
    #[must_use]
    pub fn current_finding_id(&self) -> Option<&'a StableId> {
        self.value.current_finding_id()
    }
    #[must_use]
    pub const fn decision_conflict(&self) -> bool {
        self.value.decision_conflict()
    }
}

fn evidence_observation_projection_text(value: EvidenceObservationV3) -> &'static str {
    match value {
        EvidenceObservationV3::FactPresent => "fact_present",
        EvidenceObservationV3::Witnessed => "witnessed",
    }
}

fn evidence_relation_projection_text(value: EvidenceRelationV3) -> &'static str {
    match value {
        EvidenceRelationV3::Qualifies => "qualifies",
        EvidenceRelationV3::Reproduces => "reproduces",
    }
}

fn verification_outcome_projection_text(value: VerificationOutcomeV3) -> &'static str {
    match value {
        VerificationOutcomeV3::Passed => "passed",
        VerificationOutcomeV3::Inconclusive => "inconclusive",
        VerificationOutcomeV3::Unsupported => "unsupported",
    }
}

fn decision_outcome_projection_text(value: DecisionOutcomeV3) -> &'static str {
    match value {
        DecisionOutcomeV3::Accept => "accept",
        DecisionOutcomeV3::Reject => "reject",
        DecisionOutcomeV3::Defer => "defer",
        DecisionOutcomeV3::Exception => "exception",
    }
}

fn finding_status_projection_text(value: FindingStatusV3) -> &'static str {
    match value {
        FindingStatusV3::UnverifiedCandidate => "unverified_candidate",
        FindingStatusV3::VerifiedCandidate => "verified_candidate",
        FindingStatusV3::Accepted => "accepted",
        FindingStatusV3::Rejected => "rejected",
    }
}

impl<'a> crate::BorrowedEvidenceProjectionV3<'a> {
    pub fn body_hash(&self) -> M4Result<ContentHash> {
        self.value.body_hash()
    }
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        "reviewgraphen.evidence.v3"
    }
    #[must_use]
    pub fn id(&self) -> &'a StableId {
        self.value.id()
    }
    #[must_use]
    pub fn kind(&self) -> &'static str {
        evidence_kind_projection_text(self.value.kind)
    }
    #[must_use]
    pub fn snapshot_id(&self) -> &'a StableId {
        self.value.snapshot_id()
    }
    #[must_use]
    pub fn subject_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.subject_ids())
    }
    #[must_use]
    pub fn descriptor_id(&self) -> &'static str {
        self.value.descriptor().id()
    }
    #[must_use]
    pub fn procedure_version(&self) -> &'static str {
        self.value.procedure().id()
    }
    #[must_use]
    pub fn input_registration_id(&self) -> &'a StableId {
        self.value.input_registration_id()
    }
    #[must_use]
    pub fn output_registration_id(&self) -> &'a StableId {
        self.value.output_registration_id()
    }
    #[must_use]
    pub fn observation(&self) -> &'static str {
        evidence_observation_projection_text(self.value.observation)
    }
}

impl<'a> crate::BorrowedEvidenceBindingProjectionV3<'a> {
    pub fn body_hash(&self) -> M4Result<ContentHash> {
        self.value.body_hash()
    }
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        "reviewgraphen.evidence_binding.v3"
    }
    #[must_use]
    pub fn id(&self) -> &'a StableId {
        self.value.id()
    }
    #[must_use]
    pub fn claim_id(&self) -> &'a StableId {
        self.value.claim_id()
    }
    #[must_use]
    pub fn evidence_id(&self) -> &'a StableId {
        self.value.evidence_id()
    }
    #[must_use]
    pub fn relation(&self) -> &'static str {
        evidence_relation_projection_text(self.value.relation())
    }
    #[must_use]
    pub fn property_id(&self) -> &'a str {
        self.value.property_id()
    }
}

impl<'a> crate::BorrowedVerificationProjectionV3<'a> {
    pub fn body_hash(&self) -> M4Result<ContentHash> {
        self.value.body_hash()
    }
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        "reviewgraphen.verification.v3"
    }
    #[must_use]
    pub fn id(&self) -> &'a StableId {
        self.value.id()
    }
    #[must_use]
    pub fn claim_id(&self) -> &'a StableId {
        self.value.claim_id()
    }
    #[must_use]
    pub fn descriptor_id(&self) -> &'static str {
        self.value.descriptor().id()
    }
    #[must_use]
    pub fn procedure_version(&self) -> &'static str {
        self.value.procedure().id()
    }
    #[must_use]
    pub fn input_registration_id(&self) -> &'a StableId {
        self.value.input_registration_id()
    }
    #[must_use]
    pub fn output_registration_id(&self) -> &'a StableId {
        self.value.output_registration_id()
    }
    #[must_use]
    pub fn evidence_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.evidence_ids())
    }
    #[must_use]
    pub fn outcome(&self) -> &'static str {
        verification_outcome_projection_text(self.value.outcome())
    }
    #[must_use]
    pub fn limitations(&self) -> crate::BorrowedStringSliceIterV4<'a> {
        crate::BorrowedStringSliceIterV4::new(&self.value.limitations)
    }
}

impl<'a> crate::BorrowedDecisionProjectionV3<'a> {
    pub fn body_hash(&self) -> M4Result<ContentHash> {
        self.value.body_hash()
    }
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        "reviewgraphen.human_decision.v3"
    }
    #[must_use]
    pub fn id(&self) -> &'a StableId {
        self.value.id()
    }
    #[must_use]
    pub fn policy_revision_hash(&self) -> &'a ContentHash {
        self.value.policy_revision_hash()
    }
    #[must_use]
    pub fn run_id(&self) -> &'a StableId {
        self.value.run_id()
    }
    #[must_use]
    pub fn universe_id(&self) -> &'a StableId {
        self.value.universe_id()
    }
    #[must_use]
    pub fn claim_id(&self) -> &'a StableId {
        self.value.claim_id()
    }
    #[must_use]
    pub fn property_id(&self) -> &'a str {
        self.value.property_id()
    }
    #[must_use]
    pub fn outcome(&self) -> &'static str {
        decision_outcome_projection_text(self.value.outcome())
    }
    #[must_use]
    pub fn actor(&self) -> &'a str {
        self.value.actor()
    }
    #[must_use]
    pub fn authority_id(&self) -> &'a str {
        self.value.authority_id()
    }
    #[must_use]
    pub fn snapshot_id(&self) -> &'a StableId {
        self.value.snapshot_id()
    }
    #[must_use]
    pub fn source_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.source_ids())
    }
    #[must_use]
    pub fn rationale(&self) -> &'a str {
        &self.value.rationale
    }
    #[must_use]
    pub fn issued_at(&self) -> &'a str {
        self.value.issued_at()
    }
    #[must_use]
    pub fn expires_at(&self) -> Option<&'a str> {
        self.value.expires_at()
    }
}

impl<'a> crate::BorrowedFindingProjectionV3<'a> {
    pub fn body_hash(&self) -> M4Result<ContentHash> {
        self.value.body_hash()
    }
    #[must_use]
    pub const fn schema(&self) -> &'static str {
        "reviewgraphen.finding.v3"
    }
    #[must_use]
    pub fn id(&self) -> &'a StableId {
        self.value.id()
    }
    #[must_use]
    pub const fn projection_descriptor_id(&self) -> &'static str {
        FINDING_PROJECTION_ID
    }
    #[must_use]
    pub fn claim_id(&self) -> &'a StableId {
        self.value.claim_id()
    }
    #[must_use]
    pub fn status(&self) -> &'static str {
        finding_status_projection_text(self.value.status())
    }
    #[must_use]
    pub fn evidence_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.evidence_ids())
    }
    #[must_use]
    pub fn verification_ids(&self) -> crate::BorrowedStableIdSliceIterV4<'a> {
        crate::BorrowedStableIdSliceIterV4::new(self.value.verification_ids())
    }
    #[must_use]
    pub fn decision_id(&self) -> Option<&'a StableId> {
        self.value.decision_id()
    }
    #[must_use]
    pub fn supersedes_finding_id(&self) -> Option<&'a StableId> {
        self.value.supersedes_finding_id()
    }
}

#[cfg(test)]
pub(crate) fn m5_test_passed_assessment() -> (ExecutionClaimV2, ClaimAssessmentV3) {
    let id = |kind: &str, name: &str| StableId::parse(format!("{kind}:{name}")).unwrap();
    let execution_id = id("execution", "m5-proof");
    let obligation_id = id("obligation", "m5-proof");
    let target = id("artifact", "target");
    let source = id("artifact", "source");
    let identity = serde_json::json!({"assumptions":[],"execution_id":execution_id,"obligation_ids":[obligation_id],"polarity":"issue_present","property_id":M4_PROPERTY_ID,"requested_evidence":[],"source_ids":[source],"summary":"duplicate charge","target_refs":[target]});
    let claim_id = derived_id("claim", &identity).unwrap();
    let claim: ExecutionClaimV2 = serde_json::from_value(serde_json::json!({"assumptions":[],"author_kind":"ai","candidate_confidence":0.99,"disposition":"proposed","execution_id":"execution:m5-proof","id":claim_id,"obligation_ids":["obligation:m5-proof"],"polarity":"issue_present","property_id":M4_PROPERTY_ID,"requested_evidence":[],"review_status":"unreviewed","source_ids":["artifact:source"],"summary":"duplicate charge","target_refs":["artifact:target"]})).unwrap();
    let scope = AuthorityScopeV3::new(
        ContentHash::sha256(b"policy"),
        id("run", "m5-proof"),
        id("snapshot", "m5-proof"),
        id("universe", "m5-proof"),
        &claim,
    )
    .unwrap();
    let evidence = EvidenceV3::new(
        &scope,
        EvidenceKindV3::TestWitness,
        BTreeSet::from([id("artifact", "target"), id("test", "double-submit")]),
        VerifierDescriptorV3::FixedFixtureV1,
        id("registration", "m5-proof-in"),
        id("registration", "m5-proof-out"),
        EvidenceObservationV3::Witnessed,
    )
    .unwrap();
    let binding =
        EvidenceBindingV3::new(&scope, &evidence, EvidenceRelationV3::Reproduces).unwrap();
    let verification = VerificationV3::new(
        &scope,
        VerifierDescriptorV3::FixedFixtureV1,
        id("registration", "m5-proof-in"),
        id("registration", "m5-proof-out"),
        BTreeSet::from([evidence.id.clone()]),
        VerificationOutcomeV3::Passed,
        BTreeSet::new(),
    )
    .unwrap();
    let mut assessment = ClaimAssessmentV3::new(&scope);
    assessment
        .record_evidence(&scope, evidence, binding)
        .unwrap();
    assessment
        .record_verification(&scope, verification)
        .unwrap();
    (claim, assessment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_json;
    use serde_json::{Value, json};

    fn with_test_working_budget<T>(
        limit: u64,
        retained_override: Option<u64>,
        operation: impl FnOnce() -> T,
    ) -> (T, Option<u64>, usize) {
        M4_TEST_WORKING_LIMIT.with(|value| value.set(Some(limit)));
        M4_TEST_RETAINED_OVERRIDE.with(|value| value.set(retained_override));
        M4_LAST_WORKING_PEAK.with(|value| value.set(None));
        M4_WORKING_PREFLIGHTS.with(|value| value.set(0));
        let result = operation();
        let peak = M4_LAST_WORKING_PEAK.with(std::cell::Cell::get);
        let preflights = M4_WORKING_PREFLIGHTS.with(std::cell::Cell::get);
        M4_TEST_WORKING_LIMIT.with(|value| value.set(None));
        M4_TEST_RETAINED_OVERRIDE.with(|value| value.set(None));
        M4_LAST_WORKING_PEAK.with(|value| value.set(None));
        M4_WORKING_PREFLIGHTS.with(|value| value.set(0));
        (result, peak, preflights)
    }

    fn id(kind: &str, name: &str) -> StableId {
        StableId::parse(format!("{kind}:{name}")).unwrap()
    }
    fn claim() -> ExecutionClaimV2 {
        claim_with(M4_PROPERTY_ID, ClaimPolarity::IssuePresent)
    }
    fn claim_with(property_id: &str, polarity: ClaimPolarity) -> ExecutionClaimV2 {
        let execution_id = id("execution", "one");
        let obligation_id = id("obligation", "one");
        let target = id("artifact", "target");
        let source = id("artifact", "source");
        let identity = json!({"assumptions":[],"execution_id":execution_id,"obligation_ids":[obligation_id],"polarity":polarity,"property_id":property_id,"requested_evidence":[],"source_ids":[source],"summary":"duplicate charge","target_refs":[target]});
        let claim_id = derived_id("claim", &identity).unwrap();
        serde_json::from_value(json!({"assumptions":[],"author_kind":"ai","candidate_confidence":0.99,"disposition":"proposed","execution_id":"execution:one","id":claim_id,"obligation_ids":["obligation:one"],"polarity":polarity,"property_id":property_id,"requested_evidence":[],"review_status":"unreviewed","source_ids":["artifact:source"],"summary":"duplicate charge","target_refs":["artifact:target"]})).unwrap()
    }
    fn scope() -> AuthorityScopeV3 {
        AuthorityScopeV3::new(
            ContentHash::sha256(b"policy"),
            id("run", "one"),
            id("snapshot", "one"),
            id("universe", "one"),
            &claim(),
        )
        .unwrap()
    }
    fn fixture(
        scope: &AuthorityScopeV3,
        suffix: &str,
    ) -> (EvidenceV3, EvidenceBindingV3, VerificationV3) {
        let evidence = EvidenceV3::new(
            scope,
            EvidenceKindV3::TestWitness,
            BTreeSet::from([id("artifact", "target"), id("test", "double-submit")]),
            VerifierDescriptorV3::FixedFixtureV1,
            id("registration", &format!("in-{suffix}")),
            id("registration", &format!("out-{suffix}")),
            EvidenceObservationV3::Witnessed,
        )
        .unwrap();
        let binding =
            EvidenceBindingV3::new(scope, &evidence, EvidenceRelationV3::Reproduces).unwrap();
        let verification = VerificationV3::new(
            scope,
            VerifierDescriptorV3::FixedFixtureV1,
            id("registration", &format!("in-{suffix}")),
            id("registration", &format!("out-{suffix}")),
            BTreeSet::from([evidence.id.clone()]),
            VerificationOutcomeV3::Passed,
            BTreeSet::new(),
        )
        .unwrap();
        (evidence, binding, verification)
    }

    #[test]
    fn deterministic_ids_hashes_and_strict_decode() {
        let s = scope();
        assert_eq!(
            bounded_bytes(&s, "canonical authority scope").unwrap(),
            canonical_json(&s).unwrap()
        );
        let descriptor =
            AuthorityScopeDescriptorV3::from_json_bytes(&canonical_json(&s).unwrap()).unwrap();
        let mut spliced_descriptor = descriptor.clone();
        spliced_descriptor.result_size += 1;
        assert!(matches!(
            AuthorityScopeV3::admit(
                spliced_descriptor,
                &claim(),
                VerifiedHostScopeProofV3::for_test(&descriptor).unwrap(),
            ),
            Err(M4Error::AuthorityScopeMismatch)
        ));
        let admitted = AuthorityScopeV3::admit(
            descriptor.clone(),
            &claim(),
            VerifiedHostScopeProofV3::for_test(&descriptor).unwrap(),
        )
        .unwrap();
        assert_eq!(admitted, s);
        let mut noncanonical_scope = serde_json::to_value(&s).unwrap();
        noncanonical_scope["target_refs"] = json!(["artifact:target", "artifact:target"]);
        assert!(
            AuthorityScopeDescriptorV3::from_json_bytes(
                &serde_json::to_vec(&noncanonical_scope).unwrap()
            )
            .is_err()
        );
        let (e, _, _) = fixture(&s, "one");
        assert_eq!(
            bounded_bytes(&e, "canonical evidence").unwrap(),
            canonical_json(&e).unwrap()
        );
        let again = EvidenceV3::from_json_bytes(&canonical_json(&e).unwrap()).unwrap();
        assert_eq!(e, again);
        assert_eq!(e.body_hash().unwrap(), again.body_hash().unwrap());
        let encoded = serde_json::to_value(&e).unwrap();
        assert_eq!(encoded["descriptor_id"], FIXTURE_DESCRIPTOR_ID);
        assert_eq!(encoded["procedure_version"], FIXTURE_PROCEDURE_ID);
        assert!(encoded.get("descriptor").is_none());
        let mut value = serde_json::to_value(&e).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), Value::Bool(true));
        assert!(EvidenceV3::from_json_bytes(&serde_json::to_vec(&value).unwrap()).is_err());
        let mut value = serde_json::to_value(&e).unwrap();
        value["subject_ids"] = json!(["test:double-submit", "artifact:target"]);
        assert!(EvidenceV3::from_json_bytes(&serde_json::to_vec(&value).unwrap()).is_err());

        let fixture = FixedFixtureResultV1::new(&s).unwrap();
        assert_eq!(fixture.witness_hash().as_str(), FIXTURE_WITNESS_HASH);
        assert_eq!(
            FixedFixtureResultV1::from_json_bytes(&fixture.canonical_bytes().unwrap()).unwrap(),
            fixture
        );
    }

    #[test]
    fn static_results_never_pass_or_support() {
        let s = scope();
        for a in [
            StaticApplicabilityV1::Absent,
            StaticApplicabilityV1::Unique,
            StaticApplicabilityV1::Ambiguous,
        ] {
            assert_eq!(
                StaticFactResultV1::new(&s, a).unwrap().outcome(),
                VerificationOutcomeV3::Inconclusive
            );
        }
        assert!(matches!(
            EvidenceV3::new(
                &s,
                EvidenceKindV3::StaticFact,
                BTreeSet::from([id("artifact", "target")]),
                VerifierDescriptorV3::StaticFactV1,
                id("registration", "si"),
                id("registration", "so"),
                EvidenceObservationV3::FactPresent,
            ),
            Err(M4Error::AuthorityScopeMismatch)
        ));
    }

    #[test]
    fn static_applicability_is_derived_from_exact_program_obligation_and_claim() {
        let program: ProgramSpace = serde_json::from_str(include_str!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        let obligation: Obligation = serde_json::from_value(json!({
            "id":"obligation:one",
            "target_kind":"artifact",
            "target_refs":["artifact:target"],
            "normalized_target_refs":["artifact:target"],
            "semantic_key":"payment.at_most_once|artifact:target",
            "property_id":M4_PROPERTY_ID,
            "property_version":"1",
            "context_ids":["context:ui-event"],
            "normalized_context_ids":["context:ui-event"],
            "required_capabilities":[],
            "evidence_required":true,
            "accepted_evidence_modes":["static_fact"],
            "applicability_status":"applicable",
            "applicability_reasons":[],
            "qualification_ids":[],
            "weight":1.0,
            "version":{
                "profile":"code-review@1",
                "rule":"payment-at-most-once@1",
                "extractor_set":"sha256:01234567",
                "snapshot":program.snapshot_id()
            },
            "lifecycle":"generated",
            "depends_on":[],
            "normalized_depends_on":[],
            "generator_ids":[],
            "source_ids":["artifact:source"],
            "normalized_source_ids":["artifact:source"]
        }))
        .unwrap();
        let evaluation = evaluate_static_fact_v1(&program, &obligation, &claim()).unwrap();
        assert_eq!(
            evaluation.result().applicability(),
            StaticApplicabilityV1::Absent
        );
        assert!(evaluation.evidence_subject_ids().is_none());
        assert_eq!(
            StaticFactInputV1::from_json_bytes(
                &bounded_bytes(evaluation.input(), "static input test").unwrap()
            )
            .unwrap(),
            *evaluation.input()
        );

        let mut unique_value: Value = serde_json::from_str(include_str!(
            "../../../examples/double-submit-payment/program-space.json"
        ))
        .unwrap();
        unique_value["invariants"][0]["scope_ids"] = json!(["context:ui-event"]);
        let unique_program: ProgramSpace = serde_json::from_value(unique_value.clone()).unwrap();
        let unique = evaluate_static_fact_v1(&unique_program, &obligation, &claim()).unwrap();
        assert_eq!(
            unique.result().applicability(),
            StaticApplicabilityV1::Unique
        );
        assert_eq!(
            unique.evidence_subject_ids().unwrap(),
            &BTreeSet::from([
                id("artifact", "target"),
                id("invariant", "payment-at-most-once")
            ])
        );
        let proposal = unique
            .materialize(
                id("registration", "static-input"),
                id("registration", "static-output"),
            )
            .unwrap();
        assert_eq!(
            proposal.verification.outcome,
            VerificationOutcomeV3::Inconclusive
        );
        assert_eq!(
            proposal.binding.as_ref().unwrap().relation,
            EvidenceRelationV3::Qualifies
        );
        let mut authority = scope();
        authority.snapshot_id = unique_program.snapshot_id().clone();
        let mut static_assessment = ClaimAssessmentV3::new(&authority);
        static_assessment
            .record_static_admitted(
                unique.scope(),
                LockedStaticProofV3::for_test(unique.scope()),
                proposal.clone(),
            )
            .unwrap();
        assert_eq!(
            static_assessment.disposition(),
            AssessmentDispositionV3::Proposed
        );
        let mut spliced = proposal;
        spliced.scope.obligation_id = id("obligation", "spliced");
        assert!(matches!(
            ClaimAssessmentV3::new(&authority).record_static_admitted(
                unique.scope(),
                LockedStaticProofV3::for_test(unique.scope()),
                spliced,
            ),
            Err(M4Error::AuthorityScopeMismatch)
        ));

        let mut second = unique_value["invariants"][0].clone();
        second["id"] = json!("invariant:second");
        unique_value["invariants"]
            .as_array_mut()
            .unwrap()
            .push(second);
        let ambiguous_program: ProgramSpace = serde_json::from_value(unique_value).unwrap();
        let ambiguous = evaluate_static_fact_v1(&ambiguous_program, &obligation, &claim()).unwrap();
        assert_eq!(
            ambiguous.result().applicability(),
            StaticApplicabilityV1::Ambiguous
        );
        assert!(ambiguous.evidence_subject_ids().is_none());

        let mut unsupported_obligation_value = serde_json::to_value(&obligation).unwrap();
        unsupported_obligation_value["property_id"] = json!("other.property");
        let unsupported_obligation: Obligation =
            serde_json::from_value(unsupported_obligation_value).unwrap();
        let unsupported_claim = claim_with("other.property", ClaimPolarity::IssuePresent);
        let unsupported =
            evaluate_static_fact_v1(&program, &unsupported_obligation, &unsupported_claim).unwrap();
        assert_eq!(
            unsupported.result().applicability(),
            StaticApplicabilityV1::UnsupportedProperty
        );
        assert_eq!(
            unsupported.result().outcome(),
            VerificationOutcomeV3::Unsupported
        );

        let mut stale_value = serde_json::to_value(&obligation).unwrap();
        stale_value["version"]["snapshot"] = json!("snapshot:stale");
        let stale: Obligation = serde_json::from_value(stale_value).unwrap();
        assert!(matches!(
            evaluate_static_fact_v1(&program, &stale, &claim()),
            Err(M4Error::SnapshotMismatch { .. })
        ));
    }

    #[test]
    fn transition_conflict_redecision_and_finding_replacement() {
        let s = scope();
        let mut a = ClaimAssessmentV3::new(&s);
        let (e, b, v) = fixture(&s, "one");
        a.record_evidence(&s, e, b).unwrap();
        a.record_verification(&s, v).unwrap();
        let sources = a
            .expected_decision_sources(DecisionOutcomeV3::Accept)
            .unwrap();
        let d = DecisionV3::new(
            &s,
            DecisionOutcomeV3::Accept,
            "human:alice",
            "review-board",
            sources,
            "accept",
            "2026-08-10T00:00:00Z",
            None,
        )
        .unwrap();
        a.record_decision(&s, d).unwrap();
        assert_eq!(
            a.project_finding(&s).unwrap().status(),
            FindingStatusV3::Accepted
        );
        let (e2, b2, v2) = fixture(&s, "two");
        a.record_evidence(&s, e2, b2).unwrap();
        assert!(a.current_finding_id().is_none());
        a.record_verification(&s, v2).unwrap();
        assert!(a.decision_conflict());
        assert_eq!(a.disposition(), AssessmentDispositionV3::Supported);
        let sources = a
            .expected_decision_sources(DecisionOutcomeV3::Accept)
            .unwrap();
        let d2 = DecisionV3::new(
            &s,
            DecisionOutcomeV3::Accept,
            "human:bob",
            "review-board",
            sources,
            "reaccept",
            "2026-08-10T00:00:01Z",
            None,
        )
        .unwrap();
        a.record_decision(&s, d2).unwrap();
        assert!(!a.decision_conflict());
        let f = a.project_finding(&s).unwrap();
        assert_eq!(f.status(), FindingStatusV3::Accepted);
        assert!(a.project_finding(&s).is_err());
    }

    #[test]
    fn cross_scope_and_property_projection_refuse() {
        let s = scope();
        let (e, _, _) = fixture(&s, "x");
        let other = AuthorityScopeV3 {
            snapshot_id: id("snapshot", "other"),
            ..s.clone()
        };
        let sealed = LockedVerificationProofV3::for_test(&s).unwrap();
        assert!(matches!(
            EvidenceV3::new_admitted(
                &other,
                &sealed,
                EvidenceKindV3::TestWitness,
                BTreeSet::from([id("artifact", "target"), id("test", "double-submit")]),
                VerifierDescriptorV3::FixedFixtureV1,
                id("registration", "sealed-in"),
                id("registration", "sealed-out"),
                EvidenceObservationV3::Witnessed,
            ),
            Err(M4Error::AuthorityScopeMismatch)
        ));
        assert!(matches!(
            EvidenceBindingV3::new(&other, &e, EvidenceRelationV3::Reproduces),
            Err(M4Error::SnapshotMismatch { .. })
        ));
        let mut wrong = s.clone();
        wrong.property_id = "other.property".to_owned();
        let mut a = ClaimAssessmentV3::new(&wrong);
        assert!(matches!(
            a.project_finding(&wrong),
            Err(M4Error::FindingNotProjectable)
        ));
    }

    #[test]
    fn reducer_returns_closed_splice_and_collision_variants() {
        let s = scope();
        let mut assessment = ClaimAssessmentV3::new(&s);
        let (evidence, binding, verification) = fixture(&s, "typed");
        assessment
            .record_evidence(&s, evidence.clone(), binding.clone())
            .unwrap();
        let mut other_scope = s.clone();
        other_scope.run_id = id("run", "cross-splice");
        assert!(matches!(
            assessment.record_evidence(&other_scope, evidence, binding),
            Err(M4Error::IdCollision { .. })
        ));
        assessment
            .record_verification(&s, verification.clone())
            .unwrap();
        assert!(matches!(
            assessment.record_verification(&other_scope, verification),
            Err(M4Error::IdCollision { .. })
        ));

        let mut other_run = s.clone();
        other_run.run_id = id("run", "other");
        let sources = BTreeSet::from([s.claim_id.clone()]);
        let decision = DecisionV3::new(
            &other_run,
            DecisionOutcomeV3::Reject,
            "human:alice",
            "review-board",
            sources,
            "reject",
            "2026-08-10T00:00:00Z",
            None,
        )
        .unwrap();
        assert!(matches!(
            assessment.record_decision(&s, decision),
            Err(M4Error::RunMismatch { .. })
        ));

        let valid_decision = DecisionV3::new(
            &s,
            DecisionOutcomeV3::Reject,
            "human:alice",
            "review-board",
            BTreeSet::from([s.claim_id.clone()]),
            "reject exact scope",
            "2026-08-10T00:00:01Z",
            None,
        )
        .unwrap();
        assessment
            .record_decision(&s, valid_decision.clone())
            .unwrap();
        assert!(matches!(
            assessment.record_decision(&other_scope, valid_decision),
            Err(M4Error::IdCollision { .. })
        ));
    }

    #[test]
    fn exact_and_plus_one_count_string_and_canonical_bounds() {
        #[derive(Serialize)]
        struct Blob<'a> {
            x: &'a str,
        }
        let overhead = canonical_json(&Blob { x: "" }).unwrap().len();
        assert_eq!(
            bounded_bytes(
                &Blob {
                    x: &"x".repeat(MAX_RECORD_BYTES - overhead),
                },
                "test canonical bound",
            )
            .unwrap()
            .len(),
            MAX_RECORD_BYTES
        );
        assert!(matches!(
            bounded_bytes(
                &Blob {
                    x: &"x".repeat(MAX_RECORD_BYTES - overhead + 1),
                },
                "test canonical bound",
            ),
            Err(M4Error::Incomplete {
                limit: MAX_RECORD_BYTES,
                observed,
                ..
            }) if observed == MAX_RECORD_BYTES + 1
        ));

        let exact_array = serde_json::to_string(
            &(0..MAX_SET)
                .map(|index| format!("artifact:a{index:03}"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(serde_json::from_str::<BoundedSeq<BoundedStableId, MAX_SET>>(&exact_array).is_ok());
        let over_array = serde_json::to_string(
            &(0..=MAX_SET)
                .map(|index| format!("artifact:a{index:03}"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(serde_json::from_str::<BoundedSeq<BoundedStableId, MAX_SET>>(&over_array).is_err());
        assert!(
            serde_json::from_str::<BoundedSeq<BoundedStableId, 2>>(
                r#"["artifact:b","artifact:a"]"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<BoundedSeq<BoundedStableId, 2>>(
                r#"["artifact:a","artifact:a"]"#
            )
            .is_err()
        );

        let exact_id = format!(
            "artifact:{}",
            "x".repeat(MAX_TRACE_BYTES - "artifact:".len())
        );
        assert_eq!(exact_id.len(), MAX_TRACE_BYTES);
        assert!(serde_json::from_value::<BoundedStableId>(json!(exact_id)).is_ok());
        let over_id = format!(
            "artifact:{}",
            "x".repeat(MAX_TRACE_BYTES + 1 - "artifact:".len())
        );
        assert!(serde_json::from_value::<BoundedStableId>(json!(over_id)).is_err());
        assert!(serde_json::from_value::<BoundedContentHash>(json!(FIXTURE_WITNESS_HASH)).is_ok());
        assert!(
            serde_json::from_value::<BoundedContentHash>(json!("x".repeat(MAX_TRACE_BYTES + 1)))
                .is_err()
        );

        let exact_utf8 = format!("\"{}\"", "é".repeat(MAX_TRACE_BYTES / 2));
        assert!(serde_json::from_str::<BoundedString<MAX_TRACE_BYTES>>(&exact_utf8).is_ok());
        let over_utf8 = format!("\"{}x\"", "é".repeat(MAX_TRACE_BYTES / 2));
        assert!(serde_json::from_str::<BoundedString<MAX_TRACE_BYTES>>(&over_utf8).is_err());

        assert_eq!(
            checked_working_peak(MAX_RETAINED_WORKING_BYTES - 1, [1]).unwrap(),
            MAX_RETAINED_WORKING_BYTES
        );
        assert!(matches!(
            checked_working_peak(MAX_RETAINED_WORKING_BYTES, [1]),
            Err(M4Error::Incomplete { .. })
        ));
        assert!(matches!(
            checked_working_peak(u64::MAX, [1]),
            Err(M4Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));

        let s = scope();
        let mut assessment = ClaimAssessmentV3::new(&s);
        let retained = assessment.retained_bytes().unwrap();
        assessment.evidence_ids.try_reserve_exact(8).unwrap();
        assert_eq!(assessment.retained_bytes().unwrap(), retained);
        let (mut capacity_record, _, _) = fixture(&s, "capacity-model");
        let record_requested = capacity_record.allocated_bytes().unwrap();
        capacity_record.subject_ids.try_reserve_exact(8).unwrap();
        assert_eq!(capacity_record.allocated_bytes().unwrap(), record_requested);
        let before_assessment = assessment.clone();
        assessment
            .ensure_working_additions(&[MAX_RETAINED_WORKING_BYTES - retained])
            .unwrap();
        assert!(matches!(
            assessment.ensure_working_additions(&[MAX_RETAINED_WORKING_BYTES - retained + 1]),
            Err(M4Error::Incomplete { .. })
        ));
        assert!(matches!(
            assessment.ensure_working_additions(&[u64::MAX]),
            Err(M4Error::Incomplete {
                observed: usize::MAX,
                ..
            })
        ));
        assert_eq!(assessment, before_assessment);

        let (bounded_record, _, _) = fixture(&s, "raw-bound");
        let mut exact_record = bounded_bytes(&bounded_record, "raw record bound").unwrap();
        exact_record.resize(MAX_RECORD_BYTES, b' ');
        let before = M4_SERDE_ENTRIES.with(std::cell::Cell::get);
        assert!(EvidenceV3::from_json_bytes(&exact_record).is_ok());
        assert_eq!(M4_SERDE_ENTRIES.with(std::cell::Cell::get), before + 1);
        exact_record.push(b' ');
        let before = M4_SERDE_ENTRIES.with(std::cell::Cell::get);
        assert!(matches!(
            EvidenceV3::from_json_bytes(&exact_record),
            Err(M4Error::Incomplete {
                limit: MAX_RECORD_BYTES,
                observed,
                ..
            }) if observed == MAX_RECORD_BYTES + 1
        ));
        assert_eq!(M4_SERDE_ENTRIES.with(std::cell::Cell::get), before);

        for rejected_before_serde in [
            format!(
                r#"{{"id":"evidence:{}"}}"#,
                "\\u0078".repeat(MAX_TRACE_BYTES + 1 - "evidence:".len())
            ),
            format!(
                r#"{{"result_hash":"{}"}}"#,
                "\\u0078".repeat(MAX_TRACE_BYTES + 1)
            ),
            format!(
                r#"{{"source_ids":[{}]}}"#,
                (0..=MAX_DECISION_SOURCES)
                    .map(|index| format!(r#""artifact:{index:03}""#))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            r#"{"id":"evidence:a","id":"evidence:b"}"#.to_owned(),
            r#"{"source_ids":["artifact:\u0061","artifact:a"]}"#.to_owned(),
            r#"{"source_ids":["artifact:\u0062","artifact:a"]}"#.to_owned(),
        ] {
            let before = M4_SERDE_ENTRIES.with(std::cell::Cell::get);
            assert!(EvidenceV3::from_json_bytes(rejected_before_serde.as_bytes()).is_err());
            assert_eq!(M4_SERDE_ENTRIES.with(std::cell::Cell::get), before);
        }

        let mut subjects = (0..MAX_EVIDENCE_SUBJECTS - 1)
            .map(|index| id("artifact", &format!("s{index:03}")))
            .collect::<BTreeSet<_>>();
        subjects.insert(id("test", "double-submit"));
        let exact_evidence = EvidenceV3::new(
            &s,
            EvidenceKindV3::TestWitness,
            subjects.clone(),
            VerifierDescriptorV3::FixedFixtureV1,
            id("registration", "static-in"),
            id("registration", "static-out"),
            EvidenceObservationV3::Witnessed,
        );
        assert!(exact_evidence.is_ok(), "{exact_evidence:?}");
        let mut over = subjects;
        over.insert(id("artifact", "too-many"));
        assert!(matches!(
            EvidenceV3::new(
                &s,
                EvidenceKindV3::TestWitness,
                over,
                VerifierDescriptorV3::FixedFixtureV1,
                id("registration", "static-in"),
                id("registration", "static-out"),
                EvidenceObservationV3::Witnessed,
            ),
            Err(M4Error::Incomplete {
                operation: "evidence subjects",
                limit: MAX_EVIDENCE_SUBJECTS,
                observed,
            }) if observed == MAX_EVIDENCE_SUBJECTS + 1
        ));

        let exact = DecisionV3::new(
            &s,
            DecisionOutcomeV3::Reject,
            "human:alice",
            "review-board",
            BTreeSet::from([s.claim_id.clone()]),
            "r".repeat(MAX_RATIONALE_BYTES),
            "2026-08-10T00:00:00Z",
            None,
        );
        assert!(exact.is_ok());
        assert!(matches!(
            DecisionV3::new(
                &s,
                DecisionOutcomeV3::Reject,
                "human:alice",
                "review-board",
                BTreeSet::from([s.claim_id.clone()]),
                "r".repeat(MAX_RATIONALE_BYTES + 1),
                "2026-08-10T00:00:00Z",
                None,
            ),
            Err(M4Error::Incomplete {
                operation: "decision rationale",
                limit: MAX_RATIONALE_BYTES,
                ..
            })
        ));
    }

    #[test]
    fn actual_mutation_paths_enforce_exact_plus_one_and_overflow_working_budgets() {
        let s = scope();

        let finding_base = ClaimAssessmentV3::new(&s);
        let mut finding_probe = finding_base.clone();
        let (probe_result, finding_peak, finding_preflights) =
            with_test_working_budget(MAX_RETAINED_WORKING_BYTES, None, || {
                finding_probe.project_finding(&s)
            });
        let projected_finding = probe_result.unwrap();
        assert_eq!(finding_preflights, 1);
        let finding_peak = finding_peak.expect("finding projection must perform one preflight");
        let finding_requested = record_addition_bytes(
            &projected_finding.id,
            size_of::<FindingV3>(),
            projected_finding.allocated_bytes().unwrap(),
            canonical_len(&projected_finding, "finding budget oracle").unwrap(),
        )
        .unwrap()
            + projected_finding.allocated_bytes().unwrap()
            + 2 * u64::try_from(projected_finding.id.allocated_bytes()).unwrap();
        assert_eq!(
            finding_peak,
            finding_base.retained_bytes().unwrap() + finding_requested
        );

        let mut finding_exact = finding_base.clone();
        let (exact_result, _, preflights) =
            with_test_working_budget(finding_peak, None, || finding_exact.project_finding(&s));
        exact_result.unwrap();
        assert_eq!(preflights, 1);

        let mut finding_plus_one = finding_base.clone();
        let finding_before = finding_plus_one.clone();
        let (plus_one_result, _, preflights) =
            with_test_working_budget(finding_peak - 1, None, || {
                finding_plus_one.project_finding(&s)
            });
        assert_eq!(preflights, 1);
        assert!(matches!(
            plus_one_result,
            Err(M4Error::Incomplete {
                operation: "M4 verifier working bytes",
                limit,
                observed,
            }) if limit + 1 == observed
        ));
        assert_eq!(finding_plus_one, finding_before);

        let mut finding_overflow = finding_base.clone();
        let finding_before = finding_overflow.clone();
        let (overflow_result, _, preflights) =
            with_test_working_budget(u64::MAX, Some(u64::MAX), || {
                finding_overflow.project_finding(&s)
            });
        assert_eq!(preflights, 1);
        assert!(matches!(
            overflow_result,
            Err(M4Error::Incomplete {
                operation: "M4 verifier working bytes",
                observed: usize::MAX,
                ..
            })
        ));
        assert_eq!(finding_overflow, finding_before);

        let append_base = ClaimAssessmentV3::new(&s);
        let (evidence, binding, _) = fixture(&s, "budget-append");
        let mut append_probe = append_base.clone();
        let accounting_evidence = evidence.clone();
        let accounting_binding = binding.clone();
        let append_requested = record_addition_bytes(
            &accounting_evidence.id,
            size_of::<EvidenceV3>(),
            accounting_evidence.allocated_bytes().unwrap(),
            canonical_len(&accounting_evidence, "evidence budget oracle").unwrap(),
        )
        .unwrap()
            + record_addition_bytes(
                &accounting_binding.id,
                size_of::<EvidenceBindingV3>(),
                accounting_binding.allocated_bytes().unwrap(),
                canonical_len(&accounting_binding, "binding budget oracle").unwrap(),
            )
            .unwrap();
        let (probe_result, append_peak, preflights) =
            with_test_working_budget(MAX_RETAINED_WORKING_BYTES, None, || {
                append_probe.record_evidence(&s, evidence.clone(), binding.clone())
            });
        probe_result.unwrap();
        assert_eq!(preflights, 1);
        let append_peak = append_peak.expect("record append must perform one preflight");
        assert_eq!(
            append_peak,
            append_base.retained_bytes().unwrap() + append_requested
        );

        let mut append_exact = append_base.clone();
        let (exact_result, _, preflights) = with_test_working_budget(append_peak, None, || {
            append_exact.record_evidence(&s, evidence.clone(), binding.clone())
        });
        exact_result.unwrap();
        assert_eq!(preflights, 1);

        let mut append_plus_one = append_base.clone();
        let append_before = append_plus_one.clone();
        let (plus_one_result, _, preflights) =
            with_test_working_budget(append_peak - 1, None, || {
                append_plus_one.record_evidence(&s, evidence.clone(), binding.clone())
            });
        assert_eq!(preflights, 1);
        assert!(matches!(
            plus_one_result,
            Err(M4Error::Incomplete {
                operation: "M4 verifier working bytes",
                limit,
                observed,
            }) if limit + 1 == observed
        ));
        assert_eq!(append_plus_one, append_before);

        let mut append_overflow = append_base.clone();
        let append_before = append_overflow.clone();
        let (overflow_result, _, preflights) =
            with_test_working_budget(u64::MAX, Some(u64::MAX), || {
                append_overflow.record_evidence(&s, evidence, binding)
            });
        assert_eq!(preflights, 1);
        assert!(matches!(
            overflow_result,
            Err(M4Error::Incomplete {
                operation: "M4 verifier working bytes",
                observed: usize::MAX,
                ..
            })
        ));
        assert_eq!(append_overflow, append_before);
    }

    #[test]
    fn strict_decode_rejects_duplicate_keys_unknown_enums_and_identity_tamper() {
        let s = scope();
        let (evidence, _, _) = fixture(&s, "strict");
        let text = String::from_utf8(canonical_json(&evidence).unwrap()).unwrap();
        let duplicate = text.replacen("{", "{\"schema\":\"reviewgraphen.evidence.v3\",", 1);
        assert!(EvidenceV3::from_json_bytes(duplicate.as_bytes()).is_err());

        let mut unknown_enum = serde_json::to_value(&evidence).unwrap();
        unknown_enum["descriptor_id"] = json!("reviewgraphen.unknown@1");
        assert!(EvidenceV3::from_json_bytes(&serde_json::to_vec(&unknown_enum).unwrap()).is_err());

        let mut tampered_id = serde_json::to_value(&evidence).unwrap();
        tampered_id["id"] = json!("evidence:tampered");
        assert!(EvidenceV3::from_json_bytes(&serde_json::to_vec(&tampered_id).unwrap()).is_err());
    }

    #[test]
    fn static_applicability_and_decision_time_are_exact() {
        let s = scope();
        assert_eq!(
            StaticFactResultV1::new(&s, StaticApplicabilityV1::UnsupportedProperty)
                .unwrap()
                .outcome(),
            VerificationOutcomeV3::Unsupported
        );

        let sources = BTreeSet::from([s.claim_id.clone()]);
        for invalid in [
            "2026-08-10T00:00:00+00:00",
            "2026-08-10T00:00:00.000Z",
            "2026-02-29T00:00:00Z",
            "2026-08-10T24:00:00Z",
        ] {
            assert!(
                DecisionV3::new(
                    &s,
                    DecisionOutcomeV3::Reject,
                    "human:alice",
                    "review-board",
                    sources.clone(),
                    "reject",
                    invalid,
                    None,
                )
                .is_err()
            );
        }
        assert!(
            DecisionV3::new(
                &s,
                DecisionOutcomeV3::Reject,
                "alice",
                "review-board",
                sources.clone(),
                "reject",
                "2026-08-10T00:00:00Z",
                None,
            )
            .is_err()
        );
        assert!(
            DecisionV3::new(
                &s,
                DecisionOutcomeV3::Exception,
                "human:alice",
                "review-board",
                sources,
                "exception",
                "2026-08-10T00:00:00Z",
                Some("2026-08-10T00:00:00Z".to_owned()),
            )
            .is_err()
        );
    }

    #[test]
    fn every_decision_outcome_and_finding_state_is_reduced_without_inference() {
        let s = scope();
        let mut initial = ClaimAssessmentV3::new(&s);
        assert_eq!(initial.disposition(), AssessmentDispositionV3::Proposed);
        assert_eq!(
            initial.project_finding(&s).unwrap().status(),
            FindingStatusV3::UnverifiedCandidate
        );

        for outcome in [
            DecisionOutcomeV3::Reject,
            DecisionOutcomeV3::Defer,
            DecisionOutcomeV3::Exception,
        ] {
            let mut assessment = ClaimAssessmentV3::new(&s);
            let sources = assessment.expected_decision_sources(outcome).unwrap();
            let decision = DecisionV3::new(
                &s,
                outcome,
                "human:alice",
                "review-board",
                sources,
                "decision",
                "2026-08-10T00:00:00Z",
                (outcome == DecisionOutcomeV3::Exception)
                    .then(|| "2026-08-11T00:00:00Z".to_owned()),
            )
            .unwrap();
            assessment.record_decision(&s, decision).unwrap();
            match outcome {
                DecisionOutcomeV3::Reject => {
                    assert_eq!(assessment.disposition(), AssessmentDispositionV3::Rejected);
                    assert_eq!(
                        assessment.project_finding(&s).unwrap().status(),
                        FindingStatusV3::Rejected
                    );
                }
                DecisionOutcomeV3::Defer | DecisionOutcomeV3::Exception => {
                    assert_eq!(assessment.disposition(), AssessmentDispositionV3::Proposed);
                    assert_eq!(
                        assessment.review_status(),
                        AssessmentReviewStatusV3::HumanReviewed
                    );
                }
                DecisionOutcomeV3::Accept => unreachable!(),
            }
            let suffix = match outcome {
                DecisionOutcomeV3::Reject => "reject-conflict",
                DecisionOutcomeV3::Defer => "defer-conflict",
                DecisionOutcomeV3::Exception => "exception-conflict",
                DecisionOutcomeV3::Accept => unreachable!(),
            };
            let (evidence, binding, _) = fixture(&s, suffix);
            assessment.record_evidence(&s, evidence, binding).unwrap();
            assert!(assessment.decision_conflict());
            assert!(assessment.active_decision_id().is_none());
            assert_eq!(assessment.disposition(), AssessmentDispositionV3::Supported);
            assert_eq!(
                assessment.review_status(),
                AssessmentReviewStatusV3::HumanReviewed
            );
        }

        let mut verified = ClaimAssessmentV3::new(&s);
        let (evidence, binding, verification) = fixture(&s, "matrix");
        verified.record_evidence(&s, evidence, binding).unwrap();
        assert_eq!(verified.disposition(), AssessmentDispositionV3::Supported);
        verified.record_verification(&s, verification).unwrap();
        assert_eq!(
            verified.project_finding(&s).unwrap().status(),
            FindingStatusV3::VerifiedCandidate
        );
    }
}
