//! Plugins: packages that add drivers and themes to an installed Dexo.

pub mod host;
pub mod proxy;
pub mod rpc;

use std::sync::{Mutex, MutexGuard, PoisonError};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
