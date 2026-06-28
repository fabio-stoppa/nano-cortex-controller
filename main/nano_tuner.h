#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    NANO_TUNER_ROUTE_UNKNOWN = 0,
    NANO_TUNER_ROUTE_MUTE,
    NANO_TUNER_ROUTE_BYPASS,
} nano_tuner_route_t;

typedef struct {
    bool active;
    bool has_pitch;
    int8_t note; /* 0–11 (C..B), or -1 if unknown */
    int16_t cents;
    nano_tuner_route_t route;
} nano_tuner_t;

void nano_tuner_reset(void);
void nano_tuner_tick(void);

/** Parse tuner pitch / on / off from a C305 packet. Returns true if handled. */
bool nano_tuner_feed_c305(const uint8_t *data, size_t len);

const nano_tuner_t *nano_tuner_get(void);

/** Human-readable mute/bypass label matching the Nano pedal (not raw wire enum). */
const char *nano_tuner_route_label(nano_tuner_route_t route);

/** Apply tuner mode from preset state (field 45 = on, f51 = off). Route UNKNOWN keeps current route. */
void nano_tuner_sync_from_state(bool active, nano_tuner_route_t route);

/** True when a recent C305 tuner event should override conflicting preset state. */
bool nano_tuner_may_apply_state(bool state_active);

/** Enable or disable tuner (confirmed C304 packets from sniff). */
bool nano_tuner_set_active(bool active);

#ifdef __cplusplus
}
#endif
