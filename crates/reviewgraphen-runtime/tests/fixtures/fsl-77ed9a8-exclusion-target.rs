// Minimal source slice from rust/fslc/tests/typed_agreement/engines.rs at
// 77ed9a81cb3fb2072de2b497656d72730b582a9c. The test path is material: under
// rust.production.v1 this accepted caller/callee pair is a profile exclusion.
use std::future::Future;

pub fn block_on<F: Future>(future: F) -> F::Output {
    todo!("target fixture retains only the direct-call shape: {future:?}")
}

pub fn compare_agreement() {
    let _ = block_on(async {});
}
