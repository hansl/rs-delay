use crate::{Delay, Waiter, WaiterError};
use std::pin::Pin;

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
    fn wait(&self) -> Result<(), WaiterError> {
        self.a.wait()?;
        self.b.wait()?;
        Ok(())
    }

    #[cfg(feature = "async")]
    fn async_wait(&self) -> Pin<Box<dyn std::future::Future<Output = Result<(), WaiterError>>>> {
        use futures_util::TryFutureExt;
        Box::pin(
            futures_util::future::try_join(self.a.async_wait(), self.b.async_wait()).map_ok(|_| ()),
        )
    }
}
