use crate::{Delay, Waiter, WaiterError};

#[cfg(feature = "async")]
use core::{future::Future, pin::Pin};

#[cfg(all(feature = "no_std", feature = "async"))]
use alloc::boxed::Box;

#[derive(Clone)]
pub struct DelayComposer {
    a: Delay,
    b: Delay,
}
impl DelayComposer {
    pub fn new(a: Delay, b: Delay) -> Self {
        Self { a, b }
    }
}
impl Waiter for DelayComposer {
    fn restart(&mut self) -> Result<(), WaiterError> {
        self.a.restart()?;
        self.b.restart()?;
        Ok(())
    }
    fn start(&mut self) {
        self.a.start();
        self.b.start();
    }
    fn wait(&mut self) -> Result<(), WaiterError> {
        self.a.wait()?;
        self.b.wait()?;
        Ok(())
    }

    #[cfg(feature = "async")]
    fn async_wait(&mut self) -> Pin<Box<dyn Future<Output = Result<(), WaiterError>> + Send>> {
        use futures_util::TryFutureExt;
        Box::pin(
            futures_util::future::try_join(self.a.async_wait(), self.b.async_wait()).map_ok(|_| ()),
        )
    }
}
