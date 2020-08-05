use std::cell::RefCell;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

/// An error happened while waiting.
#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub enum WaiterError {
    Timeout,
}

/// A waiter trait, that can be used for executing a delay. Waiters need to be
/// multi-threaded and cloneable.
/// A waiter should not be reused twice.
pub trait Waiter: WaiterClone + Send {
    /// Called when we start the waiting cycle.
    fn start(&mut self) {}

    /// Called at each cycle of the waiting cycle.
    fn wait(&self) -> Result<(), WaiterError>;
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
    pub fn timeout(timeout: Duration) -> Self {
        Self::from(Box::new(TimeoutWaiter::new(timeout)))
    }

    /// A Delay that times out after waiting a certain number of times.
    pub fn count_timeout(count: u64) -> Self {
        Self::from(Box::new(CountTimeoutWaiter::new(count)))
    }

    /// A delay that waits every wait() call for a certain time.
    pub fn throttle(throttle: Duration) -> Self {
        Self::from(Box::new(ThrottleWaiter::new(throttle)))
    }

    /// A delay that recalculate a wait time every wait() calls and exponentially waits.
    /// The calculation is new_wait_time = max(current_wait_time * multiplier, cap).
    pub fn exponential_backoff_capped(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        Self::from(Box::new(ExponentialBackoffWaiter::new(
            initial, multiplier, cap,
        )))
    }

    /// A delay that recalculate a wait time every wait() calls and exponentially waits.
    /// The calculation is new_wait_time = current_wait_time * multiplier.
    /// There is no limit for this backoff.
    pub fn exponential_backoff(initial: Duration, multiplier: f32) -> Self {
        Self::exponential_backoff_capped(initial, multiplier, Duration::from_secs(std::u64::MAX))
    }

    pub fn builder() -> DelayBuilder {
        DelayBuilder { inner: None }
    }
}

impl Waiter for Delay {
    fn start(&mut self) {
        self.inner.start()
    }
    fn wait(&self) -> Result<(), WaiterError> {
        self.inner.wait()
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
    pub fn timeout(self, timeout: Duration) -> Self {
        self.with(Delay::timeout(timeout))
    }
    pub fn throttle(self, throttle: Duration) -> Self {
        self.with(Delay::throttle(throttle))
    }
    pub fn exponential_backoff(self, initial: Duration, multiplier: f32) -> Self {
        self.with(Delay::exponential_backoff(initial, multiplier))
    }
    pub fn exponential_backoff_capped(
        self,
        initial: Duration,
        multiplier: f32,
        cap: Duration,
    ) -> Self {
        self.with(Delay::exponential_backoff_capped(initial, multiplier, cap))
    }
    pub fn build(mut self) -> Delay {
        self.inner.take().unwrap_or_else(Delay::instant)
    }
}

#[derive(Clone)]
struct DelayComposer {
    a: Delay,
    b: Delay,
}
impl DelayComposer {
    fn new(a: Delay, b: Delay) -> Self {
        Self { a, b }
    }
}
impl Waiter for DelayComposer {
    fn start(&mut self) {
        self.a.start();
        self.b.start();
    }
    fn wait(&self) -> Result<(), WaiterError> {
        self.a.wait()?;
        self.b.wait()?;
        Ok(())
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
struct TimeoutWaiter {
    timeout: Duration,
    start: Instant,
}
impl TimeoutWaiter {
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            start: Instant::now(),
        }
    }
}
impl Waiter for TimeoutWaiter {
    fn start(&mut self) {
        self.start = Instant::now();
    }
    fn wait(&self) -> Result<(), WaiterError> {
        if self.start.elapsed() > self.timeout {
            Err(WaiterError::Timeout)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
struct CountTimeoutWaiter {
    max_count: u64,
    count: RefCell<u64>,
}
impl CountTimeoutWaiter {
    pub fn new(max_count: u64) -> Self {
        CountTimeoutWaiter {
            max_count,
            count: RefCell::new(0),
        }
    }
}
impl Waiter for CountTimeoutWaiter {
    fn start(&mut self) {
        self.count = RefCell::new(0);
    }

    fn wait(&self) -> Result<(), WaiterError> {
        let current = *self.count.borrow() + 1;
        self.count.replace(current);

        if current >= self.max_count {
            Err(WaiterError::Timeout)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone)]
struct ThrottleWaiter {
    throttle: Duration,
}
impl ThrottleWaiter {
    pub fn new(throttle: Duration) -> Self {
        Self { throttle }
    }
}
impl Waiter for ThrottleWaiter {
    fn wait(&self) -> Result<(), WaiterError> {
        std::thread::sleep(self.throttle);

        Ok(())
    }
}

#[derive(Clone)]
struct ExponentialBackoffWaiter {
    next: RefCell<Duration>,
    initial: Duration,
    multiplier: f32,
    cap: Duration,
}
impl ExponentialBackoffWaiter {
    pub fn new(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        ExponentialBackoffWaiter {
            next: RefCell::new(initial),
            initial,
            multiplier,
            cap,
        }
    }
}
impl Waiter for ExponentialBackoffWaiter {
    fn start(&mut self) {
        self.next = RefCell::new(self.initial);
    }

    fn wait(&self) -> Result<(), WaiterError> {
        let current = *self.next.borrow();
        let current_nsec = current.as_nanos() as f32;

        // Find the next throttle.
        let mut next_duration = Duration::from_nanos((current_nsec * self.multiplier) as u64);
        if next_duration > self.cap {
            next_duration = self.cap;
        }

        self.next.replace(next_duration);

        std::thread::sleep(current);

        Ok(())
    }
}
