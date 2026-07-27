//! Interior-mutability wrapper for the module-level FX state.

use core::cell::UnsafeCell;

pub struct Shared<T>(UnsafeCell<T>);

// Same contract as the C port: FX state is touched from the C305 worker, the
// esp_timer task and the UI task, ordered only by the BLE event flow.
unsafe impl<T> Sync for Shared<T> {}

impl<T> Shared<T> {
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get(&self) -> &'static mut T {
        unsafe { &mut *self.0.get() }
    }
}
