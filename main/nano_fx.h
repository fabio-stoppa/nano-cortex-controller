#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NANO_FX_BLOCK_COUNT 5
#define NANO_FX_MAX_PARAMS  24

/** @deprecated use NANO_FX_MAX_PARAMS */
#define NANO_FX_UI_PARAMS   NANO_FX_MAX_PARAMS

typedef enum {
    NANO_FX_PRE1 = 0,
    NANO_FX_PRE2,
    NANO_FX_POST1,
    NANO_FX_POST2,
    NANO_FX_POST3,
} nano_fx_block_id_t;

typedef enum {
    NANO_FX_CAT_NONE = 0,
    NANO_FX_CAT_GUITAR_OD,
    NANO_FX_CAT_BASS_OD,
    NANO_FX_CAT_COMPRESSOR,
    NANO_FX_CAT_PITCH,
    NANO_FX_CAT_FILTER,
    NANO_FX_CAT_WAH,
    NANO_FX_CAT_UTILITY,
    NANO_FX_CAT_MODULATION,
    NANO_FX_CAT_DELAY,
    NANO_FX_CAT_REVERB,
} nano_fx_category_t;

typedef struct {
    const char *name;
    uint8_t id_len;
    uint8_t id[3];
    uint8_t param_count;
    const char *const *param_names;
} nano_fx_model_def_t;

typedef struct {
    float normalized;
    float display;
    bool valid;
} nano_fx_param_t;

typedef struct {
    bool on;
    bool valid;
    /** True when ON/OFF came from field 54 or a full dump (safe to apply). */
    bool on_known;
    /** True when reduction_pct came from preset field 53 or a local write. */
    bool reduction_valid;
    uint8_t reduction_pct;
} nano_gate_t;

typedef struct {
    nano_fx_block_id_t id;
    bool on;
    bool valid;
    const nano_fx_model_def_t *model;
    uint8_t model_id_raw[8];
    uint8_t model_id_len;
    nano_fx_param_t params[NANO_FX_MAX_PARAMS];
    uint8_t param_count;
    bool params_loaded;
    bool has_model_id;
} nano_fx_block_t;

typedef struct {
    nano_gate_t gate;
    nano_fx_block_t blocks[NANO_FX_BLOCK_COUNT];
    bool valid;
} nano_fx_state_t;

extern const nano_fx_model_def_t nano_fx_catalog[];
extern const size_t nano_fx_catalog_count;

void nano_fx_reset(void);

/** Parse gate + FX block models/bypass from preset state blob. */
bool nano_fx_parse_preset(const uint8_t *data, size_t len, nano_fx_state_t *out);

/** After preset parse, queue param refresh for active blocks. */
void nano_fx_begin_param_refresh(void);

/** Parse preset blob into internal FX state and start param refresh. */
void nano_fx_sync_from_preset(const uint8_t *data, size_t len);

/** True when raw dump bytes contain gate amount (f53) and/or bypass (f54) tags. */
bool nano_fx_dump_has_gate_fields(const uint8_t *data, size_t len);

/**
 * Salvage gate ON/OFF + amount from any blob that contains f53/f54 tags
 * (full preset dump or a late high-bit C305 tail after a mid-dump gap).
 */
bool nano_fx_try_apply_gate_bytes(const uint8_t *data, size_t len);

/** Feed C305; returns true if consumed (FX param refresh response). */
bool nano_fx_feed_c305(const uint8_t *data, size_t len);

const nano_fx_state_t *nano_fx_get(void);

const char *nano_fx_block_label(nano_fx_block_id_t block);

/** Effect family for signal-chain icons (PRE/PFX slots). */
nano_fx_category_t nano_fx_model_category(const nano_fx_model_def_t *model);

const char *nano_fx_category_label(nano_fx_category_t cat);

/** Model name, or "ID xx yy" / "Empty" when unknown. */
void nano_fx_block_model_text(const nano_fx_block_t *block, char *buf, size_t cap);

/** Short label for signal-chain icon (e.g. "P95", "MHL"). */
void nano_fx_model_chain_acronym(const nano_fx_block_t *block, char *buf, size_t cap);

bool nano_fx_write_gate_enabled(bool enabled);
bool nano_fx_write_gate_reduction(uint8_t pct);
bool nano_fx_write_block_enabled(nano_fx_block_id_t block, bool enabled);

/** Whether catalog entry may be loaded in this FX slot (NanOS 2.0 rules). */
bool nano_fx_is_model_allowed(nano_fx_block_id_t block, const nano_fx_model_def_t *model);

/** Nth allowed catalog model for block (0 .. count-1). */
const nano_fx_model_def_t *nano_fx_get_allowed_model(nano_fx_block_id_t block, size_t index);

size_t nano_fx_allowed_model_count(nano_fx_block_id_t block);

/** Load model on device, enable block, queue param refresh. */
bool nano_fx_write_block_model(nano_fx_block_id_t block, const nano_fx_model_def_t *model);

bool nano_fx_write_param_normalized(nano_fx_block_id_t block, uint8_t param_index, float normalized);
bool nano_fx_write_param_display(nano_fx_block_id_t block, uint8_t param_index, float display_value);

/** Format param for UI (e.g. "35.0 %"). */
void nano_fx_format_param(const nano_fx_block_t *block, uint8_t param_index, char *buf, size_t cap);

/** Map display value ↔ normalized using param name heuristics. */
float nano_fx_display_to_normalized(const char *param_name, float display);
float nano_fx_normalized_to_display(const char *param_name, float normalized);

bool nano_fx_param_is_enum(const char *param_name);
int nano_fx_param_enum_count(const char *param_name);

/** Human-readable label for enum params (e.g. "Off", "On"). */
const char *nano_fx_param_enum_label(const char *param_name, int index);

/** Param display range for UI sliders. */
void nano_fx_param_range(const char *param_name, float *min_out, float *max_out, float *step_out);

/** Integer scale for lv_slider (1, 10, or 100 for fractional steps). */
int nano_fx_param_scale(const char *param_name);

int nano_fx_display_to_slider(const char *param_name, float display);
float nano_fx_slider_to_display(const char *param_name, int slider_value);
int nano_fx_slider_min(const char *param_name);
int nano_fx_slider_max(const char *param_name);

#ifdef __cplusplus
}
#endif
