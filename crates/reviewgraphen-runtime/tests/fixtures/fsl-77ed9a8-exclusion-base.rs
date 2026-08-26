// Minimal source slice from rust/fslc/tests/typed_agreement/engines.rs at
// 265e7e020ba8210b728ed984cd971d06285c8cf3. The test path is material: under
// rust.production.v1 this accepted caller/callee pair is a profile exclusion.
use std::future::Future;

pub fn block_on<F: Future>(future: F) -> F::Output {
    todo!("base fixture retains only the direct-call shape: {future:?}")
}

pub fn run_agreement() {
    let _ = block_on(async {});
}
