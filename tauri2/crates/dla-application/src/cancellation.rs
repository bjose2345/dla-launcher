use std::{
    fmt,
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::scanner::ScanCancellation;

/// Cooperative cancellation between the side that starts a long operation and
/// the code doing it, which hold clones of the same flag. `E` is the error the
/// work reports when it finds the flag set; a plain flag leaves it `()`.
pub struct CancellationToken<E = ()> {
    cancelled: Arc<AtomicBool>,
    error: PhantomData<fn() -> E>,
}

/// An error with a value for cancelled work.
pub trait Cancelled {
    fn cancelled() -> Self;
}

impl<E> CancellationToken<E> {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl<E: Cancelled> CancellationToken<E> {
    pub fn check(&self) -> Result<(), E> {
        if self.is_cancelled() {
            Err(E::cancelled())
        } else {
            Ok(())
        }
    }
}

impl<E> Clone for CancellationToken<E> {
    fn clone(&self) -> Self {
        Self {
            cancelled: Arc::clone(&self.cancelled),
            error: PhantomData,
        }
    }
}

impl<E> Default for CancellationToken<E> {
    fn default() -> Self {
        Self {
            cancelled: Arc::default(),
            error: PhantomData,
        }
    }
}

impl<E> fmt::Debug for CancellationToken<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

impl<E> ScanCancellation for CancellationToken<E> {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}
