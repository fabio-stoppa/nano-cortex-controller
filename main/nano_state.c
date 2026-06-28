#include "nano_state.h"

#include <stdlib.h>
#include <string.h>

#include "esp_log.h"
#include "esp_timer.h"

#include "nano_ble.h"
#include "nano_protocol.h"
#include "nano_proto.h"
#include "nano_tuner.h"
#include "nano_ui.h"

static const char *TAG = "nano_state";

#define STATE_BUF_MAX (48 * 1024)
#define PARSE_DEBOUNCE_US (600 * 1000)
#define STATE_REQUEST_DELAY_US (2200 * 1000)
#define REFRESH_DEBOUNCE_US (800 * 1000)
#define REFRESH_MIN_INTERVAL_US (1500 * 1000)
#define LIVE_POLL_INTERVAL_US (15000 * 1000)
#define SNIFF_MIN_INTERVAL_US (500 * 1000)
#define TUNER_FAST_SYNC_US (400 * 1000)
#define TUNER_WATCH_POLL_US (5000 * 1000)
#define STATE_RX_TIMEOUT_US (5000 * 1000)
#define STATE_PROBE_LABEL_MAX 24

static nano_rx_mode_t s_mode = NANO_RX_IDLE;
static nano_amp_t s_amp;
static uint8_t *s_state_buf;
static size_t s_state_len;
static esp_timer_handle_t s_parse_timer;
static esp_timer_handle_t s_sync_timer;
static esp_timer_handle_t s_refresh_timer;
static esp_timer_handle_t s_poll_timer;
static esp_timer_handle_t s_tuner_fast_sync_timer;
static esp_timer_handle_t s_tuner_watch_timer;
static int64_t s_last_refresh_us;
static int64_t s_last_sniff_us;
static int64_t s_state_rx_since_us;
static bool s_live_poll_active;
static bool s_tuner_fast_sync_armed;
static bool s_tuner_watch_running;
static bool s_pending_tuner_sync;
static char s_state_probe_label[STATE_PROBE_LABEL_MAX];

static void append_state_payload(const uint8_t *data, size_t len)
{
    if (len <= 2 || !s_state_buf) {
        return;
    }
    const uint8_t *payload = data + 2;
    size_t plen = len - 2;
    if (s_state_len + plen > STATE_BUF_MAX) {
        ESP_LOGW(TAG, "State buffer full, dropping %u bytes", (unsigned)plen);
        return;
    }
    memcpy(s_state_buf + s_state_len, payload, plen);
    s_state_len += plen;
    s_state_rx_since_us = esp_timer_get_time();
}

static void finish_state_capture(void)
{
    s_state_len = 0;
    s_mode = NANO_RX_IDLE;
    s_state_rx_since_us = 0;
}

static void abort_stale_state_rx(const char *why)
{
    if (s_mode != NANO_RX_CURRENT) {
        return;
    }
    ESP_LOGW(TAG, "State RX aborted (%s, had %u bytes)", why, (unsigned)s_state_len);
    finish_state_capture();
}

static void check_state_rx_timeout(void)
{
    if (s_mode != NANO_RX_CURRENT || s_state_rx_since_us == 0) {
        return;
    }
    if (esp_timer_get_time() - s_state_rx_since_us > STATE_RX_TIMEOUT_US) {
        abort_stale_state_rx("timeout");
    }
}

static void maybe_retry_tuner_sync(void)
{
    if (!s_pending_tuner_sync) {
        return;
    }
    s_pending_tuner_sync = false;
    nano_state_request_tuner_sync();
}

static void start_live_poll(void)
{
    if (!s_poll_timer || s_live_poll_active) {
        return;
    }
    s_live_poll_active = true;
    esp_timer_start_periodic(s_poll_timer, LIVE_POLL_INTERVAL_US);
    ESP_LOGI(TAG, "Live amp sync: polling state every %ds", (int)(LIVE_POLL_INTERVAL_US / 1000000));
}

static void send_state_request(const char *label, bool from_initial_sync)
{
    if (s_mode == NANO_RX_CURRENT) {
        check_state_rx_timeout();
        if (s_mode == NANO_RX_CURRENT) {
            return;
        }
    }
    if (!from_initial_sync && s_mode != NANO_RX_IDLE) {
        if (label && strstr(label, "Tuner") != NULL) {
            s_pending_tuner_sync = true;
        }
        return;
    }

    int64_t now = esp_timer_get_time();
    bool force_probe = s_state_probe_label[0] != '\0' || s_tuner_fast_sync_armed;
    if (!from_initial_sync && !force_probe && now - s_last_refresh_us < REFRESH_MIN_INTERVAL_US) {
        return;
    }
    s_tuner_fast_sync_armed = false;
    s_last_refresh_us = now;

    uint8_t buf[32];
    nano_rx_mode_set(NANO_RX_CURRENT);
    s_state_rx_since_us = esp_timer_get_time();
    size_t n = nano_request_current_state(buf, sizeof(buf));
    if (n > 0) {
        ESP_LOGI(TAG, "Sending preset state request");
        nano_ble_write_c304(label ? label : "Amp state refresh", buf, n);
    }
}

static void request_current_state_now(const char *label)
{
    send_state_request(label, false);
}

static void parse_state_buffer(void)
{
    if (s_state_len == 0) {
        return;
    }

    const char *label = s_state_probe_label[0] ? s_state_probe_label : "ROUTINE";
    ESP_LOGI(TAG, "Parsing preset state (%u bytes) [%s]", (unsigned)s_state_len, label);
    if (strcmp(label, "INITIAL") == 0) {
        nano_log_proto_fields(TAG, label, s_state_buf, s_state_len);
    }
    s_state_probe_label[0] = '\0';

    if (nano_parse_amp(s_state_buf, s_state_len, &s_amp)) {
        nano_log_amp(TAG, &s_amp);
        nano_ui_set_amp(&s_amp);
        start_live_poll();
    } else {
        ESP_LOGW(TAG, "Could not find amp fields 3–7 in state blob");
    }

    finish_state_capture();

    bool tuner_active = false;
    nano_tuner_route_t tuner_route = NANO_TUNER_ROUTE_UNKNOWN;
    if (nano_parse_tuner_state(s_state_buf, s_state_len, &tuner_active, &tuner_route)) {
        if (strcmp(label, "ROUTE") == 0 || strcmp(label, "TUNER") == 0 || tuner_active ||
            nano_tuner_get()->active) {
            nano_log_tuner_state_fields(TAG, s_state_buf, s_state_len);
        }
        nano_tuner_sync_from_state(tuner_active, tuner_route);
    } else {
        if (strcmp(label, "ROUTE") == 0 || strcmp(label, "TUNER") == 0 || nano_tuner_get()->active) {
            nano_log_tuner_state_fields(TAG, s_state_buf, s_state_len);
        }
        nano_state_tuner_watch_update(nano_tuner_get()->active);
    }

    maybe_retry_tuner_sync();
}

static void parse_timer_cb(void *arg)
{
    (void)arg;
    parse_state_buffer();
}

static void refresh_timer_cb(void *arg)
{
    (void)arg;
    request_current_state_now("Debounced amp refresh");
}

static void poll_timer_cb(void *arg)
{
    (void)arg;
    if (s_mode == NANO_RX_IDLE && s_amp.valid) {
        request_current_state_now("Live amp poll");
    }
}

static void tuner_fast_sync_timer_cb(void *arg)
{
    (void)arg;
    if (s_mode != NANO_RX_IDLE) {
        s_pending_tuner_sync = true;
        return;
    }
    s_tuner_fast_sync_armed = true;
    snprintf(s_state_probe_label, sizeof(s_state_probe_label), "TUNER");
    send_state_request("Tuner fast sync", false);
}

static void tuner_watch_poll_cb(void *arg)
{
    (void)arg;

    const nano_tuner_t *tuner = nano_tuner_get();
    if (tuner && tuner->active) {
        nano_state_tuner_watch_update(true);
        return;
    }
    if (s_mode != NANO_RX_IDLE || !s_amp.valid) {
        return;
    }

    request_current_state_now("Tuner watch");
}

static void schedule_parse(void)
{
    if (!s_parse_timer) {
        return;
    }
    esp_timer_stop(s_parse_timer);
    esp_timer_start_once(s_parse_timer, PARSE_DEBOUNCE_US);
}

static void schedule_refresh(void)
{
    if (!s_refresh_timer || s_mode != NANO_RX_IDLE) {
        return;
    }
    esp_timer_stop(s_refresh_timer);
    esp_timer_start_once(s_refresh_timer, REFRESH_DEBOUNCE_US);
}

static bool is_fe_stream(const uint8_t *data, size_t len, nano_rx_mode_t mode)
{
    if (len < 3) {
        return false;
    }
    if (data[0] == 0xFE || data[0] == 0xFD || data[0] == 0xCE || data[0] == 0xD0) {
        return true;
    }
    if ((mode == NANO_RX_METADATA || mode == NANO_RX_CURRENT) && (data[1] & 0x80)) {
        return true;
    }
    return false;
}

/** Preset state stream uses FE 41; metadata uses FE 01 and must not enter the state buffer. */
static bool is_state_stream_chunk(const uint8_t *data, size_t len)
{
    return len >= 3 && data[0] == 0xFE && data[1] == 0x41;
}

static void sniff_unknown_c305(const uint8_t *data, size_t len)
{
    int64_t now = esp_timer_get_time();
    if (now - s_last_sniff_us < SNIFF_MIN_INTERVAL_US) {
        return;
    }
    s_last_sniff_us = now;
    nano_log_bytes(TAG, "C305 sniff (turn a knob — capture this)", data, len > 32 ? 32 : len);
}

static bool apply_knob_update(uint8_t knob_id, uint8_t raw, const char *source)
{
    if (knob_id > 4) {
        return false;
    }

    uint8_t *fields[] = {
        &s_amp.gain, &s_amp.level, &s_amp.bass, &s_amp.mid, &s_amp.treble,
    };
    static const char *names[] = {"gain", "level", "bass", "mid", "treble"};

    if (s_amp.valid && *fields[knob_id] == raw) {
        return false;
    }

    *fields[knob_id] = raw;
    s_amp.valid = true;
    ESP_LOGI(TAG, "Amp %s -> raw %u (%s)", names[knob_id], raw, source);
    nano_ui_set_amp(&s_amp);
    return true;
}

static bool try_inline_amp_update(const uint8_t *data, size_t len)
{
    uint8_t knob_id = 0;
    uint8_t raw = 0;

    if (nano_try_parse_amp_hw_notify(data, len, &knob_id, &raw)) {
        if (apply_knob_update(knob_id, raw, "hw knob")) {
            if (s_refresh_timer) {
                esp_timer_stop(s_refresh_timer);
            }
        }
        return true;
    }

    if (nano_try_parse_amp_knob_notify(data, len, &knob_id, &raw)) {
        return apply_knob_update(knob_id, raw, "knob notify");
    }

    return false;
}

static void sync_timer_cb(void *arg)
{
    (void)arg;
    ESP_LOGI(TAG, "Initial sync timer — requesting preset state");
    send_state_request("Current preset state request", true);
}

static void ensure_timers(void)
{
    if (!s_parse_timer) {
        const esp_timer_create_args_t parse_args = {
            .callback = parse_timer_cb,
            .name = "nano_parse",
        };
        ESP_ERROR_CHECK(esp_timer_create(&parse_args, &s_parse_timer));
    }
    if (!s_sync_timer) {
        const esp_timer_create_args_t sync_args = {
            .callback = sync_timer_cb,
            .name = "nano_sync",
        };
        ESP_ERROR_CHECK(esp_timer_create(&sync_args, &s_sync_timer));
    }
    if (!s_refresh_timer) {
        const esp_timer_create_args_t refresh_args = {
            .callback = refresh_timer_cb,
            .name = "nano_refresh",
        };
        ESP_ERROR_CHECK(esp_timer_create(&refresh_args, &s_refresh_timer));
    }
    if (!s_poll_timer) {
        const esp_timer_create_args_t poll_args = {
            .callback = poll_timer_cb,
            .name = "nano_poll",
        };
        ESP_ERROR_CHECK(esp_timer_create(&poll_args, &s_poll_timer));
    }
    if (!s_tuner_fast_sync_timer) {
        const esp_timer_create_args_t fast_sync_args = {
            .callback = tuner_fast_sync_timer_cb,
            .name = "nano_tsync",
        };
        ESP_ERROR_CHECK(esp_timer_create(&fast_sync_args, &s_tuner_fast_sync_timer));
    }
    if (!s_tuner_watch_timer) {
        const esp_timer_create_args_t watch_args = {
            .callback = tuner_watch_poll_cb,
            .name = "nano_twatch",
        };
        ESP_ERROR_CHECK(esp_timer_create(&watch_args, &s_tuner_watch_timer));
    }
}

static void reset_amp_sync(void)
{
    s_mode = NANO_RX_IDLE;
    s_state_len = 0;
    s_live_poll_active = false;
    s_last_refresh_us = 0;
    s_last_sniff_us = 0;
    s_state_probe_label[0] = '\0';
    s_tuner_watch_running = false;
    s_pending_tuner_sync = false;
    s_state_rx_since_us = 0;
    memset(&s_amp, 0, sizeof(s_amp));
    if (s_parse_timer) {
        esp_timer_stop(s_parse_timer);
    }
    if (s_sync_timer) {
        esp_timer_stop(s_sync_timer);
    }
    if (s_refresh_timer) {
        esp_timer_stop(s_refresh_timer);
    }
    if (s_poll_timer) {
        esp_timer_stop(s_poll_timer);
    }
    if (s_tuner_fast_sync_timer) {
        esp_timer_stop(s_tuner_fast_sync_timer);
    }
    if (s_tuner_watch_timer) {
        esp_timer_stop(s_tuner_watch_timer);
    }
    s_tuner_watch_running = false;
}

void nano_state_reset(void)
{
    reset_amp_sync();
    nano_tuner_reset();
}

void nano_rx_mode_set(nano_rx_mode_t mode)
{
    s_mode = mode;
    if (mode == NANO_RX_CURRENT) {
        s_state_len = 0;
    }
}

nano_rx_mode_t nano_rx_mode_get(void)
{
    return s_mode;
}

void nano_state_feed_c305(const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        return;
    }

    check_state_rx_timeout();

    if (len >= 2 && data[0] == 0x10 && data[1] == 0xC0) {
        ESP_LOGD(TAG, "Preset change ACK");
        return;
    }

    if (is_state_stream_chunk(data, len)) {
        if (s_mode == NANO_RX_METADATA) {
            ESP_LOGD(TAG, "State stream chunk before request (%u bytes)", (unsigned)len);
            return;
        }
        if (s_mode == NANO_RX_IDLE) {
            s_state_len = 0;
            s_mode = NANO_RX_CURRENT;
            s_state_rx_since_us = esp_timer_get_time();
        }
        append_state_payload(data, len);
        schedule_parse();
        return;
    }

    if (is_fe_stream(data, len, s_mode)) {
        if (s_mode == NANO_RX_METADATA) {
            ESP_LOGD(TAG, "Metadata chunk (%u bytes)", (unsigned)len);
        }
        return;
    }

    if (try_inline_amp_update(data, len)) {
        return;
    }

    if (nano_tuner_feed_c305(data, len)) {
        return;
    }

    if (s_mode == NANO_RX_IDLE) {
        sniff_unknown_c305(data, len);
        schedule_refresh();
    } else {
        ESP_LOGD(TAG, "Other C305 packet (%u bytes), first=0x%02X", (unsigned)len, data[0]);
    }
}

const nano_amp_t *nano_state_get_amp(void)
{
    return s_amp.valid ? &s_amp : NULL;
}

bool nano_state_write_amp_knob(nano_knob_t knob, uint8_t raw)
{
    if (knob > NANO_KNOB_TREBLE || !nano_ble_connected()) {
        return false;
    }

    uint8_t *fields[] = {
        &s_amp.gain, &s_amp.level, &s_amp.bass, &s_amp.mid, &s_amp.treble,
    };
    static const char *names[] = {"gain", "level", "bass", "mid", "treble"};

    *fields[knob] = raw;
    s_amp.valid = true;

    uint8_t buf[32];
    size_t n = nano_amp_knob_payload(knob, raw, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }

    char label[32];
    snprintf(label, sizeof(label), "Amp %s = %u", names[knob], raw);
    ESP_LOGI(TAG, "Amp %s -> raw %u (touch)", names[knob], raw);
    return nano_ble_write_c304(label, buf, n) == ESP_OK;
}

void nano_state_begin_sync(void)
{
    uint8_t buf[32];

    /* Amp/metadata sync only — tuner mode is learned from live C305 traffic. */
    reset_amp_sync();
    if (!s_state_buf) {
        s_state_buf = malloc(STATE_BUF_MAX);
        if (!s_state_buf) {
            ESP_LOGE(TAG, "Failed to allocate state buffer");
            return;
        }
    }

    ensure_timers();

    nano_rx_mode_set(NANO_RX_METADATA);

    size_t n = nano_fe_metadata_request(buf, sizeof(buf));
    if (n > 0) {
        nano_ble_write_c304("FE metadata request", buf, n);
    }

    esp_timer_stop(s_sync_timer);
    esp_timer_start_once(s_sync_timer, STATE_REQUEST_DELAY_US);
    snprintf(s_state_probe_label, sizeof(s_state_probe_label), "INITIAL");
    ESP_LOGI(TAG, "Sync started — preset state request in 2.2s");
}

void nano_state_request_tuner_sync(void)
{
    ensure_timers();
    if (!s_tuner_fast_sync_timer) {
        return;
    }
    esp_timer_stop(s_tuner_fast_sync_timer);
    esp_timer_start_once(s_tuner_fast_sync_timer, TUNER_FAST_SYNC_US);
}

void nano_state_tuner_watch_update(bool tuner_active)
{
    ensure_timers();
    if (!s_tuner_watch_timer) {
        return;
    }

    if (tuner_active) {
        /* Tuner live: route/mode come from C305 only — do not poll preset state. */
        if (s_tuner_watch_running) {
            esp_timer_stop(s_tuner_watch_timer);
            s_tuner_watch_running = false;
        }
        return;
    }

    if (!s_amp.valid) {
        return;
    }

    if (!s_tuner_watch_running) {
        esp_timer_start_periodic(s_tuner_watch_timer, TUNER_WATCH_POLL_US);
        s_tuner_watch_running = true;
        if (s_mode == NANO_RX_IDLE) {
            request_current_state_now("Tuner watch");
        } else {
            s_pending_tuner_sync = true;
        }
    }
}
