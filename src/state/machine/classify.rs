//! C305 payload classification helpers.

use log::debug;

use super::ctx::{now_us, st, NANO_RX_CURRENT, NANO_RX_METADATA, TAG};

const SNIFF_MIN_INTERVAL_US: i64 = 500 * 1000;

pub(super) fn is_fe_stream(data: &[u8], mode: u32) -> bool {
    if data.len() < 3 {
        return false;
    }
    if data[0] == 0xFE || data[0] == 0xFD || data[0] == 0xCE || data[0] == 0xD0 {
        return true;
    }
    /*
     * High-bit byte1 marks segmented FE/state continuations, but live notifies
     * use lead C0 (also high bit). Classifying C0 as FE stream drops gain/EQ/
     * master updates for the whole dump window — UI freezes until parse.
     */
    if (mode == NANO_RX_METADATA || mode == NANO_RX_CURRENT)
        && (data[1] & 0x80) != 0
        && data[1] != 0xC0
    {
        return true;
    }
    false
}

/// Preset state: FE 41 stream chunks, or single-packet dump with byte1 == 0xC1.
pub(super) fn is_state_stream_chunk(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }
    if data[0] == 0xFE && data[1] == 0x41 {
        return true;
    }
    // Desktop decoder: full current-state dump notifications use second byte 0xC1.
    if data[1] == 0xC1 {
        return true;
    }
    false
}

pub(super) fn sniff_unknown_c305(data: &[u8]) {
    let s = st();
    let now = now_us();
    if now - s.last_sniff_us < SNIFF_MIN_INTERVAL_US {
        return;
    }
    s.last_sniff_us = now;
    // Keep this at debug — live Amount/FX traffic used to flood INFO and stall BLE.
    debug!(
        target: TAG,
        "unknown C305 len={} lead={:02X} {:02X}",
        data.len(),
        data.first().copied().unwrap_or(0),
        data.get(1).copied().unwrap_or(0)
    );
}
