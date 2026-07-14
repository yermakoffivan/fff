use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use fff::{SharedFilePicker, WatchId};
use pyo3::prelude::*;

/// Handle for an active watch subscription returned by `FileFinder.watch`.
///
/// Usable as a context manager: exiting the `with` block unsubscribes.
/// Subscriptions also die with the picker, so `FileFinder.close()` makes an
/// explicit `unsubscribe()` unnecessary (but still safe).
#[pyclass]
pub struct WatchSubscription {
    picker: SharedFilePicker,
    id: u64,
    /// Shared with the delivery closure: flipped before core unwatch so the
    /// user callback never runs for events racing the unsubscribe.
    active: Arc<AtomicBool>,
}

impl WatchSubscription {
    pub(crate) fn new(picker: SharedFilePicker, id: u64, active: Arc<AtomicBool>) -> Self {
        Self { picker, id, active }
    }
}

#[pymethods]
impl WatchSubscription {
    #[getter]
    fn id(&self) -> u64 {
        self.id
    }

    #[getter]
    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire) && self.picker.is_watch_active(WatchId(self.id))
    }

    /// Stop delivering events. Idempotent: returns True when the subscription
    /// was removed by this call, False if it was already inactive.
    ///
    /// Non-blocking and safe to call from inside the callback itself. The
    /// user callback is guaranteed to not be invoked for this subscription
    /// after this returns (a callback already executing may finish).
    fn unsubscribe(&self) -> bool {
        if !self.active.swap(false, Ordering::AcqRel) {
            return false;
        }
        self.picker.unwatch(WatchId(self.id))
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(&self, _exc_type: PyObject, _exc_value: PyObject, _traceback: PyObject) {
        self.unsubscribe();
    }

    fn __repr__(&self) -> String {
        let active = if self.active() { "True" } else { "False" };
        format!("WatchSubscription(id={}, active={})", self.id, active)
    }
}
