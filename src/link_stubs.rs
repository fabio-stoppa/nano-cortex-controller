//! Keep the legacy I2C driver out of the image.
//!
//! The display board uses the new `i2c_master` driver. `esp-idf-hal` still
//! emits drop glue for its unused legacy `I2cDriver`, and that reference pulls
//! in `driver/i2c.c`. That file's startup check aborts when both drivers are
//! linked. These symbols satisfy the linker so the legacy object is never
//! pulled. Nothing in this firmware calls them.

use core::ffi::c_void;

#[no_mangle]
pub extern "C" fn i2c_driver_delete(_: i32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_cmd_link_create() -> *mut c_void {
    core::ptr::null_mut()
}

#[no_mangle]
pub extern "C" fn i2c_cmd_link_delete(_: *mut c_void) {}

#[no_mangle]
pub extern "C" fn i2c_master_cmd_begin(_: i32, _: *mut c_void, _: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_master_start(_: *mut c_void) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_master_stop(_: *mut c_void) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_master_write(_: *mut c_void, _: *const u8, _: usize, _: bool) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_master_write_byte(_: *mut c_void, _: u8, _: bool) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_master_read(_: *mut c_void, _: *mut u8, _: usize, _: i32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_slave_read_buffer(_: i32, _: *mut u8, _: usize, _: u32) -> i32 {
    0
}

#[no_mangle]
pub extern "C" fn i2c_slave_write_buffer(_: i32, _: *const u8, _: usize, _: u32) -> i32 {
    0
}
