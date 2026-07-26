#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

#define NANO_BLE_NAME_PREFIX "Neural DSP Nano Cortex"

/** Init NVS + BT controller, register GATT client, start scanning. */
esp_err_t nano_ble_start(void);

bool nano_ble_connected(void);

bool nano_ble_scanning(void);

/** Write payload to C304 (no-op if not connected). */
esp_err_t nano_ble_write_c304(const char *label, const uint8_t *data, size_t len);

/** Drop pending C305 packets (call on disconnect). */
void nano_ble_drain_rx_queue(void);

/** Flush stale C305 FIFO when tuner goes live (keeps live pitch mailbox). */
void nano_ble_flush_c305_backlog(void);

#ifdef __cplusplus
}
#endif
