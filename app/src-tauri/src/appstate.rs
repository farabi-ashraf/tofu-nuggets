//! Cross-thread app flags shared by the hover engine, badge layer, and tray.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Whether hover panels + badges are paused (toggled from the tray).
#[derive(Clone, Default)]
pub struct Paused(Arc<AtomicBool>);

impl Paused {
    pub fn is_paused(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, v: bool) {
        self.0.store(v, Ordering::Relaxed);
    }

    pub fn toggle(&self) -> bool {
        let now = !self.is_paused();
        self.set(now);
        now
    }
}
