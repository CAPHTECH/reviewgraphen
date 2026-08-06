use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Barrier,
};

use crate::PaymentRepository;

#[derive(Debug)]
pub struct CheckoutController {
    loading: AtomicBool,
    validation_boundary: Arc<Barrier>,
    payments: Arc<PaymentRepository>,
}

impl CheckoutController {
    pub fn new(
        validation_boundary: Arc<Barrier>,
        payments: Arc<PaymentRepository>,
    ) -> Self {
        Self {
            loading: AtomicBool::new(false),
            validation_boundary,
            payments,
        }
    }

    pub fn submit(&self, order_id: &str) {
        if self.loading.load(Ordering::SeqCst) {
            return;
        }

        // Models an awaited validation step. Both calls can pass the guard
        // before either one writes `loading = true`.
        self.validation_boundary.wait();
        self.loading.store(true, Ordering::SeqCst);

        self.payments.charge(order_id);
        self.loading.store(false, Ordering::SeqCst);
    }
}
