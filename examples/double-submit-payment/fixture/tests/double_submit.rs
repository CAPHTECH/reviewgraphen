use std::{sync::{Arc, Barrier}, thread};

use double_submit_payment_fixture::{CheckoutController, PaymentRepository, StripeClient};

#[test]
fn double_submit_reproduces_two_external_charges() {
    let gateway = Arc::new(StripeClient::default());
    let payments = Arc::new(PaymentRepository::new(Arc::clone(&gateway)));
    let validation_boundary = Arc::new(Barrier::new(2));
    let controller = Arc::new(CheckoutController::new(validation_boundary, payments));

    let left = {
        let controller = Arc::clone(&controller);
        thread::spawn(move || controller.submit("order-42"))
    };
    let right = {
        let controller = Arc::clone(&controller);
        thread::spawn(move || controller.submit("order-42"))
    };

    left.join().expect("left submit thread must finish");
    right.join().expect("right submit thread must finish");

    // This is a counterexample/reproduction test, so it passes when the bug is
    // observed. The business invariant would require this value to be 1.
    assert_eq!(gateway.charge_calls(), 2);
}
