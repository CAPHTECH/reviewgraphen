use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Debug, Default)]
pub struct StripeClient {
    charge_calls: AtomicUsize,
}

impl StripeClient {
    pub fn charge(&self, _order_id: &str) {
        self.charge_calls.fetch_add(1, Ordering::SeqCst);
    }

    pub fn charge_calls(&self) -> usize {
        self.charge_calls.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
pub struct PaymentRepository {
    gateway: Arc<StripeClient>,
}

impl PaymentRepository {
    pub fn new(gateway: Arc<StripeClient>) -> Self {
        Self { gateway }
    }

    /// Intentionally unsafe contract: no idempotency key or deduplication.
    pub fn charge(&self, order_id: &str) {
        self.gateway.charge(order_id);
    }
}
