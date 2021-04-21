#![cfg(not(feature = "no_std"))]
use crate::{Waiter, WaiterError};
use std::time::Duration;

use core::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "async")]
use std::{future::Future, pin::Pin};

#[cfg(feature = "async")]
mod future {
    use crate::WaiterError;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    /// A Future that resolves when a time has passed.
    /// This is based on [https://rust-lang.github.io/async-book/02_execution/03_wakeups.html].
    pub(super) struct ThrottleTimerFuture {
        shared_state: Arc<Mutex<SharedState>>,
    }

    /// Shared state between the future and the waiting thread
    struct SharedState {
        /// Whether or not the sleep time has elapsed
        completed: bool,

        /// The waker for the task that `TimerFuture` is running on.
        /// The thread can use this after setting `completed = true` to tell
        /// `TimerFuture`'s task to wake up, see that `completed = true`, and
        /// move forward.
        waker: Option<Waker>,
    }

    impl Future for ThrottleTimerFuture {
        type Output = Result<(), WaiterError>;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            // Look at the shared state to see if the timer has already completed.
            let mut shared_state = self.shared_state.lock().unwrap();
            if shared_state.completed {
                Poll::Ready(Ok(()))
            } else {
                // Set waker so that the thread can wake up the current task
                // when the timer has completed, ensuring that the future is polled
                // again and sees that `completed = true`.
                //
                // It's tempting to do this once rather than repeatedly cloning
                // the waker each time. However, the `TimerFuture` can move between
                // tasks on the executor, which could cause a stale waker pointing
                // to the wrong task, preventing `TimerFuture` from waking up
                // correctly.
                //
                // N.B. it's possible to check for this using the `Waker::will_wake`
                // function, but we omit that here to keep things simple.
                shared_state.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }

    impl ThrottleTimerFuture {
        /// Create a new `TimerFuture` which will complete after the provided
        /// timeout.
        pub fn new(duration: Duration) -> Self {
            let shared_state = Arc::new(Mutex::new(SharedState {
                completed: false,
                waker: None,
            }));

            // Spawn the new thread
            let thread_shared_state = shared_state.clone();
            std::thread::spawn(move || {
                std::thread::sleep(duration);
                let mut shared_state = thread_shared_state.lock().unwrap();
                // Signal that the timer has completed and wake up the last
                // task on which the future was polled, if one exists.
                shared_state.completed = true;
                if let Some(waker) = shared_state.waker.take() {
                    waker.wake()
                }
            });

            ThrottleTimerFuture { shared_state }
        }
    }
}

#[derive(Clone)]
pub struct ThrottleWaiter {
    throttle: Duration,
}
impl ThrottleWaiter {
    pub fn new(throttle: Duration) -> Self {
        Self { throttle }
    }
}
impl Waiter for ThrottleWaiter {
    fn wait(&mut self) -> Result<(), WaiterError> {
        std::thread::sleep(self.throttle);

        Ok(())
    }

    #[cfg(feature = "async")]
    fn async_wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), WaiterError>> + Send>> {
        Box::pin(future::ThrottleTimerFuture::new(self.throttle))
    }
}

pub struct ExponentialBackoffWaiter {
    next_as_micros: Option<AtomicU64>,
    initial_as_micros: u64,
    multiplier: f64,
    cap_as_micros: u64,
}
impl ExponentialBackoffWaiter {
    pub fn new(initial: Duration, multiplier: f32, cap: Duration) -> Self {
        ExponentialBackoffWaiter {
            next_as_micros: None,
            initial_as_micros: initial.as_micros() as u64,
            multiplier: multiplier as f64,
            cap_as_micros: cap.as_micros() as u64,
        }
    }

    fn increment(&mut self) -> Result<Duration, WaiterError> {
        let next = self
            .next_as_micros
            .as_ref()
            .ok_or(WaiterError::NotStarted)?;
        let current = next.load(Ordering::Relaxed);

        // Find the next throttle.
        let next = u64::max(
            (current as f64 * self.multiplier) as u64,
            self.cap_as_micros,
        );
        self.next_as_micros
            .as_mut()
            .unwrap()
            .store(next, Ordering::Relaxed);
        Ok(Duration::from_micros(current))
    }
}
impl Clone for ExponentialBackoffWaiter {
    fn clone(&self) -> Self {
        Self {
            next_as_micros: self
                .next_as_micros
                .as_ref()
                .map(|a| AtomicU64::new(a.load(Ordering::Relaxed))),
            ..*self
        }
    }
}
impl Waiter for ExponentialBackoffWaiter {
    fn restart(&mut self) -> Result<(), WaiterError> {
        if self.next_as_micros.is_none() {
            Err(WaiterError::NotStarted)
        } else {
            self.next_as_micros = Some(AtomicU64::new(self.initial_as_micros));
            Ok(())
        }
    }

    fn start(&mut self) {
        self.next_as_micros = Some(AtomicU64::new(self.initial_as_micros));
    }

    fn wait(&mut self) -> Result<(), WaiterError> {
        let current = self.increment()?;
        std::thread::sleep(current);
        Ok(())
    }

    #[cfg(feature = "async")]
    fn async_wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), WaiterError>> + Send>> {
        match self.increment() {
            Ok(current) => Box::pin(future::ThrottleTimerFuture::new(current)),
            Err(e) => Box::pin(futures_util::future::err(e)),
        }
    }
}
