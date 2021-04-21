#![cfg(test)]
#![cfg(not(feature = "no_std"))]
use crate::{Delay, Waiter};
use std::time::{Duration, Instant};

#[test]
fn throttle_works() {
    let start = Instant::now();

    let mut waiter = Delay::throttle(Duration::from_millis(50));
    waiter.start();
    waiter.wait().unwrap();

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
}

#[test]
fn counter_works() {
    let mut waiter = Delay::count_timeout(3);
    waiter.start();

    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_ok());
    assert!(waiter.wait().is_err());
    assert!(waiter.wait().is_err());
}

#[test]
fn clone_works() {
    let mut waiter1 = Delay::count_timeout(3);
    let mut waiter2 = waiter1.clone();

    waiter1.start();
    assert!(waiter1.wait().is_ok());
    assert!(waiter1.wait().is_ok());
    assert!(waiter1.wait().is_ok());
    assert!(waiter1.wait().is_err());

    waiter2.start();
    assert!(waiter2.wait().is_ok());
    assert!(waiter2.wait().is_ok());
    assert!(waiter2.wait().is_ok());
    assert!(waiter2.wait().is_err());
}

#[test]
fn cannot_restart_or_wait_without_start() {
    let mut waiter = Delay::timeout(Duration::from_millis(50));
    assert!(waiter.wait().is_err());
    assert!(waiter.restart().is_err());
    waiter.start();
    assert!(waiter.wait().is_ok());
    assert!(waiter.restart().is_ok());

    let mut waiter1 = Delay::count_timeout(3);
    assert!(waiter1.wait().is_err());
    assert!(waiter1.restart().is_err());
    waiter1.start();
    assert!(waiter1.wait().is_ok());
    assert!(waiter1.restart().is_ok());
}

#[test]
fn restart_works() {
    let mut waiter = Delay::timeout(Duration::from_millis(50));
    waiter.start();

    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(10));
    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(50));
    assert!(waiter.wait().is_err());

    assert!(waiter.restart().is_ok());

    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(10));
    assert!(waiter.wait().is_ok());
    std::thread::sleep(Duration::from_millis(50));
    assert!(waiter.wait().is_err());
}

#[test]
fn can_send_between_threads() {
    let mut waiter = Delay::count_timeout(5);
    let (tx, rx) = std::sync::mpsc::channel();
    let (tx_end, rx_end) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        waiter.start();

        while let Some(x) = rx.recv().unwrap_or(None) {
            for _i in 1..x {
                waiter.wait().unwrap();
            }
        }

        tx_end.send(()).unwrap();
    });

    tx.send(Some(4)).unwrap();
    tx.send(Some(1)).unwrap();
    tx.send(None).unwrap();

    rx_end.recv_timeout(Duration::from_millis(100)).unwrap();
}

#[tokio::test]
async fn works_as_async() {
    let mut waiter = Delay::count_timeout(5);

    let (tx, mut rx) = tokio::sync::mpsc::channel(5);
    let (tx_end, mut rx_end) = tokio::sync::mpsc::channel(1);

    tokio::task::spawn(async move {
        waiter.start();

        while let Some(x) = rx.recv().await.unwrap_or(None) {
            for _i in 1..x {
                waiter.async_wait().await.unwrap();
            }
        }

        tx_end.send(()).await.unwrap();
    });

    tx.send(Some(4)).await.unwrap();
    tx.send(Some(1)).await.unwrap();
    tx.send(None).await.unwrap();

    rx_end.recv().await.unwrap();
}
