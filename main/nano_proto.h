#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#include "nano_tuner.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint8_t gain;
    uint8_t level;
    uint8_t bass;
    uint8_t mid;
    uint8_t treble;
    bool valid;
} nano_amp_t;

bool nano_read_varint(const uint8_t *data, size_t len, size_t offset, uint32_t *value, size_t *next);

/** Parse protobuf fields 3–7 (amp knobs) from a state blob. */
bool nano_parse_amp(const uint8_t *data, size_t len, nano_amp_t *out);

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
