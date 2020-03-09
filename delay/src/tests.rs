#![cfg(test)]
use crate::{Delay, Waiter};
use std::time::{Duration, Instant};

#[test]
fn throttle_works() {
    let start = Instant::now();

    let mut waiter = Delay::throttle(Duration::from_millis(50));
    waiter.start();
    waiter.wait().unwrap();
    waiter.stop();

    assert!(Instant::now().duration_since(start).as_millis() >= 50);
}

#[test]
fn timeout_works() {
    let mut waiter = Delay::timeout(Duration::from_millis(50));
    waiter.start();

    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(10));
    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(50));
    assert!(waiter.wait().is_err());

    waiter.stop();
}

#[test]
fn counter_works() {
    let mut waiter = Delay::count_timeout(3);
    waiter.start();

    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_err());
    assert!(waiter.wait().is_err());

    waiter.stop();
}
