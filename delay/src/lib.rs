use std::sync::{Arc, Mutex};
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
pub trait Waiter {
    fn start(&mut self) {}
    fn wait(&self) -> Result<(), WaiterError>;
    fn stop(&self) {}
}

/// A Delay struct that encapsulates a Waiter.
///
/// To use this class, first create an instance of it by either calling a method
/// on it (like [Delay::timeout]) or create a builder and add multiple Waiters.
/// Then when you're ready to start a process that needs to wait, use the [start()]
/// function. Every wait period, call the [wait()] function on it (it may block the
/// thread). When you're done, you may call [stop()].
///
/// Waiters can be reused and re-started, but most would expect to have [stop()]
/// called on them when you do, to free any additional resources.
#[derive(Clone)]
pub struct Delay {
    inner: Arc<dyn Waiter>,
}

impl Delay {
    fn from(inner: Arc<dyn Waiter>) -> Self {
        Delay { inner }
    }

    /// A Delay that never waits. This can hog resources, so careful.
    pub fn instant() -> Self {
        Self::from(Arc::new(InstantWaiter {}))
    }

    /// A Delay that doesn't wait, but times out after a while.
    pub fn timeout(timeout: Duration) -> Self {
        Self::from(Arc::new(TimeoutWaiter::new(timeout)))
    }

    /// A Delay that times out after waiting a certain number of times.
    pub fn count_timeout(count: u64) -> Self {
        Self::from(Arc::new(CountTimeoutWaiter::new(count)))
    }

    /// A delay that waits every wait() call for a certain time.
    pub fn throttle(throttle: Duration) -> Self {
        Self::from(Arc::new(ThrottleWaiter::new(throttle)))
    }

    /// A delay that recalculate a wait time every wait() calls and exponentially waits.
    /// The calculation is new_wait_time = max(current_wait_time * multiplier, cap).
    pub fn exponential_backoff_capped(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        Self::from(Arc::new(ExponentialBackoffWaiter::new(
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
        Arc::get_mut(&mut self.inner).unwrap().start()
    }
    fn wait(&self) -> Result<(), WaiterError> {
        self.inner.wait()
    }
    fn stop(&self) {
        self.inner.stop()
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
            Some(w) => Delay::from(Arc::new(DelayComposer::new(w, other))),
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
    fn stop(&self) {
        self.a.stop();
        self.b.stop();
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
    count: Arc<Mutex<u64>>,
}
impl CountTimeoutWaiter {
    pub fn new(max_count: u64) -> Self {
        CountTimeoutWaiter {
            max_count,
            count: Arc::new(Mutex::new(0)),
        }
    }
}
impl Waiter for CountTimeoutWaiter {
    fn start(&mut self) {
        *self.count.lock().unwrap() = 0;
    }

    fn wait(&self) -> Result<(), WaiterError> {
        let current = *self.count.lock().unwrap() + 1;
        *self.count.lock().unwrap() = current;

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
    next: Arc<Mutex<Duration>>,
    initial: Duration,
    multiplier: f32,
    cap: Duration,
}
impl ExponentialBackoffWaiter {
    pub fn new(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        ExponentialBackoffWaiter {
            next: Arc::new(Mutex::new(initial)),
            initial,
            multiplier,
            cap,
        }
    }
}
impl Waiter for ExponentialBackoffWaiter {
    fn start(&mut self) {
        self.next = Arc::new(Mutex::new(self.initial));
    }

    fn wait(&self) -> Result<(), WaiterError> {
        let current = *self.next.lock().unwrap();
        let current_nsec = current.as_nanos() as f32;

        // Find the next throttle.
        let mut next_duration = Duration::from_nanos((current_nsec * self.multiplier) as u64);
        if next_duration > self.cap {
            next_duration = self.cap;
        }

        *self.next.lock().unwrap() = next_duration;

        std::thread::sleep(current);

        Ok(())
    }
}