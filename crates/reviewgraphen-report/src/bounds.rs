//! Deterministic resource accounting for source-bound report construction.
//!
//! This module deliberately works from explicit measurements.  It neither
//! estimates allocations from JSON length nor consults allocator statistics:
//! callers measure retained journal/index bytes and report ownership while
//! building typed report records, then ask this module to apply ADR 0018's
//! checked peak formulas before reserving or serializing output.

use serde::{Serialize, ser};
use thiserror::Error;

/// Inclusive construction limits for `reviewgraphen.review.report.v2`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReportLimits {
    pub executions: u64,
    pub raw_registrations: u64,
    pub claims: u64,
    /// M4 evidence rows in a v3 report.
    pub evidence: u64,
    pub evidence_bindings: u64,
    pub verifications: u64,
    pub decisions: u64,
    pub findings: u64,
    pub claim_assessments: u64,
    pub obstructions: u64,
    pub views: u64,
    pub information_loss_records: u64,
    pub rows: u64,
    pub canonical_bytes: u64,
    pub working_bytes: u64,
}

impl Default for ReportLimits {
    fn default() -> Self {
        Self {
            executions: 8_192,
            raw_registrations: 8_192,
            claims: 131_072,
            evidence: 131_072,
            evidence_bindings: 131_072,
            verifications: 131_072,
            decisions: 131_072,
            findings: 131_072,
            claim_assessments: 131_072,
            obstructions: 8_192,
            views: 3,
            information_loss_records: 4_096,
            rows: 200_000,
            canonical_bytes: 67_108_864,
            working_bytes: 268_435_456,
        }
    }
}

/// Every ADR-0021 v3 report-row class which makes up the report-row denominator.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportCounts {
    pub registrations: u64,
    pub executions: u64,
    pub claims: u64,
    pub evidence: u64,
    pub evidence_bindings: u64,
    pub verifications: u64,
    pub decisions: u64,
    pub findings: u64,
    pub claim_assessments: u64,
    pub obstructions: u64,
    pub views: u64,
    pub information_loss_records: u64,
}

impl ReportCounts {
    /// The exact report-row count from ADR 0018 §10.
    pub fn rows(self, limit: u64) -> Result<u64, BoundsError> {
        checked_sum(
            "report_rows",
            limit,
            [
                self.registrations,
                self.executions,
                self.claims,
                self.evidence,
                self.evidence_bindings,
                self.verifications,
                self.decisions,
                self.findings,
                self.claim_assessments,
                self.obstructions,
                self.views,
                self.information_loss_records,
            ],
        )
    }
}

/// Explicit retained-byte measurements used by the two normative peak checks.
///
/// `journal_bytes` is `J`, `index_bytes` is `I`, `reserved_report_bytes` is
/// `Rr`, `realized_report_bytes` is `R`, `largest_record_bytes` is `S`, and
/// `canonical_report_bytes` is `O` in ADR 0018 §10.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReportAccounting {
    pub journal_bytes: u64,
    pub index_bytes: u64,
    pub reserved_report_bytes: u64,
    pub realized_report_bytes: u64,
    pub largest_record_bytes: u64,
    pub canonical_report_bytes: u64,
}

/// A typed refusal that callers map directly to `ReportError::Incomplete`.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum BoundsError {
    #[error("report construction {operation} exceeded bound {limit} (observed {observed})")]
    Incomplete {
        operation: &'static str,
        limit: u64,
        observed: u64,
    },
}

/// Error returned while measuring the deterministic ownership charge of a
/// serializable typed record.  This is intentionally not a JSON tree: report
/// construction must not materialize an unbounded `serde_json::Value` just to
/// account for it.
#[derive(Debug, Error)]
pub enum OwnershipError {
    #[error("report ownership charge overflowed u64")]
    Overflow,
    #[error("report ownership measurement cannot represent a non-string map key")]
    NonStringMapKey,
    #[error("report ownership measurement failed: {0}")]
    Message(String),
}

impl ser::Error for OwnershipError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Message(message.to_string())
    }
}

/// A borrow-only builder for the ADR 0018 ownership model.
///
/// A source-bound report builder can walk journal/index/request rows and add
/// the *logical* report fields in their final JSON shape without allocating
/// the final `Report`, a report-owned `Vec`, or a raw CAS buffer.  Every
/// method has the same charge as [`ownership_charge`], so the completed shape
/// can be checked against the final typed report before serialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LogicalCharge {
    total: u64,
}

impl LogicalCharge {
    pub const fn new() -> Self {
        Self { total: 0 }
    }

    pub const fn bytes(self) -> u64 {
        self.total
    }

    /// Adds the recursive charge of a borrowed typed JSON value.
    pub fn serialized<T: Serialize>(&mut self, value: &T) -> Result<(), OwnershipError> {
        value.serialize(&mut OwnershipSerializer { charge: self })
    }

    /// Adds a JSON string scalar using its actual UTF-8 byte length.
    pub fn string(&mut self, value: &str) -> Result<(), OwnershipError> {
        self.add(u64::try_from(value.len()).map_err(|_| OwnershipError::Overflow)?)
    }

    /// Adds a JSON Boolean scalar.
    pub fn boolean(&mut self) -> Result<(), OwnershipError> {
        self.add(1)
    }

    /// Adds a JSON integer or float scalar.
    pub fn number(&mut self) -> Result<(), OwnershipError> {
        self.add(8)
    }

    /// Adds a JSON null scalar, which owns no charge in this model.
    pub const fn null(&mut self) {}

    /// Adds a list member's slot and then its already computed child charge.
    pub fn list_charge(&mut self, child: LogicalCharge) -> Result<(), OwnershipError> {
        self.add(8)?;
        self.add(child.total)
    }

    /// Adds a list member's slot and measures a borrowed child in place.
    pub fn list_serialized<T: Serialize>(&mut self, value: &T) -> Result<(), OwnershipError> {
        self.add(8)?;
        self.serialized(value)
    }

    /// Adds a JSON object entry, key, and an already computed child charge.
    pub fn field_charge(&mut self, key: &str, child: LogicalCharge) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.add(child.total)
    }

    /// Adds a JSON object entry, key, and a borrowed typed child value.
    pub fn field_serialized<T: Serialize>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.serialized(value)
    }

    /// Adds an object entry and string scalar without allocating an owned
    /// copy of either source value.
    pub fn field_string(&mut self, key: &str, value: &str) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.string(value)
    }

    /// Adds an object entry and Boolean scalar.
    pub fn field_boolean(&mut self, key: &str) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.boolean()
    }

    /// Adds an object entry and numeric scalar.
    pub fn field_number(&mut self, key: &str) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.number()
    }

    /// Adds an object entry and null scalar.
    pub fn field_null(&mut self, key: &str) -> Result<(), OwnershipError> {
        self.object_entry(key)?;
        self.null();
        Ok(())
    }

    fn add(&mut self, value: u64) -> Result<(), OwnershipError> {
        self.total = self
            .total
            .checked_add(value)
            .ok_or(OwnershipError::Overflow)?;
        Ok(())
    }

    fn object_entry(&mut self, key: &str) -> Result<(), OwnershipError> {
        self.add(16)?;
        self.string(key)
    }
}

/// Returns the ADR 0018 ownership charge for a typed report value.
///
/// It charges actual UTF-8 bytes for every string and object key, 8 bytes per
/// list slot and numeric scalar, 16 bytes per object entry, one byte per
/// Boolean, and zero for null.  The report's maps are JSON objects and
/// therefore must have string keys.  Overflow is reported before any caller
/// can reserve based on the result.
pub fn ownership_charge<T: Serialize>(value: &T) -> Result<u64, OwnershipError> {
    let mut charge = LogicalCharge::new();
    charge.serialized(value)?;
    Ok(charge.bytes())
}

struct OwnershipSerializer<'a> {
    charge: &'a mut LogicalCharge,
}

impl OwnershipSerializer<'_> {
    fn add(&mut self, value: u64) -> Result<(), OwnershipError> {
        self.charge.add(value)
    }

    fn string(&mut self, value: &str) -> Result<(), OwnershipError> {
        self.charge.string(value)
    }

    fn map_key(&mut self, key: &str) -> Result<(), OwnershipError> {
        self.charge.object_entry(key)
    }
}

impl<'a, 'charge> ser::Serializer for &'a mut OwnershipSerializer<'charge> {
    type Ok = ();
    type Error = OwnershipError;
    type SerializeSeq = Sequence<'a, 'charge>;
    type SerializeTuple = Sequence<'a, 'charge>;
    type SerializeTupleStruct = Sequence<'a, 'charge>;
    type SerializeTupleVariant = Sequence<'a, 'charge>;
    type SerializeMap = Map<'a, 'charge>;
    type SerializeStruct = Struct<'a, 'charge>;
    type SerializeStructVariant = Struct<'a, 'charge>;

    fn serialize_bool(self, _: bool) -> Result<Self::Ok, Self::Error> {
        self.add(1)
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_i128(self, _: i128) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_u128(self, _: u128) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, Self::Error> {
        self.add(8)
    }
    fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
        let mut encoded = [0_u8; 4];
        self.string(value.encode_utf8(&mut encoded))
    }
    fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
        self.string(value)
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
        let slots = u64::try_from(value.len()).map_err(|_| OwnershipError::Overflow)?;
        // JSON encodes bytes as a list of numeric values, so each member owns
        // both its list slot and numeric scalar charge.
        self.add(slots.checked_mul(16).ok_or(OwnershipError::Overflow)?)
    }
    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }
    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        value: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.string(value)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        self.add(16)?;
        self.map_key(variant)?;
        value.serialize(self)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Ok(Sequence { serializer: self })
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(Sequence { serializer: self })
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(Sequence { serializer: self })
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.add(16)?;
        self.map_key(variant)?;
        Ok(Sequence { serializer: self })
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(Map { serializer: self })
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(Struct { serializer: self })
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.add(16)?;
        self.map_key(variant)?;
        Ok(Struct { serializer: self })
    }
}

struct Sequence<'a, 'charge> {
    serializer: &'a mut OwnershipSerializer<'charge>,
}

impl ser::SerializeSeq for Sequence<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        self.serializer.add(8)?;
        value.serialize(&mut *self.serializer)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}
impl ser::SerializeTuple for Sequence<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}
impl ser::SerializeTupleStruct for Sequence<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}
impl ser::SerializeTupleVariant for Sequence<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        ser::SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

struct Map<'a, 'charge> {
    serializer: &'a mut OwnershipSerializer<'charge>,
}
impl ser::SerializeMap for Map<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Self::Error> {
        key.serialize(MapKeySerializer {
            serializer: self.serializer,
        })
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Self::Error> {
        value.serialize(&mut *self.serializer)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

struct MapKeySerializer<'a, 'charge> {
    serializer: &'a mut OwnershipSerializer<'charge>,
}
impl ser::Serializer for MapKeySerializer<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    type SerializeSeq = ser::Impossible<(), OwnershipError>;
    type SerializeTuple = ser::Impossible<(), OwnershipError>;
    type SerializeTupleStruct = ser::Impossible<(), OwnershipError>;
    type SerializeTupleVariant = ser::Impossible<(), OwnershipError>;
    type SerializeMap = ser::Impossible<(), OwnershipError>;
    type SerializeStruct = ser::Impossible<(), OwnershipError>;
    type SerializeStructVariant = ser::Impossible<(), OwnershipError>;
    fn serialize_str(self, value: &str) -> Result<(), OwnershipError> {
        self.serializer.map_key(value)
    }
    fn serialize_char(self, value: char) -> Result<(), OwnershipError> {
        let mut encoded = [0_u8; 4];
        self.serializer.map_key(value.encode_utf8(&mut encoded))
    }
    fn serialize_bool(self, _: bool) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_i8(self, _: i8) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_i16(self, _: i16) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_i32(self, _: i32) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_i64(self, _: i64) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_i128(self, _: i128) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_u8(self, _: u8) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_u16(self, _: u16) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_u32(self, _: u32) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_u64(self, _: u64) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_u128(self, _: u128) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_f32(self, _: f32) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_f64(self, _: f64) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_none(self) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_unit(self) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: &T,
    ) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<(), OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, OwnershipError> {
        Err(OwnershipError::NonStringMapKey)
    }
}

struct Struct<'a, 'charge> {
    serializer: &'a mut OwnershipSerializer<'charge>,
}
impl ser::SerializeStruct for Struct<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        self.serializer.map_key(key)?;
        value.serialize(&mut *self.serializer)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}
impl ser::SerializeStructVariant for Struct<'_, '_> {
    type Ok = ();
    type Error = OwnershipError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        ser::SerializeStruct::serialize_field(self, key, value)
    }
    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl ReportLimits {
    /// Checks all count-based limits plus the pre-reservation peak `J + I + Rr`.
    ///
    /// Call this before report-owned vector/string reservations or output
    /// allocation.  The caller may only pass a reservation charge computed
    /// from the deterministic ownership model.
    pub fn preflight(
        self,
        counts: ReportCounts,
        journal_bytes: u64,
        index_bytes: u64,
        reserved_report_bytes: u64,
    ) -> Result<(), BoundsError> {
        self.check_counts(counts)?;
        let projection_peak = checked_sum(
            "projection_peak",
            self.working_bytes,
            [journal_bytes, index_bytes, reserved_report_bytes],
        )?;
        if projection_peak > self.working_bytes {
            return Err(incomplete(
                "projection_peak",
                self.working_bytes,
                projection_peak,
            ));
        }
        Ok(())
    }

    /// Checks output bytes and the serialization peak `J + I + R + S + O`.
    ///
    /// This must run before allocating the output buffer or returning any
    /// report bytes.
    pub fn check_serialization(
        self,
        journal_bytes: u64,
        index_bytes: u64,
        realized_report_bytes: u64,
        largest_record_bytes: u64,
        canonical_report_bytes: u64,
    ) -> Result<(), BoundsError> {
        if canonical_report_bytes > self.canonical_bytes {
            return Err(incomplete(
                "canonical_report_bytes",
                self.canonical_bytes,
                canonical_report_bytes,
            ));
        }
        let serialization_peak = checked_sum(
            "serialization_peak",
            self.working_bytes,
            [
                journal_bytes,
                index_bytes,
                realized_report_bytes,
                largest_record_bytes,
                canonical_report_bytes,
            ],
        )?;
        if serialization_peak > self.working_bytes {
            return Err(incomplete(
                "serialization_peak",
                self.working_bytes,
                serialization_peak,
            ));
        }
        Ok(())
    }

    /// Applies the full report accounting contract to explicit measurements.
    pub fn check(
        self,
        counts: ReportCounts,
        accounting: ReportAccounting,
    ) -> Result<(), BoundsError> {
        self.preflight(
            counts,
            accounting.journal_bytes,
            accounting.index_bytes,
            accounting.reserved_report_bytes,
        )?;
        self.check_serialization(
            accounting.journal_bytes,
            accounting.index_bytes,
            accounting.realized_report_bytes,
            accounting.largest_record_bytes,
            accounting.canonical_report_bytes,
        )
    }

    fn check_counts(self, counts: ReportCounts) -> Result<(), BoundsError> {
        check_limit("executions", self.executions, counts.executions)?;
        check_limit(
            "raw_registrations",
            self.raw_registrations,
            counts.registrations,
        )?;
        check_limit("claims", self.claims, counts.claims)?;
        check_limit("evidence", self.evidence, counts.evidence)?;
        check_limit(
            "evidence_bindings",
            self.evidence_bindings,
            counts.evidence_bindings,
        )?;
        check_limit("verifications", self.verifications, counts.verifications)?;
        check_limit("decisions", self.decisions, counts.decisions)?;
        check_limit("findings", self.findings, counts.findings)?;
        check_limit(
            "claim_assessments",
            self.claim_assessments,
            counts.claim_assessments,
        )?;
        check_limit("obstructions", self.obstructions, counts.obstructions)?;
        check_limit("projection_views", self.views, counts.views)?;
        check_limit(
            "information_loss_records",
            self.information_loss_records,
            counts.information_loss_records,
        )?;
        let rows = counts.rows(self.rows)?;
        check_limit("report_rows", self.rows, rows)
    }
}

fn check_limit(operation: &'static str, limit: u64, observed: u64) -> Result<(), BoundsError> {
    if observed > limit {
        return Err(incomplete(operation, limit, observed));
    }
    Ok(())
}

fn checked_sum<const N: usize>(
    operation: &'static str,
    limit: u64,
    values: [u64; N],
) -> Result<u64, BoundsError> {
    values.into_iter().try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .ok_or_else(|| incomplete(operation, limit, u64::MAX))
    })
}

fn incomplete(operation: &'static str, limit: u64, observed: u64) -> BoundsError {
    BoundsError::Incomplete {
        operation,
        limit,
        observed,
    }
}

#[cfg(test)]
mod tests {
    use super::{BoundsError, ReportAccounting, ReportCounts, ReportLimits, ownership_charge};
    use serde::Serialize;
    use std::collections::BTreeMap;

    fn limits() -> ReportLimits {
        ReportLimits {
            executions: 2,
            raw_registrations: 2,
            claims: 3,
            evidence: 3,
            evidence_bindings: 3,
            verifications: 3,
            decisions: 3,
            findings: 3,
            claim_assessments: 3,
            obstructions: 2,
            views: 3,
            information_loss_records: 4,
            rows: 16,
            canonical_bytes: 32,
            working_bytes: 100,
        }
    }

    #[test]
    fn defaults_are_the_adr_contract() {
        assert_eq!(
            ReportLimits::default(),
            ReportLimits {
                executions: 8_192,
                raw_registrations: 8_192,
                claims: 131_072,
                evidence: 131_072,
                evidence_bindings: 131_072,
                verifications: 131_072,
                decisions: 131_072,
                findings: 131_072,
                claim_assessments: 131_072,
                obstructions: 8_192,
                views: 3,
                information_loss_records: 4_096,
                rows: 200_000,
                canonical_bytes: 67_108_864,
                working_bytes: 268_435_456,
            }
        );
    }

    #[test]
    fn every_record_count_accepts_exact_and_refuses_plus_one() {
        let base = limits();
        let cases = [
            (
                "executions",
                ReportCounts {
                    executions: 2,
                    ..Default::default()
                },
                ReportCounts {
                    executions: 3,
                    ..Default::default()
                },
            ),
            (
                "raw_registrations",
                ReportCounts {
                    registrations: 2,
                    ..Default::default()
                },
                ReportCounts {
                    registrations: 3,
                    ..Default::default()
                },
            ),
            (
                "claims",
                ReportCounts {
                    claims: 3,
                    ..Default::default()
                },
                ReportCounts {
                    claims: 4,
                    ..Default::default()
                },
            ),
            (
                "evidence",
                ReportCounts {
                    evidence: 3,
                    ..Default::default()
                },
                ReportCounts {
                    evidence: 4,
                    ..Default::default()
                },
            ),
            (
                "evidence_bindings",
                ReportCounts {
                    evidence_bindings: 3,
                    ..Default::default()
                },
                ReportCounts {
                    evidence_bindings: 4,
                    ..Default::default()
                },
            ),
            (
                "verifications",
                ReportCounts {
                    verifications: 3,
                    ..Default::default()
                },
                ReportCounts {
                    verifications: 4,
                    ..Default::default()
                },
            ),
            (
                "decisions",
                ReportCounts {
                    decisions: 3,
                    ..Default::default()
                },
                ReportCounts {
                    decisions: 4,
                    ..Default::default()
                },
            ),
            (
                "findings",
                ReportCounts {
                    findings: 3,
                    ..Default::default()
                },
                ReportCounts {
                    findings: 4,
                    ..Default::default()
                },
            ),
            (
                "claim_assessments",
                ReportCounts {
                    claim_assessments: 3,
                    ..Default::default()
                },
                ReportCounts {
                    claim_assessments: 4,
                    ..Default::default()
                },
            ),
            (
                "obstructions",
                ReportCounts {
                    obstructions: 2,
                    ..Default::default()
                },
                ReportCounts {
                    obstructions: 3,
                    ..Default::default()
                },
            ),
            (
                "projection_views",
                ReportCounts {
                    views: 3,
                    ..Default::default()
                },
                ReportCounts {
                    views: 4,
                    ..Default::default()
                },
            ),
            (
                "information_loss_records",
                ReportCounts {
                    information_loss_records: 4,
                    ..Default::default()
                },
                ReportCounts {
                    information_loss_records: 5,
                    ..Default::default()
                },
            ),
        ];
        for (operation, exact, plus_one) in cases {
            assert!(base.preflight(exact, 0, 0, 0).is_ok(), "{operation}");
            assert_eq!(
                base.preflight(plus_one, 0, 0, 0),
                Err(BoundsError::Incomplete {
                    operation,
                    limit: match operation {
                        "claims" | "evidence" | "evidence_bindings" | "verifications"
                        | "decisions" | "findings" | "claim_assessments" => 3,
                        "projection_views" => 3,
                        "information_loss_records" => 4,
                        _ => 2,
                    },
                    observed: match operation {
                        "claims" | "evidence" | "evidence_bindings" | "verifications"
                        | "decisions" | "findings" | "claim_assessments" => 4,
                        "projection_views" => 4,
                        "information_loss_records" => 5,
                        _ => 3,
                    },
                })
            );
        }
    }

    #[test]
    fn rows_are_the_checked_sum_of_every_v3_record_class() {
        let exact = ReportCounts {
            registrations: 2,
            executions: 2,
            claims: 3,
            obstructions: 2,
            views: 3,
            information_loss_records: 4,
            ..ReportCounts::default()
        };
        assert!(limits().preflight(exact, 0, 0, 0).is_ok());
        let plus_one = ReportCounts {
            information_loss_records: 5,
            ..exact
        };
        assert_eq!(
            limits().preflight(plus_one, 0, 0, 0),
            Err(BoundsError::Incomplete {
                operation: "information_loss_records",
                limit: 4,
                observed: 5,
            })
        );

        let row_limited = ReportLimits {
            rows: 15,
            ..limits()
        };
        assert_eq!(
            row_limited.preflight(exact, 0, 0, 0),
            Err(BoundsError::Incomplete {
                operation: "report_rows",
                limit: 15,
                observed: 16,
            })
        );
    }

    #[test]
    fn canonical_bytes_accept_exact_and_refuse_plus_one() {
        let exact = ReportAccounting {
            canonical_report_bytes: 32,
            ..Default::default()
        };
        assert!(limits().check(ReportCounts::default(), exact).is_ok());
        let plus_one = ReportAccounting {
            canonical_report_bytes: 33,
            ..Default::default()
        };
        assert_eq!(
            limits().check(ReportCounts::default(), plus_one),
            Err(BoundsError::Incomplete {
                operation: "canonical_report_bytes",
                limit: 32,
                observed: 33,
            })
        );
    }

    #[test]
    fn projection_and_serialization_peaks_are_separate_checked_formulas() {
        let exact_projection = ReportAccounting {
            journal_bytes: 20,
            index_bytes: 30,
            reserved_report_bytes: 50,
            ..Default::default()
        };
        assert!(
            limits()
                .preflight(ReportCounts::default(), 20, 30, 50)
                .is_ok()
        );
        let projection_plus_one = ReportAccounting {
            reserved_report_bytes: 51,
            ..exact_projection
        };
        assert_eq!(
            limits().preflight(ReportCounts::default(), 20, 30, 51),
            Err(BoundsError::Incomplete {
                operation: "projection_peak",
                limit: 100,
                observed: 101,
            })
        );

        let exact_serialization = ReportAccounting {
            journal_bytes: 20,
            index_bytes: 20,
            realized_report_bytes: 20,
            largest_record_bytes: 20,
            canonical_report_bytes: 20,
            ..Default::default()
        };
        assert!(limits().check_serialization(20, 20, 20, 20, 20).is_ok());
        let serialization_plus_one = ReportAccounting {
            canonical_report_bytes: 21,
            ..exact_serialization
        };
        assert_eq!(
            limits().check_serialization(20, 20, 20, 20, 21),
            Err(BoundsError::Incomplete {
                operation: "serialization_peak",
                limit: 100,
                observed: 101,
            })
        );

        assert_ne!(exact_projection, projection_plus_one);
        assert_ne!(exact_serialization, serialization_plus_one);
    }

    #[test]
    fn checked_overflow_reports_u64_max_without_reserving() {
        let error = ReportLimits {
            executions: u64::MAX,
            raw_registrations: u64::MAX,
            rows: u64::MAX,
            ..limits()
        }
        .preflight(
            ReportCounts {
                registrations: u64::MAX,
                executions: 1,
                ..Default::default()
            },
            0,
            0,
            0,
        )
        .unwrap_err();
        assert_eq!(
            error,
            BoundsError::Incomplete {
                operation: "report_rows",
                limit: u64::MAX,
                observed: u64::MAX,
            }
        );

        let error = ReportLimits {
            working_bytes: 7,
            ..limits()
        }
        .preflight(ReportCounts::default(), u64::MAX, 1, 0)
        .unwrap_err();
        assert_eq!(
            error,
            BoundsError::Incomplete {
                operation: "projection_peak",
                limit: 7,
                observed: u64::MAX,
            }
        );
    }

    #[test]
    fn ownership_charge_uses_utf8_and_the_normative_slot_entry_scalar_rules() {
        #[derive(Serialize)]
        struct Sample {
            label: String,
            list: Vec<Scalar>,
            optional: Option<String>,
            map: BTreeMap<String, String>,
        }
        #[derive(Serialize)]
        #[serde(untagged)]
        enum Scalar {
            Bool(bool),
            Number(u64),
        }
        let sample = Sample {
            label: "界".into(),
            list: vec![Scalar::Bool(true), Scalar::Number(7)],
            optional: None,
            map: BTreeMap::from([("k".into(), "x".into())]),
        };
        // label: 16 + 5 + 3; list: 16 + 4 + (8 + 1) + (8 + 8);
        // optional: 16 + 8 + 0; map: 16 + 3 + (16 + 1 + 1).
        assert_eq!(ownership_charge(&sample).unwrap(), 130);

        // Bytes serialize as JSON numeric list members: 8 for each slot and
        // 8 for each numeric scalar.
        struct RawBytes;
        impl Serialize for RawBytes {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(&[1, 2])
            }
        }
        assert_eq!(ownership_charge(&RawBytes).unwrap(), 32);
    }

    #[test]
    fn logical_shape_matches_the_typed_final_value_without_allocating_that_value() {
        #[derive(Serialize)]
        struct FinalShape {
            name: String,
            values: Vec<u64>,
            enabled: bool,
            absent: Option<String>,
        }
        let final_value = FinalShape {
            name: "review".into(),
            values: vec![7, 9],
            enabled: true,
            absent: None,
        };

        let mut values = super::LogicalCharge::new();
        values.list_serialized(&7_u64).unwrap();
        values.list_serialized(&9_u64).unwrap();
        let mut shape = super::LogicalCharge::new();
        shape.field_string("name", "review").unwrap();
        shape.field_charge("values", values).unwrap();
        shape.field_boolean("enabled").unwrap();
        shape.field_null("absent").unwrap();
        assert_eq!(shape.bytes(), ownership_charge(&final_value).unwrap());

        let mut primitive = super::LogicalCharge::new();
        primitive.boolean().unwrap();
        primitive.number().unwrap();
        primitive.null();
        let mut list = super::LogicalCharge::new();
        list.list_charge(primitive).unwrap();
        assert_eq!(list.bytes(), 17);

        let metadata = BTreeMap::from([("rule".to_owned(), "v1".to_owned())]);
        let mut borrowed = super::LogicalCharge::new();
        borrowed.field_serialized("metadata", &metadata).unwrap();
        assert_eq!(borrowed.bytes(), 16 + 8 + 16 + 4 + 2);
    }
}
