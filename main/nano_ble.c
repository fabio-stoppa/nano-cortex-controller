/*
 * BLE GATT client for Neural DSP Nano Cortex.
 * A002/A003: C304 write, C305 notify, C306 indicate (often mirrors C305).
 * Also subscribe to any other NOTIFY/INDICATE chars (incl. BLE-MIDI).
 */

#include "nano_ble.h"
#include "nano_state.h"
#include "nano_tuner.h"
#include "nano_ui.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "esp_bt.h"
#include "esp_bt_main.h"
#include "esp_gap_ble_api.h"
#include "esp_gatt_common_api.h"
#include "esp_gattc_api.h"
#include "esp_gatt_defs.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "freertos/queue.h"
#include "nvs_flash.h"

#include "nano_protocol.h"
#include "nano_proto.h"

static const char *TAG = "nano_ble";

#define C305_QUEUE_LEN 48
#define C305_LIVE_QUEUE_LEN 1
#define C305_PKT_MAX 256
#define C305_TASK_STACK 4096
/* Keep below LVGL (prio 6) so continuous pitch cannot starve the display. */
#define C305_TASK_PRIO 1
#define GATT_NOTIFY_MAX 12
#define GATT_EXTRA_SVC_MAX 6
#define BPM_READ_HANDLE_MAX 4

typedef struct {
    uint16_t handle;
    uint16_t len;
    uint8_t data[C305_PKT_MAX];
} c305_queue_item_t;

typedef struct {
    uint16_t handle;
    uint16_t cccd_value; /* bit0=notify, bit1=indicate */
} gatt_sub_t;

static QueueHandle_t s_c305_queue;
static QueueHandle_t s_c305_live_queue; /* length-1 overwrite mailbox while tuner is live */
static TaskHandle_t s_c305_task;
static uint32_t s_c305_drop_count;

static gatt_sub_t s_subs[GATT_NOTIFY_MAX];
static size_t s_sub_count;

static uint16_t s_bpm_read_handles[BPM_READ_HANDLE_MAX];
static size_t s_bpm_read_count;
static size_t s_bpm_read_next;

static struct {
    uint16_t start;
    uint16_t end;
    char name[24];
} s_extra_svcs[GATT_EXTRA_SVC_MAX];
static size_t s_extra_svc_count;

#define PROFILE_APP_ID 0
#define INVALID_HANDLE 0

#define UUID_SVC_A002 0xA002
#define UUID_SVC_A003 0xA003
#define UUID_CHAR_C304  0xC304
#define UUID_CHAR_C305  0xC305
#define UUID_CHAR_C306  0xC306
#define UUID_CHAR_C300  0xC300
#define UUID_CHAR_C301  0xC301
#define UUID_CHAR_C307  0xC307

/* BLE-MIDI service/char — Bluedroid little-endian 128-bit form. */
static const uint8_t UUID128_BLE_MIDI_SVC[16] = {
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
};
static const uint8_t UUID128_BLE_MIDI_IO[16] = {
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
};

static struct {
    esp_gatt_if_t gattc_if;
    uint16_t conn_id;
    esp_bd_addr_t remote_bda;
    bool connected;
    bool scanning;
    uint16_t a002_start;
    uint16_t a002_end;
    uint16_t a003_start;
    uint16_t a003_end;
    uint16_t c304_handle;
    uint16_t c305_handle;
    uint16_t c306_handle;
    uint16_t midi_handle;
    bool notify_enabled;
} s_ble;

static esp_bt_uuid_t uuid_cccd = {
    .len = ESP_UUID_LEN_16,
    .uuid = {.uuid16 = ESP_GATT_UUID_CHAR_CLIENT_CONFIG},
};

static esp_ble_scan_params_t scan_params = {
    .scan_type = BLE_SCAN_TYPE_ACTIVE,
    .own_addr_type = BLE_ADDR_TYPE_PUBLIC,
    .scan_filter_policy = BLE_SCAN_FILTER_ALLOW_ALL,
    .scan_interval = 0x50,
    .scan_window = 0x30,
    .scan_duplicate = BLE_SCAN_DUPLICATE_DISABLE,
};

static esp_bt_uuid_t uuid16(uint16_t id)
{
    esp_bt_uuid_t u = {
        .len = ESP_UUID_LEN_16,
        .uuid = {.uuid16 = id},
    };
    return u;
}

static void format_uuid(const esp_bt_uuid_t *u, char *buf, size_t cap)
{
    if (!u || !buf || cap == 0) {
        return;
    }
    if (u->len == ESP_UUID_LEN_16) {
        snprintf(buf, cap, "0x%04X", u->uuid.uuid16);
    } else if (u->len == ESP_UUID_LEN_32) {
        snprintf(buf, cap, "0x%08lX", (unsigned long)u->uuid.uuid32);
    } else if (u->len == ESP_UUID_LEN_128) {
        const uint8_t *b = u->uuid.uuid128;
        snprintf(buf, cap,
                 "%02X%02X%02X%02X-%02X%02X-%02X%02X-%02X%02X-%02X%02X%02X%02X%02X%02X",
                 b[15], b[14], b[13], b[12], b[11], b[10], b[9], b[8], b[7], b[6], b[5], b[4],
                 b[3], b[2], b[1], b[0]);
    } else {
        snprintf(buf, cap, "?");
    }
}

static bool uuid128_eq(const esp_bt_uuid_t *u, const uint8_t expect[16])
{
    return u && u->len == ESP_UUID_LEN_128 && memcmp(u->uuid.uuid128, expect, 16) == 0;
}

static void reset_handles(void)
{
    s_ble.a002_start = INVALID_HANDLE;
    s_ble.a002_end = INVALID_HANDLE;
    s_ble.a003_start = INVALID_HANDLE;
    s_ble.a003_end = INVALID_HANDLE;
    s_ble.c304_handle = INVALID_HANDLE;
    s_ble.c305_handle = INVALID_HANDLE;
    s_ble.c306_handle = INVALID_HANDLE;
    s_ble.midi_handle = INVALID_HANDLE;
    s_ble.notify_enabled = false;
    s_sub_count = 0;
    s_extra_svc_count = 0;
    s_bpm_read_count = 0;
    s_bpm_read_next = 0;
}

static void start_scan(void)
{
    if (s_ble.scanning || s_ble.connected) {
        return;
    }
    esp_err_t err = esp_ble_gap_start_scanning(0);
    if (err == ESP_OK) {
        s_ble.scanning = true;
        ESP_LOGI(TAG, "Scanning for \"%s\"...", NANO_BLE_NAME_PREFIX);
    } else {
        ESP_LOGE(TAG, "Scan start failed: %s", esp_err_to_name(err));
    }
}

static bool find_char_in_range(esp_gatt_if_t gattc_if, uint16_t conn_id,
                               uint16_t start, uint16_t end, uint16_t char_uuid,
                               uint16_t *out_handle)
{
    if (start == INVALID_HANDLE || end == INVALID_HANDLE) {
        return false;
    }

    uint16_t count = 0;
    if (esp_ble_gattc_get_attr_count(gattc_if, conn_id, ESP_GATT_DB_CHARACTERISTIC,
                                     start, end, INVALID_HANDLE, &count) != ESP_GATT_OK ||
        count == 0) {
        return false;
    }

    esp_gattc_char_elem_t *elems = calloc(count, sizeof(esp_gattc_char_elem_t));
    if (!elems) {
        return false;
    }

    esp_bt_uuid_t filter = uuid16(char_uuid);
    bool found = false;
    if (esp_ble_gattc_get_char_by_uuid(gattc_if, conn_id, start, end, filter, elems, &count) ==
            ESP_GATT_OK &&
        count > 0) {
        *out_handle = elems[0].char_handle;
        found = true;
    }
    free(elems);
    return found;
}

static void add_sub(uint16_t handle, uint16_t cccd_value)
{
    if (handle == INVALID_HANDLE || cccd_value == 0 || s_sub_count >= GATT_NOTIFY_MAX) {
        return;
    }
    for (size_t i = 0; i < s_sub_count; i++) {
        if (s_subs[i].handle == handle) {
            s_subs[i].cccd_value |= cccd_value;
            return;
        }
    }
    s_subs[s_sub_count].handle = handle;
    s_subs[s_sub_count].cccd_value = cccd_value;
    s_sub_count++;
}

static uint16_t cccd_for_handle(uint16_t handle)
{
    for (size_t i = 0; i < s_sub_count; i++) {
        if (s_subs[i].handle == handle) {
            return s_subs[i].cccd_value;
        }
    }
    return 0x0001; /* notify */
}

static const char *handle_label(uint16_t handle)
{
    if (s_ble.c305_handle != INVALID_HANDLE && handle == s_ble.c305_handle) {
        return "C305";
    }
    if (s_ble.c306_handle != INVALID_HANDLE && handle == s_ble.c306_handle) {
        return "C306";
    }
    if (s_ble.midi_handle != INVALID_HANDLE && handle == s_ble.midi_handle) {
        return "BLE-MIDI";
    }
    return NULL;
}

static void log_and_collect_notify_chars(esp_gatt_if_t gattc_if, uint16_t conn_id, uint16_t start,
                                         uint16_t end, const char *svc_name)
{
    if (start == INVALID_HANDLE || end == INVALID_HANDLE) {
        return;
    }

    uint16_t count = 0;
    if (esp_ble_gattc_get_attr_count(gattc_if, conn_id, ESP_GATT_DB_CHARACTERISTIC, start, end,
                                     INVALID_HANDLE, &count) != ESP_GATT_OK ||
        count == 0) {
        return;
    }

    esp_gattc_char_elem_t *elems = calloc(count, sizeof(esp_gattc_char_elem_t));
    if (!elems) {
        return;
    }

    if (esp_ble_gattc_get_all_char(gattc_if, conn_id, start, end, elems, &count, 0) != ESP_GATT_OK) {
        free(elems);
        return;
    }

    for (uint16_t i = 0; i < count; i++) {
        char uuid_s[48];
        format_uuid(&elems[i].uuid, uuid_s, sizeof(uuid_s));
        bool can_notify = (elems[i].properties & ESP_GATT_CHAR_PROP_BIT_NOTIFY) != 0;
        bool can_indicate = (elems[i].properties & ESP_GATT_CHAR_PROP_BIT_INDICATE) != 0;
        ESP_LOGI(TAG,
                 "GATT %s char %s handle=%u props=0x%02X%s%s",
                 svc_name,
                 uuid_s,
                 elems[i].char_handle,
                 elems[i].properties,
                 can_notify ? " NOTIFY" : "",
                 can_indicate ? " INDICATE" : "");

        if (elems[i].uuid.len == ESP_UUID_LEN_16) {
            uint16_t u16 = elems[i].uuid.uuid.uuid16;
            if (u16 == UUID_CHAR_C306) {
                s_ble.c306_handle = elems[i].char_handle;
                /* Skip C306 indicate mirror — confirm traffic doubles pitch load. */
                continue;
            }
            if ((u16 == UUID_CHAR_C300 || u16 == UUID_CHAR_C301 || u16 == UUID_CHAR_C307) &&
                (elems[i].properties & ESP_GATT_CHAR_PROP_BIT_READ) &&
                s_bpm_read_count < BPM_READ_HANDLE_MAX) {
                s_bpm_read_handles[s_bpm_read_count++] = elems[i].char_handle;
                ESP_LOGI(TAG, "BPM hunt: queued GATT read C%03X handle=%u", (unsigned)u16,
                         elems[i].char_handle);
            }
        }
        if (uuid128_eq(&elems[i].uuid, UUID128_BLE_MIDI_IO)) {
            s_ble.midi_handle = elems[i].char_handle;
        }

        if (can_notify || can_indicate) {
            uint16_t cccd = 0;
            if (can_notify) {
                cccd |= 0x0001;
            }
            if (can_indicate) {
                cccd |= 0x0002;
            }
            add_sub(elems[i].char_handle, cccd);
        }
    }

    free(elems);
}

static bool discover_nano_chars(esp_gatt_if_t gattc_if, uint16_t conn_id)
{
    s_sub_count = 0;
    s_bpm_read_count = 0;
    s_bpm_read_next = 0;

    bool c304 = find_char_in_range(gattc_if, conn_id, s_ble.a002_start, s_ble.a002_end,
                                   UUID_CHAR_C304, &s_ble.c304_handle);
    if (!c304) {
        c304 = find_char_in_range(gattc_if, conn_id, s_ble.a003_start, s_ble.a003_end,
                                  UUID_CHAR_C304, &s_ble.c304_handle);
    }

    bool c305 = find_char_in_range(gattc_if, conn_id, s_ble.a002_start, s_ble.a002_end,
                                   UUID_CHAR_C305, &s_ble.c305_handle);
    if (!c305) {
        c305 = find_char_in_range(gattc_if, conn_id, s_ble.a003_start, s_ble.a003_end,
                                  UUID_CHAR_C305, &s_ble.c305_handle);
    }

    bool c306 = find_char_in_range(gattc_if, conn_id, s_ble.a002_start, s_ble.a002_end,
                                   UUID_CHAR_C306, &s_ble.c306_handle);
    if (!c306) {
        c306 = find_char_in_range(gattc_if, conn_id, s_ble.a003_start, s_ble.a003_end,
                                  UUID_CHAR_C306, &s_ble.c306_handle);
    }

    log_and_collect_notify_chars(gattc_if, conn_id, s_ble.a002_start, s_ble.a002_end, "A002");
    log_and_collect_notify_chars(gattc_if, conn_id, s_ble.a003_start, s_ble.a003_end, "A003");
    for (size_t i = 0; i < s_extra_svc_count; i++) {
        log_and_collect_notify_chars(gattc_if, conn_id, s_extra_svcs[i].start, s_extra_svcs[i].end,
                                     s_extra_svcs[i].name);
    }

    /* Always include known streams even if property scan missed them. */
    if (c305) {
        add_sub(s_ble.c305_handle, 0x0001);
    }
    /*
     * Do NOT subscribe to C306. It INDICATEs a mirror of C305; each indicate
     * needs a confirm and doubles ATT traffic during pitch streaming.
     */
    (void)c306;
    if (s_ble.midi_handle != INVALID_HANDLE) {
        add_sub(s_ble.midi_handle, 0x0001);
    }

    ESP_LOGI(TAG,
             "C304=%u (%s) C305=%u (%s) C306=%u (skip sub) MIDI=%u (%s) subs=%u",
             s_ble.c304_handle, c304 ? "ok" : "missing",
             s_ble.c305_handle, c305 ? "ok" : "missing",
             s_ble.c306_handle,
             s_ble.midi_handle, s_ble.midi_handle != INVALID_HANDLE ? "ok" : "none",
             (unsigned)s_sub_count);
    return c304 && c305;
}

static void enable_all_notifies(esp_gatt_if_t gattc_if)
{
    for (size_t i = 0; i < s_sub_count; i++) {
        esp_err_t err =
            esp_ble_gattc_register_for_notify(gattc_if, s_ble.remote_bda, s_subs[i].handle);
        if (err != ESP_OK) {
            ESP_LOGE(TAG, "register_for_notify handle=%u failed: %s", s_subs[i].handle,
                     esp_err_to_name(err));
        } else {
            const char *lab = handle_label(s_subs[i].handle);
            ESP_LOGI(TAG, "Registered %s handle=%u cccd=0x%04X", lab ? lab : "char",
                     s_subs[i].handle, s_subs[i].cccd_value);
        }
    }
}

static void send_sync_packets(void)
{
    nano_state_begin_sync();
}

static void c305_worker_task(void *arg)
{
    (void)arg;
    c305_queue_item_t item;

    while (true) {
        const nano_tuner_t *tuner = nano_tuner_get();
        bool tuner_live = tuner && tuner->active;

        /* Prefer the live pitch/mode mailbox whenever it has a frame. */
        if (s_c305_live_queue && xQueueReceive(s_c305_live_queue, &item, 0) == pdTRUE) {
            if (tuner_live && s_c305_queue && uxQueueMessagesWaiting(s_c305_queue) > 0) {
                xQueueReset(s_c305_queue);
            }
        } else if (tuner_live) {
            /*
             * While tuning, NEVER drain the normal FIFO — it holds ~4s of stale
             * traffic and replays behind live pitch.
             */
            if (s_c305_queue && uxQueueMessagesWaiting(s_c305_queue) > 0) {
                xQueueReset(s_c305_queue);
            }
            (void)ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(20));
            continue;
        } else if (xQueueReceive(s_c305_queue, &item, 0) != pdTRUE) {
            (void)ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(20));
            continue;
        }

        if (s_ble.c305_handle == INVALID_HANDLE || item.handle != s_ble.c305_handle) {
            const char *lab = handle_label(item.handle);
            ESP_LOGD(TAG, "GATT RX %s h=%u len=%u", lab ? lab : "other", item.handle, item.len);
        }

        nano_state_feed_c305(item.data, item.len);
    }
}

static bool c305_queue_init(void)
{
    if (s_c305_queue) {
        return true;
    }

    s_c305_queue = xQueueCreate(C305_QUEUE_LEN, sizeof(c305_queue_item_t));
    if (!s_c305_queue) {
        ESP_LOGE(TAG, "C305 queue create failed");
        return false;
    }

    s_c305_live_queue = xQueueCreate(C305_LIVE_QUEUE_LEN, sizeof(c305_queue_item_t));
    if (!s_c305_live_queue) {
        ESP_LOGE(TAG, "C305 live queue create failed");
        vQueueDelete(s_c305_queue);
        s_c305_queue = NULL;
        return false;
    }

    if (xTaskCreate(c305_worker_task, "c305_rx", C305_TASK_STACK, NULL, C305_TASK_PRIO, &s_c305_task) !=
        pdPASS) {
        ESP_LOGE(TAG, "C305 worker task create failed");
        vQueueDelete(s_c305_live_queue);
        s_c305_live_queue = NULL;
        vQueueDelete(s_c305_queue);
        s_c305_queue = NULL;
        return false;
    }

    ESP_LOGI(TAG, "C305 RX worker ready (queue=%d live=%d) build=2026-07-26h", C305_QUEUE_LEN,
             C305_LIVE_QUEUE_LEN);
    return true;
}

void nano_ble_drain_rx_queue(void)
{
    if (s_c305_queue) {
        xQueueReset(s_c305_queue);
    }
    if (s_c305_live_queue) {
        xQueueReset(s_c305_live_queue);
    }
    s_c305_drop_count = 0;
}

/** Drop FIFO backlog only — keep the live pitch mailbox. Call when tuner goes live. */
void nano_ble_flush_c305_backlog(void)
{
    if (s_c305_queue) {
        xQueueReset(s_c305_queue);
    }
}

static bool c305_is_pitch(const uint8_t *data, size_t len)
{
    if (!data || len < 5) {
        return false;
    }
    for (size_t i = 0; i + 5 <= len; i++) {
        if (data[i] == 0xC0 && data[i + 1] == 0x08 && data[i + 2] == 0x01 && data[i + 3] == 0x22 &&
            (data[i + 4] == 0x01 || data[i + 4] == 0x02)) {
            return true;
        }
    }
    return false;
}

/** Pitch (22/01|02) or tuner mode (2D / gain+2D) — keep these on the live path. */
static bool c305_is_tuner_hot(const uint8_t *data, size_t len)
{
    if (!data || len < 5) {
        return false;
    }

    for (size_t i = 0; i + 5 <= len; i++) {
        if (data[i] != 0xC0 || data[i + 1] != 0x08 || data[i + 2] != 0x01) {
            continue;
        }
        uint8_t sub = data[i + 3];
        if (sub == 0x22) {
            uint8_t form = data[i + 4];
            if (form == 0x01 || form == 0x02) {
                return true;
            }
        } else if (sub == 0x2D || sub == 0x20) {
            return true;
        }
    }
    return false;
}

static void on_gatt_notify(uint16_t handle, const uint8_t *data, uint16_t len)
{
    if (len == 0 || !s_c305_queue || !data) {
        return;
    }

    const nano_tuner_t *tuner = nano_tuner_get();
    bool tuner_live = tuner && tuner->active;
    bool is_c305 = s_ble.c305_handle != INVALID_HANDLE && handle == s_ble.c305_handle;
    bool is_c306 = s_ble.c306_handle != INVALID_HANDLE && handle == s_ble.c306_handle;

    if (!is_c305 && !is_c306) {
        const char *lab = handle_label(handle);
        ESP_LOGD(TAG, "GATT notify h=%u (%s) len=%u", handle, lab ? lab : "?", len);
    }

    /* C306 mirrors C305 — never queue it (doubles work; stomps live pitch mailbox). */
    if (is_c306) {
        return;
    }

    /*
     * Pitch: apply in the notify callback with no logging and never enqueue.
     * Falling through to the 48-deep FIFO replayed ~4s of stale pitch
     * (48 × ~80 ms). ESP_LOGI in this path also stalls Bluedroid the same way.
     */
    if (is_c305 && c305_is_pitch(data, len)) {
        (void)nano_tuner_feed_pitch_fast(data, len);
        return;
    }

    c305_queue_item_t item = {
        .handle = handle,
        .len = len > C305_PKT_MAX ? C305_PKT_MAX : len,
    };
    memcpy(item.data, data, item.len);

    /*
     * Mode frames use the length-1 overwrite mailbox.
     * While tuner is live, drop everything else on C305.
     */
    if (is_c305 && c305_is_tuner_hot(data, len) && s_c305_live_queue) {
        if (xQueueOverwrite(s_c305_live_queue, &item) == pdPASS) {
            if (s_c305_task) {
                xTaskNotifyGive(s_c305_task);
            }
        } else {
            s_c305_drop_count++;
        }
        return;
    }

    if (tuner_live && is_c305) {
        return;
    }

    if (xQueueSend(s_c305_queue, &item, 0) != pdTRUE) {
        c305_queue_item_t drop;
        if (xQueueReceive(s_c305_queue, &drop, 0) == pdTRUE) {
            s_c305_drop_count++;
            (void)xQueueSend(s_c305_queue, &item, 0);
        }
        if ((s_c305_drop_count % 8U) == 1U) {
            ESP_LOGW(TAG, "C305 RX queue overflow (drops=%u)", (unsigned)s_c305_drop_count);
        }
    } else if (s_c305_task) {
        xTaskNotifyGive(s_c305_task);
    }
}

static void bpm_gatt_read_next(void);

static void gattc_handler(esp_gattc_cb_event_t event, esp_gatt_if_t gattc_if,
                          esp_ble_gattc_cb_param_t *param)
{
    switch (event) {
    case ESP_GATTC_REG_EVT:
        ESP_LOGI(TAG, "GATT client registered");
        esp_ble_gap_set_scan_params(&scan_params);
        break;

    case ESP_GATTC_CONNECT_EVT:
        s_ble.connected = true;
        s_ble.scanning = false;
        s_ble.conn_id = param->connect.conn_id;
        s_ble.gattc_if = gattc_if;
        memcpy(s_ble.remote_bda, param->connect.remote_bda, sizeof(esp_bd_addr_t));
        reset_handles();
        ESP_LOGI(TAG, "Connected to Nano Cortex");
        {
            /* Prefer a short interval so pitch notifies are not paced at ~50ms+. */
            esp_ble_conn_update_params_t conn = {
                .min_int = 0x06, /* 7.5 ms */
                .max_int = 0x10, /* 20 ms */
                .latency = 0,
                .timeout = 400, /* 4 s */
            };
            memcpy(conn.bda, s_ble.remote_bda, sizeof(esp_bd_addr_t));
            esp_ble_gap_update_conn_params(&conn);
        }
        esp_ble_gattc_send_mtu_req(gattc_if, param->connect.conn_id);
        break;

    case ESP_GATTC_CFG_MTU_EVT:
        ESP_LOGI(TAG, "MTU %u", param->cfg_mtu.mtu);
        esp_ble_gattc_search_service(gattc_if, s_ble.conn_id, NULL);
        break;

    case ESP_GATTC_SEARCH_RES_EVT: {
        char uuid_s[48];
        format_uuid(&param->search_res.srvc_id.uuid, uuid_s, sizeof(uuid_s));
        ESP_LOGI(TAG, "Found service %s handles %u-%u", uuid_s, param->search_res.start_handle,
                 param->search_res.end_handle);

        if (param->search_res.srvc_id.uuid.len == ESP_UUID_LEN_16) {
            uint16_t id = param->search_res.srvc_id.uuid.uuid.uuid16;
            if (id == UUID_SVC_A002) {
                s_ble.a002_start = param->search_res.start_handle;
                s_ble.a002_end = param->search_res.end_handle;
            } else if (id == UUID_SVC_A003) {
                s_ble.a003_start = param->search_res.start_handle;
                s_ble.a003_end = param->search_res.end_handle;
            } else if (s_extra_svc_count < GATT_EXTRA_SVC_MAX) {
                s_extra_svcs[s_extra_svc_count].start = param->search_res.start_handle;
                s_extra_svcs[s_extra_svc_count].end = param->search_res.end_handle;
                snprintf(s_extra_svcs[s_extra_svc_count].name,
                         sizeof(s_extra_svcs[s_extra_svc_count].name), "0x%04X", id);
                s_extra_svc_count++;
            }
        } else if (s_extra_svc_count < GATT_EXTRA_SVC_MAX) {
            s_extra_svcs[s_extra_svc_count].start = param->search_res.start_handle;
            s_extra_svcs[s_extra_svc_count].end = param->search_res.end_handle;
            if (uuid128_eq(&param->search_res.srvc_id.uuid, UUID128_BLE_MIDI_SVC)) {
                snprintf(s_extra_svcs[s_extra_svc_count].name,
                         sizeof(s_extra_svcs[s_extra_svc_count].name), "BLE-MIDI");
            } else {
                snprintf(s_extra_svcs[s_extra_svc_count].name,
                         sizeof(s_extra_svcs[s_extra_svc_count].name), "%.20s", uuid_s);
            }
            s_extra_svc_count++;
        }
        break;
    }

    case ESP_GATTC_SEARCH_CMPL_EVT:
        if (param->search_cmpl.status != ESP_GATT_OK) {
            ESP_LOGE(TAG, "Service discovery failed");
            break;
        }
        if (!discover_nano_chars(gattc_if, s_ble.conn_id)) {
            ESP_LOGE(TAG, "C304/C305 not found — disconnecting");
            esp_ble_gattc_close(gattc_if, s_ble.conn_id);
            break;
        }
        enable_all_notifies(gattc_if);
        break;

    case ESP_GATTC_REG_FOR_NOTIFY_EVT:
        if (param->reg_for_notify.status != ESP_GATT_OK) {
            ESP_LOGE(TAG, "Notify registration failed handle=%u", param->reg_for_notify.handle);
            break;
        }
        {
            uint16_t count = 1;
            esp_gattc_descr_elem_t descr;
            if (esp_ble_gattc_get_descr_by_char_handle(gattc_if, s_ble.conn_id,
                                                       param->reg_for_notify.handle, uuid_cccd,
                                                       &descr, &count) != ESP_GATT_OK ||
                count == 0) {
                ESP_LOGE(TAG, "CCCD not found handle=%u", param->reg_for_notify.handle);
                break;
            }
            /* C306 is INDICATE — writing 0x0001 alone never enables it. */
            uint16_t cccd_en = cccd_for_handle(param->reg_for_notify.handle);
            const char *lab = handle_label(param->reg_for_notify.handle);
            ESP_LOGI(TAG, "CCCD write %s handle=%u value=0x%04X", lab ? lab : "char",
                     param->reg_for_notify.handle, cccd_en);
            esp_ble_gattc_write_char_descr(gattc_if, s_ble.conn_id, descr.handle, sizeof(cccd_en),
                                           (uint8_t *)&cccd_en, ESP_GATT_WRITE_TYPE_RSP,
                                           ESP_GATT_AUTH_REQ_NONE);
        }
        break;

    case ESP_GATTC_WRITE_DESCR_EVT:
        if (param->write.status == ESP_GATT_OK && !s_ble.notify_enabled) {
            s_ble.notify_enabled = true;
            ESP_LOGI(TAG, "GATT subscriptions enabled (C305/C306/…) — syncing");
            nano_ui_set_ble_connected(true);
            send_sync_packets();
        }
        break;

    case ESP_GATTC_NOTIFY_EVT:
        on_gatt_notify(param->notify.handle, param->notify.value, param->notify.value_len);
        break;

    case ESP_GATTC_WRITE_CHAR_EVT:
        if (param->write.status != ESP_GATT_OK) {
            ESP_LOGE(TAG, "C304 write failed status=0x%x", param->write.status);
        }
        break;

    case ESP_GATTC_READ_CHAR_EVT: {
        if (param->read.status != ESP_GATT_OK) {
            ESP_LOGW(TAG, "BPM hunt: GATT read status=%d handle=%u", param->read.status,
                     param->read.handle);
        } else {
            char lab[32];
            snprintf(lab, sizeof(lab), "GATT read h=%u", param->read.handle);
            nano_log_bytes_full(TAG, lab, param->read.value, param->read.value_len);
            nano_log_bpm_raw_scan(TAG, lab, param->read.value, param->read.value_len);
            uint16_t bpm = 0;
            if (nano_try_parse_bpm_notify(param->read.value, param->read.value_len, &bpm) ||
                nano_parse_bpm(param->read.value, param->read.value_len, &bpm)) {
                ESP_LOGI(TAG, "BPM %u (GATT read)", (unsigned)bpm);
                nano_state_on_bpm_found(bpm);
            }
        }
        s_bpm_read_next++;
        bpm_gatt_read_next();
        break;
    }

    case ESP_GATTC_DISCONNECT_EVT:
        ESP_LOGW(TAG, "Disconnected reason=0x%02x — will rescan", param->disconnect.reason);
        s_ble.connected = false;
        s_ble.gattc_if = ESP_GATT_IF_NONE;
        reset_handles();
        nano_ble_drain_rx_queue();
        nano_ui_set_ble_connected(false);
        nano_ui_set_amp(NULL);
        nano_state_reset();
        vTaskDelay(pdMS_TO_TICKS(2000));
        start_scan();
        break;

    default:
        break;
    }
}

static void gattc_cb(esp_gattc_cb_event_t event, esp_gatt_if_t gattc_if,
                     esp_ble_gattc_cb_param_t *param)
{
    if (event == ESP_GATTC_REG_EVT && param->reg.status == ESP_GATT_OK) {
        s_ble.gattc_if = gattc_if;
    }
    if (gattc_if == ESP_GATT_IF_NONE || gattc_if == s_ble.gattc_if) {
        gattc_handler(event, gattc_if, param);
    }
}

static void gap_cb(esp_gap_ble_cb_event_t event, esp_ble_gap_cb_param_t *param)
{
    switch (event) {
    case ESP_GAP_BLE_SCAN_PARAM_SET_COMPLETE_EVT:
        start_scan();
        break;

    case ESP_GAP_BLE_SCAN_START_COMPLETE_EVT:
        if (param->scan_start_cmpl.status != ESP_BT_STATUS_SUCCESS) {
            ESP_LOGE(TAG, "Scan start complete failed");
            s_ble.scanning = false;
        }
        break;

    case ESP_GAP_BLE_SCAN_RESULT_EVT:
        if (param->scan_rst.search_evt != ESP_GAP_SEARCH_INQ_RES_EVT || s_ble.connected) {
            break;
        }
        {
            uint8_t adv_name_len = 0;
            uint8_t *adv_name = esp_ble_resolve_adv_data_by_type(
                param->scan_rst.ble_adv,
                param->scan_rst.adv_data_len + param->scan_rst.scan_rsp_len,
                ESP_BLE_AD_TYPE_NAME_CMPL, &adv_name_len);
            if (!adv_name || adv_name_len < sizeof(NANO_BLE_NAME_PREFIX) - 1) {
                break;
            }
            if (strncmp((char *)adv_name, NANO_BLE_NAME_PREFIX, sizeof(NANO_BLE_NAME_PREFIX) - 1) !=
                0) {
                break;
            }
            ESP_LOGI(TAG, "Found Nano Cortex, connecting...");
            esp_ble_gap_stop_scanning();
            s_ble.scanning = false;
            esp_ble_gattc_open(s_ble.gattc_if, param->scan_rst.bda, param->scan_rst.ble_addr_type,
                               true);
        }
        break;

    case ESP_GAP_BLE_SCAN_STOP_COMPLETE_EVT:
        s_ble.scanning = false;
        break;

    default:
        break;
    }
}

bool nano_ble_connected(void)
{
    return s_ble.connected && s_ble.c304_handle != INVALID_HANDLE && s_ble.notify_enabled;
}

bool nano_ble_scanning(void)
{
    return s_ble.scanning;
}

esp_err_t nano_ble_write_c304(const char *label, const uint8_t *data, size_t len)
{
    if (!nano_ble_connected()) {
        ESP_LOGW(TAG, "Skip write (%s): not ready", label ? label : "?");
        return ESP_ERR_INVALID_STATE;
    }
    if (len == 0 || len > 512) {
        return ESP_ERR_INVALID_ARG;
    }

    nano_log_bytes(TAG, label ? label : "C304", data, len);

    esp_err_t err = esp_ble_gattc_write_char(
        s_ble.gattc_if, s_ble.conn_id, s_ble.c304_handle, len, (uint8_t *)data,
        ESP_GATT_WRITE_TYPE_RSP, ESP_GATT_AUTH_REQ_NONE);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "write_char failed: %s", esp_err_to_name(err));
    }
    return err;
}

static void bpm_gatt_read_next(void)
{
    if (!nano_ble_connected() || s_bpm_read_next >= s_bpm_read_count) {
        return;
    }
    uint16_t handle = s_bpm_read_handles[s_bpm_read_next];
    ESP_LOGI(TAG, "BPM hunt: GATT read handle=%u (%u/%u)", handle,
             (unsigned)(s_bpm_read_next + 1), (unsigned)s_bpm_read_count);
    esp_err_t err = esp_ble_gattc_read_char(s_ble.gattc_if, s_ble.conn_id, handle,
                                           ESP_GATT_AUTH_REQ_NONE);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "BPM hunt: read_char failed: %s", esp_err_to_name(err));
        s_bpm_read_next++;
        bpm_gatt_read_next();
    }
}

void nano_ble_read_bpm_probe_chars(void)
{
    if (!nano_ble_connected()) {
        ESP_LOGW(TAG, "BPM hunt: GATT read skipped (not connected)");
        return;
    }
    if (s_bpm_read_count == 0) {
        ESP_LOGW(TAG, "BPM hunt: no readable C300/C301/C307 handles");
        return;
    }
    s_bpm_read_next = 0;
    bpm_gatt_read_next();
}

esp_err_t nano_ble_start(void)
{
    memset(&s_ble, 0, sizeof(s_ble));
    s_ble.gattc_if = ESP_GATT_IF_NONE;
    reset_handles();

    esp_err_t ret = nvs_flash_init();
    if (ret == ESP_ERR_NVS_NO_FREE_PAGES || ret == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        ESP_ERROR_CHECK(nvs_flash_erase());
        ret = nvs_flash_init();
    }
    ESP_ERROR_CHECK(ret);

    ESP_ERROR_CHECK(esp_bt_controller_mem_release(ESP_BT_MODE_CLASSIC_BT));

    esp_bt_controller_config_t bt_cfg = BT_CONTROLLER_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_bt_controller_init(&bt_cfg));
    ESP_ERROR_CHECK(esp_bt_controller_enable(ESP_BT_MODE_BLE));
    ESP_ERROR_CHECK(esp_bluedroid_init());
    ESP_ERROR_CHECK(esp_bluedroid_enable());

    ESP_ERROR_CHECK(esp_ble_gap_register_callback(gap_cb));
    ESP_ERROR_CHECK(esp_ble_gattc_register_callback(gattc_cb));
    ESP_ERROR_CHECK(esp_ble_gattc_app_register(PROFILE_APP_ID));

    if (!c305_queue_init()) {
        return ESP_ERR_NO_MEM;
    }

    ESP_LOGI(TAG, "BLE stack ready");
    return ESP_OK;
}
