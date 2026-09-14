//! ReviewGraphen runtime surfaces.
//!
//! Generic, provider-free orchestration is portable across supported Unix
//! hosts. Durable event-store orchestration remains Linux-only because its
//! descriptor and create-only publication contract relies on Linux APIs.

pub mod diagnostics;
pub mod generic;

#[cfg(target_os = "linux")]
pub mod fixed_offline;
#[cfg(target_os = "linux")]
pub mod m4_verification;
#[cfg(target_os = "linux")]
pub mod m5_gluing;

#[cfg(target_os = "linux")]
mod durable;
#[cfg(target_os = "linux")]
pub use durable::*;
