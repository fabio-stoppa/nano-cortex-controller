//! FX blocks + noise gate.

pub mod catalog;
pub mod engine;
pub mod models;
pub mod params;
pub mod types;
pub mod util;

pub use catalog::{nano_fx_catalog, nano_fx_catalog_count};
pub use engine::{
    nano_fx_begin_param_refresh, nano_fx_clear_model_trust, nano_fx_dump_has_gate_fields,
    nano_fx_feed_c305, nano_fx_get, nano_fx_parse_preset, nano_fx_reset, nano_fx_snapshot,
    nano_fx_sync_from_preset, nano_fx_try_apply_gate_bytes, nano_fx_write_block_enabled,
    nano_fx_write_block_model, nano_fx_write_gate_enabled, nano_fx_write_gate_reduction,
    nano_fx_write_param_display, nano_fx_write_param_normalized,
};
pub use models::{
    nano_fx_allowed_model_count, nano_fx_block_label, nano_fx_block_model_text,
    nano_fx_category_label, nano_fx_find_model, nano_fx_get_allowed_model, nano_fx_is_model_allowed,
    nano_fx_model_category, nano_fx_model_chain_acronym,
};
pub use params::{
    nano_fx_display_to_normalized, nano_fx_display_to_slider, nano_fx_format_param,
    nano_fx_normalized_to_display, nano_fx_param_enum_count, nano_fx_param_enum_label,
    nano_fx_param_is_enum, nano_fx_param_range, nano_fx_param_scale, nano_fx_slider_max,
    nano_fx_slider_min, nano_fx_slider_to_display,
};

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_fx_reset as *const (),
        nano_fx_get as *const (),
        nano_fx_snapshot as *const (),
        nano_fx_parse_preset as *const (),
        nano_fx_sync_from_preset as *const (),
        nano_fx_clear_model_trust as *const (),
        nano_fx_begin_param_refresh as *const (),
        nano_fx_dump_has_gate_fields as *const (),
        nano_fx_try_apply_gate_bytes as *const (),
        nano_fx_feed_c305 as *const (),
        nano_fx_block_label as *const (),
        nano_fx_block_model_text as *const (),
        nano_fx_model_chain_acronym as *const (),
        nano_fx_model_category as *const (),
        nano_fx_category_label as *const (),
        nano_fx_find_model as *const (),
        nano_fx_is_model_allowed as *const (),
        nano_fx_allowed_model_count as *const (),
        nano_fx_get_allowed_model as *const (),
        nano_fx_write_gate_enabled as *const (),
        nano_fx_write_gate_reduction as *const (),
        nano_fx_write_block_enabled as *const (),
        nano_fx_write_block_model as *const (),
        nano_fx_write_param_normalized as *const (),
        nano_fx_write_param_display as *const (),
        nano_fx_format_param as *const (),
        nano_fx_param_is_enum as *const (),
        nano_fx_param_enum_count as *const (),
        nano_fx_param_enum_label as *const (),
        nano_fx_param_range as *const (),
        nano_fx_param_scale as *const (),
        nano_fx_display_to_slider as *const (),
        nano_fx_slider_to_display as *const (),
        nano_fx_slider_min as *const (),
        nano_fx_slider_max as *const (),
        nano_fx_display_to_normalized as *const (),
        nano_fx_normalized_to_display as *const (),
    ];
    core::hint::black_box(fns);

    // The C UI indexes the catalog directly, so keep both statics too.
    let data: &[*const ()] = &[
        nano_fx_catalog.as_ptr() as *const (),
        &nano_fx_catalog_count as *const usize as *const (),
    ];
    core::hint::black_box(data);
}
