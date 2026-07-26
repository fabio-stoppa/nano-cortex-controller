#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

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

/**
 * Boot BPM hunt frames — intentionally empty.
 * Trailer 0x91 writes enter tap-tempo and can rewrite BPM; do not probe via C304.
 * BPM is read from nested field 56 in FE metadata.
 */
size_t nano_bpm_probe_payload(unsigned step, uint8_t *buf, size_t cap, bool *out_expects_state_dump);

/** Port of setAmpKnob() in index.html. */
size_t nano_amp_knob_payload(nano_knob_t knob, uint8_t raw_value, uint8_t *buf, size_t cap);

/** Port of setCapture() in index.html — slot 1..25. */
size_t nano_capture_select_payload(uint8_t slot, uint8_t *buf, size_t cap);

/** Port of setGateEnabled() toggle pattern: 0A C0 08 01 18 <id> 20 <val> 1F 00 00 00 */
size_t nano_global_toggle_payload(uint8_t id, uint8_t value, uint8_t *buf, size_t cap);

/** Gate reduction knob (id 0x0B), encoded varint = pct + 108. */
size_t nano_gate_reduction_payload(uint8_t pct, uint8_t *buf, size_t cap);

/** FX block param refresh: 08 C0 08 03 18 <refreshSlot> 89 00 00 00 */
size_t nano_fx_param_refresh_payload(uint8_t refresh_slot, uint8_t *buf, size_t cap);

/** FX param write: float32 normalized 0..1 at param index. */
size_t nano_fx_param_payload(uint8_t model_slot, uint8_t param_index, float normalized,
                             uint8_t *buf, size_t cap);

/** Nano Cortex Tempo Mode limits (footswitch / UI clamp). */
#define NANO_BPM_MIN 40
#define NANO_BPM_MAX 240

/**
 * Tempo Mode tap / enter (sets BPM, enters/stays in Tempo Mode):
 *   0F C0 08 01 18 01 20 01 2D <f32 BPM LE> 91 00 00 00
 */
size_t nano_bpm_tap_payload(float bpm, uint8_t *buf, size_t cap);

/**
 * Tempo Mode exit shape (leave-mode notify; best-effort write on UI close):
 *   0D C0 08 01 20 01 2D <f32 BPM LE> 91 00 00 00
 */
size_t nano_bpm_exit_payload(float bpm, uint8_t *buf, size_t cap);

/** FX model select: C0 18 <modelSlot> 20 <id...> 88 00 00 00 */
size_t nano_fx_model_select_payload(uint8_t model_slot, const uint8_t *id, uint8_t id_len,
                                    uint8_t *buf, size_t cap);

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
