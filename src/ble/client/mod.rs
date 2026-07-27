//! BLE GATT client for the Neural DSP Nano Cortex (port of `components/nano/nano_ble.c`).
//!
//! A002/A003: C304 write, C305 notify, C306 indicate (mirrors C305 and is deliberately
//! left unsubscribed). Other NOTIFY/INDICATE chars (incl. BLE-MIDI) are subscribed too.

use core::ffi::{c_void, CStr};
use core::mem::{size_of, MaybeUninit};
use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, Ordering};
use std::ffi::CString;

use esp_idf_svc::sys::*;
use log::{debug, error, info, warn};

mod api;
mod ctx;
mod gap;
mod gatt;
mod worker;

pub use api::*;
