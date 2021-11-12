//! Utilities for working with retries driven by the [`backoff`](::backoff) crate.

use backoff::backoff::Backoff;
use std::{future::Future, time::Duration};

/// A wrapper for an `&mut impl `[`Backoff`].
///
/// It's useful to be able to supply a reference to a [`Backoff`] in order to maintain the same
/// retry 'context' across multiple operations. Sadly this isn't possible with the trait
/// implementations in the `backoff` crate itself, since `Backoff` is not implement for mutable
/// references.
pub struct BackoffRef<'b, B: Backoff>(&'b mut B);

impl<'b, B: Backoff> BackoffRef<'b, B> {
    /// Construct a `BackoffRef` from a mutable reference to an implementor of [`Backoff`].
    pub fn new(backoff: &'b mut B) -> Self {
        Self(backoff)
    }

    /// Retry `operation` until permanent error or exhausting `self`.
    pub async fn retry<Op, Fut, Ok, Err>(&mut self, mut operation: Op) -> Result<Ok, Err>
    where
        Op: FnMut(BackoffRef<'_, B>) -> Fut,
        Fut: Future<Output = Result<Ok, backoff::Error<Err>>>,
    {
        loop {
            // try the operation
            match operation(BackoffRef(&mut self.0)).await {
                Ok(value) => return Ok(value),
                Err(backoff::Error::Transient(error)) => {
                    // retry the transient error, if we have time remaining
                    if let Some(duration) = self.next_backoff() {
                        tokio::time::sleep(duration).await;
                        continue;
                    } else {
                        // we're out of time, bail
                        return Err(error);
                    }
                }
                Err(backoff::Error::Permanent(error)) => {
                    // we cannot retry permanent errors here
                    return Err(error);
                }
            }
        }
    }
}

impl<'b, B: Backoff> Backoff for BackoffRef<'b, B> {
    fn next_backoff(&mut self) -> Option<Duration> {
        Backoff::next_backoff(self.0)
    }
}
