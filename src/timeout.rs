#![cfg(not(feature = "no_std"))]
use crate::{Waiter, WaiterError};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct TimeoutWaiter {
    timeout: Duration,
    start: Option<Instant>,
}
impl TimeoutWaiter {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            start: None,
        }
    }
}
impl Waiter for TimeoutWaiter {
    fn restart(&mut self) -> Result<(), WaiterError> {
        let _ = self.start.ok_or(WaiterError::NotStarted)?;
        self.start = Some(Instant::now());
        Ok(())
    }
    fn start(&mut self) {
        self.start = Some(Instant::now());
    }
    fn wait(&self) -> Result<(), WaiterError> {
        let start = self.start.ok_or(WaiterError::NotStarted)?;
        if start.elapsed() > self.timeout {
            Err(WaiterError::Timeout)
        } else {
            Ok(())
        }
    }
}
