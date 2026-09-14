//! ReviewGraphen report projections.
//!
//! Generic non-authority reports and accounting bounds are portable. Durable
//! Store-bound report projections remain Linux-only with the Store contract.

mod bounds;
mod generic_non_authority;

pub use bounds::{
    BoundsError, LogicalCharge, OwnershipError, ReportAccounting, ReportCounts, ReportLimits,
    ownership_charge,
};
pub use generic_non_authority::{
    GenericHumanReport, GenericHumanReportError, generate_generic_human_report,
    generate_generic_human_report_v3, generate_generic_human_report_v4,
    validate_generic_human_report, validate_generic_human_report_v3,
};

#[cfg(target_os = "linux")]
mod report_v3;
#[cfg(target_os = "linux")]
mod report_v4;
#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod report_v5;

#[cfg(target_os = "linux")]
mod durable;
#[cfg(target_os = "linux")]
pub use durable::*;
#[cfg(target_os = "linux")]
pub(crate) use durable::{bounded_json_bytes, json_encoded_len, validate_metadata_hash};
#[cfg(target_os = "linux")]
pub use report_v3::{ReportRequestV3, generate_v3, generate_v3_with_limits};
#[cfg(target_os = "linux")]
pub use report_v4::{
    M5BundleIncomplete, ReportLimitsV4, ReportRequestV4, ReportV4SemanticError, generate_v4,
    generate_v4_with_limits, validate_v4_semantics,
};
#[cfg(target_os = "linux")]
pub use report_v5::{
    ReportAccountingV5, ReportCountsV5, ReportLimitsV5, ReportRequestV5, ReportV5SemanticError,
    generate_v5, generate_v5_with_limits, validate_v5_semantics,
};
