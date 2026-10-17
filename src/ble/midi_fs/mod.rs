//! BLE-MIDI footswitch client (M-Vave Chocolate / Chocolate Plus).
//!
//! Second GATTC profile alongside the Nano Cortex link. Scans for `FootCtrl` /
//! `FootCtrlPlus` (case-insensitive; Builty / stock M-Vave names), can keep both
//! peers connected at once, subscribes to BLE-MIDI, and maps Control Change to
//! FX on/off + focus (Plus bank 0 → Post3 / gate / tap / solo boost).

mod api;
mod ctx;
mod gatt;
mod map;
mod midi;
mod worker;

pub use api::{
    gattc_if, is_connected, is_footctrl_name, need_peer, on_adv_match, profile_app_id,
    register_app,
};
pub(crate) use gatt::gattc_handler;
