#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#include "nano_tuner.h"

#ifdef __cplusplus
extern "C" {
#endif

#define NANO_AMP_CAPTURE_NAME_MAX 40

typedef struct {
    uint8_t gain;
    uint8_t level;
    uint8_t bass;
    uint8_t mid;
    uint8_t treble;
    uint8_t capture_slot;
    char capture_name[NANO_AMP_CAPTURE_NAME_MAX];
    bool have_capture_slot;
    bool have_capture_name;
    bool valid;
} nano_amp_t;

#define NANO_PRESET_COUNT    64
#define NANO_PRESET_NAME_MAX 48

typedef struct {
    char names[NANO_PRESET_COUNT][NANO_PRESET_NAME_MAX];
    uint8_t count;
    bool valid;
} nano_preset_catalog_t;

bool nano_read_varint(const uint8_t *data, size_t len, size_t offset, uint32_t *value, size_t *next);

/** First printable wire-2 string for a protobuf field inside a blob. */
bool nano_proto_first_string(const uint8_t *data, size_t len, uint32_t field_num, char *out, size_t cap);

/** First varint for a protobuf field inside a blob. */
bool nano_proto_field_varint(const uint8_t *data, size_t len, uint32_t field_num, uint32_t *out_val);

/** Parse preset names from FE metadata (top-level field 18 records). */
bool nano_parse_fe_preset_catalog(const uint8_t *data, size_t len, nano_preset_catalog_t *out);

/** Parse protobuf fields 3–7 (amp knobs) and capture slot/name from a state blob. */
bool nano_parse_amp(const uint8_t *data, size_t len, nano_amp_t *out);

/** Parse preset tempo (BPM) from likely global varint fields. */
bool nano_parse_bpm(const uint8_t *data, size_t len, uint16_t *out_bpm);

/**
 * Tuner fields in preset state: f45 present = on, f51 present (no f45) = off.
 * Route (mute/bypass) is not in preset state — learn it from C305 2D packets.
 * Returns false when neither signature is present (do not sync active/off).
 */
bool nano_parse_tuner_state(const uint8_t *data, size_t len, bool *out_active,
                            nano_tuner_route_t *out_route);

/** @deprecated use nano_parse_tuner_state */
bool nano_parse_tuner_active(const uint8_t *data, size_t len, bool *out_active);

/**
 * Try to read a single amp knob from a short C305 notify (write echo / live update).
 * Matches TX pattern: C0 18 <id 0–4> 20 <varint value>.
 */
bool nano_try_parse_amp_knob_notify(const uint8_t *data, size_t len, uint8_t *knob_id, uint8_t *raw);

/**
 * Hardware knob turn on the Nano (observed C305 captures):
 *   Gain float: C0 08 01 20 01 2D <f32> 63
 *   Gain varint: C0 08 01 20 <varint>
 *   Other: C0 08 01 18 <id 0–4 or 0x05–0x09> 20 <varint>
 */
bool nano_try_parse_amp_hw_notify(const uint8_t *data, size_t len, uint8_t *knob_id, uint8_t *raw);

/** Log amp as raw + percent (0–255 → 0–100%). */
void nano_log_amp(const char *tag, const nano_amp_t *amp);

/** Dump top-level protobuf fields (for protocol hunting). */
void nano_log_proto_fields(const char *tag, const char *label, const uint8_t *data, size_t len);

/** One-line dump of tuner-related preset fields (f45/f48–f51). */
void nano_log_tuner_state_fields(const char *tag, const uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif
