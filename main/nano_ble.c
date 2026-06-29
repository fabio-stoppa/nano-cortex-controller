/*
 * BLE GATT client for Neural DSP Nano Cortex.
 * Services A002/A003, C304 write, C305 notify — same as index.html.
 */

#include "nano_ble.h"
#include "nano_state.h"
#include "nano_ui.h"

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

static const char *TAG = "nano_ble";

#define C305_QUEUE_LEN 48
#define C305_PKT_MAX 256
#define C305_TASK_STACK 4096
#define C305_TASK_PRIO 5

typedef struct {
    uint16_t len;
    uint8_t data[C305_PKT_MAX];
} c305_queue_item_t;

static QueueHandle_t s_c305_queue;
static TaskHandle_t s_c305_task;
static uint32_t s_c305_drop_count;

#define PROFILE_APP_ID 0
#define INVALID_HANDLE 0

#define UUID_SVC_A002 0xA002
#define UUID_SVC_A003 0xA003
#define UUID_CHAR_C304  0xC304
#define UUID_CHAR_C305  0xC305

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

static void reset_handles(void)
{
    s_ble.a002_start = INVALID_HANDLE;
    s_ble.a002_end = INVALID_HANDLE;
    s_ble.a003_start = INVALID_HANDLE;
    s_ble.a003_end = INVALID_HANDLE;
    s_ble.c304_handle = INVALID_HANDLE;
    s_ble.c305_handle = INVALID_HANDLE;
    s_ble.notify_enabled = false;
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

static bool discover_nano_chars(esp_gatt_if_t gattc_if, uint16_t conn_id)
{
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

    ESP_LOGI(TAG, "C304 handle=%u (%s), C305 handle=%u (%s)",
             s_ble.c304_handle, c304 ? "ok" : "missing",
             s_ble.c305_handle, c305 ? "ok" : "missing");
    return c304 && c305;
}

static void enable_c305_notify(esp_gatt_if_t gattc_if)
{
    esp_err_t err = esp_ble_gattc_register_for_notify(gattc_if, s_ble.remote_bda, s_ble.c305_handle);
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "register_for_notify failed: %s", esp_err_to_name(err));
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
        if (xQueueReceive(s_c305_queue, &item, portMAX_DELAY) != pdTRUE) {
            continue;
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

    if (xTaskCreate(c305_worker_task, "c305_rx", C305_TASK_STACK, NULL, C305_TASK_PRIO, &s_c305_task) !=
        pdPASS) {
        ESP_LOGE(TAG, "C305 worker task create failed");
        vQueueDelete(s_c305_queue);
        s_c305_queue = NULL;
        return false;
    }

    ESP_LOGI(TAG, "C305 RX worker ready (queue=%d)", C305_QUEUE_LEN);
    return true;
}

void nano_ble_drain_rx_queue(void)
{
    if (!s_c305_queue) {
        return;
    }
    xQueueReset(s_c305_queue);
    s_c305_drop_count = 0;
}

static void on_c305_notify(const uint8_t *data, uint16_t len)
{
    if (len == 0 || !s_c305_queue) {
        return;
    }

    c305_queue_item_t item = {
        .len = len > C305_PKT_MAX ? C305_PKT_MAX : len,
    };
    memcpy(item.data, data, item.len);

    if (xQueueSend(s_c305_queue, &item, 0) != pdTRUE) {
        c305_queue_item_t drop;
        if (xQueueReceive(s_c305_queue, &drop, 0) == pdTRUE) {
            s_c305_drop_count++;
            (void)xQueueSend(s_c305_queue, &item, 0);
        }
        if ((s_c305_drop_count % 8U) == 1U) {
            ESP_LOGW(TAG, "C305 RX queue overflow (drops=%u)", (unsigned)s_c305_drop_count);
        }
    }
}

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
        esp_ble_gattc_send_mtu_req(gattc_if, param->connect.conn_id);
        break;

    case ESP_GATTC_CFG_MTU_EVT:
        ESP_LOGI(TAG, "MTU %u", param->cfg_mtu.mtu);
        esp_ble_gattc_search_service(gattc_if, s_ble.conn_id, NULL);
        break;

    case ESP_GATTC_SEARCH_RES_EVT: {
        uint16_t id = param->search_res.srvc_id.uuid.uuid.uuid16;
        if (param->search_res.srvc_id.uuid.len == ESP_UUID_LEN_16) {
            if (id == UUID_SVC_A002) {
                s_ble.a002_start = param->search_res.start_handle;
                s_ble.a002_end = param->search_res.end_handle;
                ESP_LOGI(TAG, "Found service A002");
            } else if (id == UUID_SVC_A003) {
                s_ble.a003_start = param->search_res.start_handle;
                s_ble.a003_end = param->search_res.end_handle;
                ESP_LOGI(TAG, "Found service A003");
            }
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
        enable_c305_notify(gattc_if);
        break;

    case ESP_GATTC_REG_FOR_NOTIFY_EVT:
        if (param->reg_for_notify.status != ESP_GATT_OK) {
            ESP_LOGE(TAG, "Notify registration failed");
            break;
        }
        {
            uint16_t count = 1;
            esp_gattc_descr_elem_t descr;
            if (esp_ble_gattc_get_descr_by_char_handle(gattc_if, s_ble.conn_id,
                                                       param->reg_for_notify.handle, uuid_cccd,
                                                       &descr, &count) != ESP_GATT_OK ||
                count == 0) {
                ESP_LOGE(TAG, "CCCD not found");
                break;
            }
            uint16_t notify_en = 1;
            esp_ble_gattc_write_char_descr(gattc_if, s_ble.conn_id, descr.handle,
                                           sizeof(notify_en), (uint8_t *)&notify_en,
                                           ESP_GATT_WRITE_TYPE_RSP, ESP_GATT_AUTH_REQ_NONE);
        }
        break;

    case ESP_GATTC_WRITE_DESCR_EVT:
        if (param->write.status == ESP_GATT_OK && !s_ble.notify_enabled) {
            s_ble.notify_enabled = true;
            ESP_LOGI(TAG, "C305 notifications enabled — syncing");
            nano_ui_set_ble_connected(true);
            send_sync_packets();
        }
        break;

    case ESP_GATTC_NOTIFY_EVT:
        on_c305_notify(param->notify.value, param->notify.value_len);
        break;

    case ESP_GATTC_WRITE_CHAR_EVT:
        if (param->write.status != ESP_GATT_OK) {
            ESP_LOGE(TAG, "C304 write failed status=0x%x", param->write.status);
        }
        break;

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
