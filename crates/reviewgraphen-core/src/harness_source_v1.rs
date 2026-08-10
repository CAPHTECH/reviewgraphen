//! Checked-in, fixed in-process semantics for the M4 duplicate-submit fixture.
//!
//! This is deliberately not a reference to `examples/`: it models the one MVP
//! interleaving locally. Both submissions observe `loading == false` before
//! either continuation commits the guard, so both reach the external charge.
//! It owns no path, process, environment, network, or authority surface.

/// Stable identity of this exact checked-in harness source.
pub(crate) const HARNESS_ID: &str = "reviewgraphen.double_submit_harness@1";

/// Stable revision of this exact checked-in harness source.
pub(crate) const HARNESS_REVISION: &str = "1";

#[derive(Clone, Copy)]
struct SubmitAttempt {
    observed_loading: bool,
}

impl SubmitAttempt {
    fn starts_when(loading: bool) -> Self {
        Self {
            observed_loading: loading,
        }
    }

    const fn reaches_charge(self) -> bool {
        !self.observed_loading
    }
}

/// Runs the fixed MVP interleaving and emits its canonical witness JSON.
pub(crate) fn run_duplicate_submit_harness() -> Vec<u8> {
    let loading_before_continuations = false;
    let first = SubmitAttempt::starts_when(loading_before_continuations);
    let second = SubmitAttempt::starts_when(loading_before_continuations);
    let charge_count = u8::from(first.reaches_charge()) + u8::from(second.reaches_charge());

    let mut witness = br#"{"charge_count":"#.to_vec();
    witness.extend_from_slice(charge_count.to_string().as_bytes());
    witness.extend_from_slice(
        br#","expected_max":1,"outcome":"witnessed","schema":"reviewgraphen.test_witness_result.v1","test_artifact_id":"test:double-submit"}"#,
    );
    witness
}
