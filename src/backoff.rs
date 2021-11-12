//! Utilities for working with retries driven by the [`backoff`](::backoff) crate.

use backoff::backoff::Backoff;
use std::time::Duration;

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
}

impl<'b, B: Backoff> Backoff for BackoffRef<'b, B> {
    fn next_backoff(&mut self) -> Option<Duration> {
        Backoff::next_backoff(self.0)
    }
}
