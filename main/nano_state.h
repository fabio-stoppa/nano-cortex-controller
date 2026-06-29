#pragma once

#include <stddef.h>
#include <stdint.h>

#include "nano_proto.h"
#include "nano_protocol.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    NANO_RX_IDLE = 0,
    NANO_RX_METADATA,
    NANO_RX_CURRENT,
} nano_rx_mode_t;

void nano_state_reset(void);
void nano_rx_mode_set(nano_rx_mode_t mode);
nano_rx_mode_t nano_rx_mode_get(void);

/** Feed one C305 notification (handles FE stream reassembly + amp parse). */
void nano_state_feed_c305(const uint8_t *data, size_t len);

const nano_amp_t *nano_state_get_amp(void);

/** Optimistically update amp state and send C304 knob write (touch UI). */
bool nano_state_write_amp_knob(nano_knob_t knob, uint8_t raw);

/** Call after C305 notify enabled: sends metadata, then state request ~2.2s later. */
void nano_state_begin_sync(void);

/** Faster preset polling while tuner appears off (catches footswitch without C305). */
void nano_state_request_tuner_sync(void);

/** Faster preset polling while tuner appears off (catches footswitch without C305). */
void nano_state_tuner_watch_update(bool tuner_active);

/** Re-read preset state (e.g. after FX block toggle). */
void nano_state_request_preset_refresh(const char *label);

#ifdef __cplusplus
}
#endif
