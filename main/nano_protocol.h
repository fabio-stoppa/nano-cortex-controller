#pragma once

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    NANO_KNOB_GAIN = 0,
    NANO_KNOB_LEVEL = 1,
    NANO_KNOB_BASS = 2,
    NANO_KNOB_MID = 3,
    NANO_KNOB_TREBLE = 4,
} nano_knob_t;

/** Fixed packets from index.html (bytesFromHex). Return length written, or 0 if buf too small. */
size_t nano_fe_metadata_request(uint8_t *buf, size_t cap);
size_t nano_request_current_state(uint8_t *buf, size_t cap);
size_t nano_preset_change_ack(uint8_t *buf, size_t cap);
size_t nano_post_save_refresh(uint8_t *buf, size_t cap);

/** Port of setAmpKnob() in index.html. */
size_t nano_amp_knob_payload(nano_knob_t knob, uint8_t raw_value, uint8_t *buf, size_t cap);

/** Port of setGateEnabled() toggle pattern: 0A C0 08 01 18 <id> 20 <val> 1F 00 00 00 */
size_t nano_global_toggle_payload(uint8_t id, uint8_t value, uint8_t *buf, size_t cap);

/** Tuner on/off (sniffed): 2D + A440 float + trailer. */
size_t nano_tuner_enable_payload(uint8_t *buf, size_t cap);
size_t nano_tuner_enable_footswitch_payload(uint8_t *buf, size_t cap);
size_t nano_tuner_disable_payload(uint8_t *buf, size_t cap);

/** Log-friendly hex dump helper (uses ESP_LOGI). */
void nano_log_bytes(const char *tag, const char *label, const uint8_t *data, size_t len);

/** Log full packet across multiple lines if needed (for protocol sniffing). */
void nano_log_bytes_full(const char *tag, const char *label, const uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif
