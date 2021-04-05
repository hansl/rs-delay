#![cfg_attr(feature = "no_std", no_std)]
extern crate alloc;

use alloc::boxed::Box;
use core::cell::RefCell;

#[cfg(not(feature = "no_std"))]
use core::time::Duration;

#[cfg(feature = "async")]
use core::{
    future::{self, Future},
    pin::Pin,
};

#[cfg(not(feature = "no_std"))]
mod throttle;
#[cfg(not(feature = "no_std"))]
pub use throttle::ExponentialBackoffWaiter;
#[cfg(not(feature = "no_std"))]
pub use throttle::ThrottleWaiter;

#[cfg(not(feature = "no_std"))]
mod timeout;
#[cfg(not(feature = "no_std"))]
pub use timeout::TimeoutWaiter;

mod compose;
pub use compose::DelayComposer;

#[cfg(test)]
mod tests;

/// An error happened while waiting.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum WaiterError {
    Timeout,
    NotStarted,
}

/// A waiter trait, that can be used for executing a delay. Waiters need to be
/// multi-threaded and cloneable.
/// A waiter should not be reused twice.
pub trait Waiter: WaiterClone + Send {
    /// Restart the wait timer.
    /// Call after starting the waiter otherwise returns an error.
    fn restart(&mut self) -> Result<(), WaiterError> {
        Ok(())
    }

    /// Called when we start the waiting cycle.
    fn start(&mut self) {}

    /// Called at each cycle of the waiting cycle.
    /// Call after starting the waiter otherwise returns an error.
    fn wait(&self) -> Result<(), WaiterError>;

    /// Async version of [wait]. By default call the blocking wait. Should be implemented
    /// to be non-blocking.
    #[cfg(feature = "async")]
    fn async_wait(&self) -> Pin<Box<dyn Future<Output = Result<(), WaiterError>>>> {
        Box::pin(future::ready(self.wait()))
    }
}

/// Implement cloning for Waiter. This requires a temporary (unfortunately public) trait
/// to allow an implementation of box clone on Waiter.
pub trait WaiterClone {
    fn clone_box(&self) -> Box<dyn Waiter>;
}

/// Implement WaiterClone for every T that implements Clone.
impl<T> WaiterClone for T
where
    T: 'static + Waiter + Clone,
{
    fn clone_box(&self) -> Box<dyn Waiter> {
        Box::new(self.clone())
    }
}

/// Finally, a Box of Waiter implements clone by calling the WaiterClone::clone_box method.
impl Clone for Box<dyn Waiter> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

/// A Delay struct that encapsulates a Waiter.
///
/// To use this class, first create an instance of it by either calling a method
/// on it (like [Delay::timeout]) or create a builder and add multiple Waiters.
/// Then when you're ready to start a process that needs to wait, use the [start()]
/// function. Every wait period, call the [wait()] function on it (it may block the
/// thread).
#[derive(Clone)]
pub struct Delay {
    inner: Box<dyn Waiter>,
}

impl Delay {
    fn from(inner: Box<dyn Waiter>) -> Self {
        Delay { inner }
    }

    /// A Delay that never waits. This can hog resources, so careful.
    pub fn instant() -> Self {
        Self::from(Box::new(InstantWaiter {}))
    }

    /// A Delay that doesn't wait, but times out after a while.
    #[cfg(not(feature = "no_std"))]
    pub fn timeout(timeout: Duration) -> Self {
        Self::from(Box::new(TimeoutWaiter::new(timeout)))
    }

    /// A Delay that times out after waiting a certain number of times.
    pub fn count_timeout(count: u64) -> Self {
        Self::from(Box::new(CountTimeoutWaiter::new(count)))
    }

    /// A delay that waits every wait() call for a certain time.
    #[cfg(not(feature = "no_std"))]
    pub fn throttle(throttle: Duration) -> Self {
        Self::from(Box::new(ThrottleWaiter::new(throttle)))
    }

    /// A delay that recalculate a wait time every wait() calls and exponentially waits.
    /// The calculation is new_wait_time = max(current_wait_time * multiplier, cap).
    #[cfg(not(feature = "no_std"))]
    pub fn exponential_backoff_capped(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        Self::from(Box::new(ExponentialBackoffWaiter::new(
            initial, multiplier, cap,
        )))
    }

    /// A delay that recalculate a wait time every wait() calls and exponentially waits.
    /// The calculation is new_wait_time = current_wait_time * multiplier.
    /// There is no limit for this backoff.
    #[cfg(not(feature = "no_std"))]
    pub fn exponential_backoff(initial: Duration, multiplier: f32) -> Self {
        Self::exponential_backoff_capped(initial, multiplier, Duration::from_secs(std::u64::MAX))
    }

    /// Call a function every tick, expecting some kind of side effect (e.g. a progress
    /// bar).
    pub fn side_effect<F>(function: F) -> Self
    where
        F: 'static + Sync + Send + Clone + Fn() -> Result<(), WaiterError>,
    {
        Self::from(Box::new(SideEffectWaiter::new(function)))
    }

    pub fn builder() -> DelayBuilder {
        DelayBuilder { inner: None }
    }
}

impl Waiter for Delay {
    fn restart(&mut self) -> Result<(), WaiterError> {
        self.inner.restart()
    }
    fn start(&mut self) {
        self.inner.start()
    }
    fn wait(&self) -> Result<(), WaiterError> {
        self.inner.wait()
    }

    #[cfg(feature = "async")]
    fn async_wait(&self) -> Pin<Box<dyn Future<Output = Result<(), WaiterError>>>> {
        self.inner.async_wait()
    }
}

pub struct DelayBuilder {
    inner: Option<Delay>,
}
impl DelayBuilder {
    /// Add a delay to the current builder. If a builder implements multiple delays, they
    /// will run sequentially, so if you have 2 Throttle delays, they will wait one after the
    /// other. This composer can be used though with a Throttle and a Timeout to throttle a
    /// thread, and error out if it throttles too long.
    pub fn with(mut self, other: Delay) -> Self {
        self.inner = Some(match self.inner.take() {
            None => other,
            Some(w) => Delay::from(Box::new(DelayComposer::new(w, other))),
        });
        self
    }
    #[cfg(not(feature = "no_std"))]
    pub fn timeout(self, timeout: Duration) -> Self {
        self.with(Delay::timeout(timeout))
    }
    #[cfg(not(feature = "no_std"))]
    pub fn throttle(self, throttle: Duration) -> Self {
        self.with(Delay::throttle(throttle))
    }
    #[cfg(not(feature = "no_std"))]
    pub fn exponential_backoff(self, initial: Duration, multiplier: f32) -> Self {
        self.with(Delay::exponential_backoff(initial, multiplier))
    }
    #[cfg(not(feature = "no_std"))]
    pub fn exponential_backoff_capped(
        self,
        initial: Duration,
        multiplier: f32,
        cap: Duration,
    ) -> Self {
        self.with(Delay::exponential_backoff_capped(initial, multiplier, cap))
    }
    pub fn side_effect<F>(self, function: F) -> Self
    where
        F: 'static + Sync + Send + Clone + Fn() -> Result<(), WaiterError>,
    {
        self.with(Delay::side_effect(function))
    }
    pub fn build(mut self) -> Delay {
        self.inner.take().unwrap_or_else(Delay::instant)
    }
}

#[derive(Clone)]
struct InstantWaiter {}
impl Waiter for InstantWaiter {
    fn wait(&self) -> Result<(), WaiterError> {
        Ok(())
    }
}

#[derive(Clone)]
struct CountTimeoutWaiter {
    max_count: u64,
    count: Option<RefCell<u64>>,
}
impl CountTimeoutWaiter {
    pub fn new(max_count: u64) -> Self {
        CountTimeoutWaiter {
            max_count,
            count: None,
        }
    }
}
impl Waiter for CountTimeoutWaiter {
    fn restart(&mut self) -> Result<(), WaiterError> {
        let count = self.count.as_ref().ok_or(WaiterError::NotStarted)?;
        count.replace(0);
        Ok(())
    }

    fn start(&mut self) {
        self.count = Some(RefCell::new(0));
    }

    fn wait(&self) -> Result<(), WaiterError> {
        let count = self.count.as_ref().ok_or(WaiterError::NotStarted)?;
        let current = *count.borrow() + 1;
        count.replace(current);

        if current >= self.max_count {
            Err(WaiterError::Timeout)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
struct SideEffectWaiter<F>
where
    F: 'static + Sync + Send + Clone + Fn() -> Result<(), WaiterError>,
{
    function: F,
}

impl<F> SideEffectWaiter<F>
where
    F: 'static + Sync + Send + Clone + Fn() -> Result<(), WaiterError>,
{
    pub fn new(function: F) -> Self {
        Self { function }
    }
}
impl<F> Waiter for SideEffectWaiter<F>
where
    F: 'static + Sync + Send + Clone + Fn() -> Result<(), WaiterError>,
{
    fn wait(&self) -> Result<(), WaiterError> {
        (self.function)()
    }
}
