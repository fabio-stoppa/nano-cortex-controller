#include "nano_state.h"

#include <math.h>
#include <stdlib.h>
#include <string.h>

#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#include "nano_ble.h"
#include "nano_amp.h"
#include "nano_fx.h"
#include "nano_protocol.h"
#include "nano_proto.h"
#include "nano_tuner.h"
#include "nano_ui.h"

static const char *TAG = "nano_state";

#define STATE_BUF_MAX (48 * 1024)
#define PARSE_DEBOUNCE_US (1200 * 1000)
#define STATE_REQUEST_DELAY_US (2200 * 1000)
#define REFRESH_DEBOUNCE_US (800 * 1000)
#define REFRESH_MIN_INTERVAL_US (1500 * 1000)
#define LIVE_POLL_INTERVAL_US (15000 * 1000)
#define SNIFF_MIN_INTERVAL_US (100 * 1000)
#define TUNER_FAST_SYNC_US (400 * 1000)
#define TUNER_WATCH_POLL_US (5000 * 1000)
#define STATE_RX_TIMEOUT_US (5000 * 1000)
/** Max times we defer finish while waiting for a gate tail after mid-f34 truncation. */
#define STATE_INCOMPLETE_DEFER_MAX 2
/** Re-request current state when INITIAL/ROUTINE dump still lacks gate fields. */
#define STATE_GATE_RETRY_MAX 2
#define STATE_PROBE_LABEL_MAX 24
#define PARSE_TASK_STACK 8192
#define TAP_TIMEOUT_US (2000 * 1000)
#define TAP_HISTORY_MAX 4
#define TEMPO_DEFAULT_BPM 120.f
#define TEMPO_EXIT_ECHO_IGNORE_US (800 * 1000)

static nano_rx_mode_t s_mode = NANO_RX_IDLE;
static nano_amp_t s_amp;
static uint8_t s_cap_vol_kept = 128;
static bool s_cap_vol_kept_valid;
/** Pedal LEVEL knob is shared with capture-volume mode — resolve via dump. */
static int s_level_pending_raw = -1;
static uint8_t s_level_pending_prev_cap;
static uint8_t s_level_pending_prev_master;
static bool s_level_pending_active;
static int64_t s_master_write_us;
static int s_master_write_raw = -1;
/** Knobs / capture-vol updated live while a dump is in flight — keep over stale parse. */
static uint8_t s_amp_live_dirty;
static bool s_cap_vol_live_dirty;
static uint8_t *s_state_buf;
static uint8_t *s_fe_buf;
static size_t s_state_len;
static size_t s_fe_len;
static size_t s_last_bpm_hunt_len;
static nano_preset_catalog_t s_preset_catalog;
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
static bool s_bpm_known;
static uint16_t s_last_bpm;
static bool s_tempo_mode_active;
static int64_t s_tempo_ignore_exit_echo_us;
static char s_state_probe_label[STATE_PROBE_LABEL_MAX];
static char s_incomplete_probe_label[STATE_PROBE_LABEL_MAX];
static unsigned s_incomplete_defer_count;
static unsigned s_gate_retry_count;
static TaskHandle_t s_parse_task;
static int64_t s_tap_times[TAP_HISTORY_MAX];
static unsigned s_tap_count;

static void parse_state_buffer(void);
static void resolve_pending_level_from_dump(void);
static void on_tempo_notify(uint16_t bpm, bool tap_marker);
static void schedule_parse(void);

void nano_state_on_bpm_found(uint16_t bpm)
{
    if (bpm < NANO_BPM_MIN || bpm > NANO_BPM_MAX) {
        return;
    }
    s_bpm_known = true;
    s_last_bpm = bpm;
    nano_ui_set_bpm(bpm, true);
}

uint16_t nano_state_get_bpm(void)
{
    return s_bpm_known ? s_last_bpm : 0;
}

bool nano_state_tempo_active(void)
{
    return s_tempo_mode_active;
}

static float tempo_bpm_f(void)
{
    if (s_bpm_known && s_last_bpm >= NANO_BPM_MIN && s_last_bpm <= NANO_BPM_MAX) {
        return (float)s_last_bpm;
    }
    return TEMPO_DEFAULT_BPM;
}

static bool write_tempo_tap_bpm(float bpm_f, const char *label)
{
    uint8_t buf[24];
    size_t n = nano_bpm_tap_payload(bpm_f, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }
    return nano_ble_write_c304(label, buf, n) == ESP_OK;
}

static void on_tempo_notify(uint16_t bpm, bool tap_marker)
{
    if (bpm < NANO_BPM_MIN || bpm > NANO_BPM_MAX) {
        return;
    }

    s_bpm_known = true;
    s_last_bpm = bpm;

    if (tap_marker) {
        /* Tap / keep-alive while in Tempo Mode — open UI + phase-lock blinker. */
        ESP_LOGI(TAG, "Tempo Mode notify TAP @ %u BPM", (unsigned)bpm);
        s_tempo_mode_active = true;
        nano_ui_set_tempo_mode(true);
        nano_ui_bpm_tap(bpm);
        return;
    }

    int64_t now = esp_timer_get_time();
    if (now < s_tempo_ignore_exit_echo_us) {
        ESP_LOGI(TAG, "Tempo Mode notify EXIT @ %u BPM (ignored echo)", (unsigned)bpm);
        nano_ui_set_bpm(bpm, true);
        return;
    }

    /*
     * Exit-shaped 0x91 (no 18 01):
     * - While active → leave Tempo Mode (Footswitch II / peer exit).
     * - While idle → enter announce (Footswitch I hold); Cloud app opens here too.
     */
    if (s_tempo_mode_active) {
        ESP_LOGI(TAG, "Tempo Mode notify EXIT @ %u BPM", (unsigned)bpm);
        s_tempo_mode_active = false;
        s_tap_count = 0;
        nano_ui_set_bpm(bpm, true);
        nano_ui_set_tempo_mode(false);
    } else {
        ESP_LOGI(TAG, "Tempo Mode notify ENTER @ %u BPM", (unsigned)bpm);
        s_tempo_mode_active = true;
        nano_ui_set_tempo_mode(true);
        nano_ui_bpm_tap(bpm);
    }
}

bool nano_state_tempo_enter(void)
{
    if (!nano_ble_connected()) {
        return false;
    }
    s_tap_count = 0;
    float bpm_f = tempo_bpm_f();
    ESP_LOGI(TAG, "Tempo Mode enter @ %.0f BPM", (double)bpm_f);
    bool ok = write_tempo_tap_bpm(bpm_f, "Tempo Mode enter");
    if (ok) {
        uint16_t bpm = (uint16_t)lroundf(bpm_f);
        s_bpm_known = true;
        s_last_bpm = bpm;
        s_tempo_mode_active = true;
        /* Number only — blinker phase-locks on the pedal echo/notify. */
        nano_ui_set_bpm(bpm, true);
    }
    return ok;
}

bool nano_state_tempo_exit(void)
{
    s_tap_count = 0;
    s_tempo_mode_active = false;
    s_tempo_ignore_exit_echo_us = esp_timer_get_time() + TEMPO_EXIT_ECHO_IGNORE_US;
    if (!nano_ble_connected()) {
        return false;
    }
    float bpm_f = tempo_bpm_f();
    uint8_t buf[24];
    size_t n = nano_bpm_exit_payload(bpm_f, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }
    ESP_LOGI(TAG, "Tempo Mode exit @ %.0f BPM", (double)bpm_f);
    return nano_ble_write_c304("Tempo Mode exit", buf, n) == ESP_OK;
}

bool nano_state_tempo_tap(void)
{
    if (!nano_ble_connected()) {
        return false;
    }

    int64_t now = esp_timer_get_time();
    if (s_tap_count > 0 && (now - s_tap_times[s_tap_count - 1]) > TAP_TIMEOUT_US) {
        s_tap_count = 0;
    }

    if (s_tap_count >= TAP_HISTORY_MAX) {
        memmove(s_tap_times, s_tap_times + 1, (TAP_HISTORY_MAX - 1) * sizeof(s_tap_times[0]));
        s_tap_count = TAP_HISTORY_MAX - 1;
    }
    s_tap_times[s_tap_count++] = now;

    float bpm_f = tempo_bpm_f();
    if (s_tap_count >= 2) {
        double sum_us = 0.0;
        for (unsigned i = 1; i < s_tap_count; i++) {
            sum_us += (double)(s_tap_times[i] - s_tap_times[i - 1]);
        }
        double avg_us = sum_us / (double)(s_tap_count - 1);
        if (avg_us >= 1.0) {
            bpm_f = (float)(60000000.0 / avg_us);
            if (bpm_f < (float)NANO_BPM_MIN) {
                bpm_f = (float)NANO_BPM_MIN;
            } else if (bpm_f > (float)NANO_BPM_MAX) {
                bpm_f = (float)NANO_BPM_MAX;
            }
        }
    }

    uint16_t bpm = (uint16_t)lroundf(bpm_f);
    ESP_LOGI(TAG, "Tempo Mode tap #%u -> %u BPM", s_tap_count, (unsigned)bpm);
    if (!write_tempo_tap_bpm(bpm_f, "Tempo Mode tap")) {
        return false;
    }

    s_bpm_known = true;
    s_last_bpm = bpm;
    s_tempo_mode_active = true;
    /* Update readout now; phase-lock on C305 echo so we match the Nano LED. */
    nano_ui_set_bpm(bpm, true);
    return true;
}

static void append_fe_payload(const uint8_t *data, size_t len)
{
    if (len <= 2 || !s_fe_buf) {
        return;
    }
    const uint8_t *payload = data + 2;
    size_t plen = len - 2;
    if (s_fe_len + plen > STATE_BUF_MAX) {
        ESP_LOGW(TAG, "FE metadata buffer full, dropping %u bytes", (unsigned)plen);
        return;
    }
    memcpy(s_fe_buf + s_fe_len, payload, plen);
    s_fe_len += plen;
}

static void finish_fe_capture(void)
{
    if (s_fe_len > 0) {
        if (nano_parse_fe_preset_catalog(s_fe_buf, s_fe_len, &s_preset_catalog)) {
            ESP_LOGI(TAG, "Preset catalog loaded (%u names)", (unsigned)s_preset_catalog.count);
        } else {
            ESP_LOGW(TAG, "FE metadata parse found no preset names (%u bytes)", (unsigned)s_fe_len);
        }

        uint16_t fe_bpm = 0;
        if (nano_parse_bpm(s_fe_buf, s_fe_len, &fe_bpm)) {
            ESP_LOGI(TAG, "BPM %u (field 56 in FE metadata)", (unsigned)fe_bpm);
            nano_state_on_bpm_found(fe_bpm);
        } else {
            nano_log_bpm_raw_scan(TAG, "FE metadata", s_fe_buf, s_fe_len);
        }
    }
    s_fe_len = 0;
}

static void update_preset_label_locked(const uint8_t *state, size_t state_len)
{
    char label[NANO_PRESET_NAME_MAX] = "-";
    char direct[NANO_PRESET_NAME_MAX];

    if (nano_proto_first_string(state, state_len, 2, direct, sizeof(direct)) && direct[0]) {
        strncpy(label, direct, sizeof(label) - 1);
        label[sizeof(label) - 1] = '\0';
    } else {
        uint32_t slot = 0;
        if (nano_proto_field_varint(state, state_len, 1, &slot)) {
            uint32_t idx = 0;
            if (slot >= 1 && slot <= NANO_PRESET_COUNT) {
                idx = slot;
            } else if (slot < NANO_PRESET_COUNT) {
                idx = slot + 1;
            }
            if (idx > 0) {
                if (s_preset_catalog.valid && s_preset_catalog.names[idx - 1][0]) {
                    strncpy(label, s_preset_catalog.names[idx - 1], sizeof(label) - 1);
                    label[sizeof(label) - 1] = '\0';
                } else {
                    snprintf(label, sizeof(label), "Preset %u", (unsigned)idx);
                }
            }
        }
    }

    nano_ui_set_preset_name(label);
}

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
    ESP_LOGI(TAG, "State chunk +%u → %u bytes", (unsigned)plen, (unsigned)s_state_len);
}

static void finish_state_capture(void)
{
    s_state_len = 0;
    s_mode = NANO_RX_IDLE;
    s_state_rx_since_us = 0;
    s_incomplete_defer_count = 0;
    s_incomplete_probe_label[0] = '\0';
}

/**
 * True when a length-delimited field claims more bytes than remain — typical
 * mid-f34 truncation that leaves gate f53/f54 in a later MTU tail.
 */
static bool state_dump_truncated(const uint8_t *data, size_t len)
{
    if (!data || len < 3) {
        return false;
    }

    size_t i = 0;
    while (i < len) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            return i > 0;
        }
        i = next;
        uint32_t wire = tag & 0x07;
        if (wire == 0) {
            if (!nano_read_varint(data, len, i, NULL, &next)) {
                return true;
            }
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                return true;
            }
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                return true;
            }
            i = next;
            if (i + (size_t)flen > len) {
                return true;
            }
            i += (size_t)flen;
        } else if (wire == 5) {
            if (i + 4 > len) {
                return true;
            }
            i += 4;
        } else {
            return true;
        }
    }
    return false;
}

static void abort_stale_state_rx(const char *why)
{
    if (s_mode != NANO_RX_CURRENT) {
        return;
    }
    ESP_LOGW(TAG, "State RX aborted (%s, had %u bytes)", why, (unsigned)s_state_len);
    /* One last parse so a delayed gate tail still applies before we drop RX. */
    if (s_state_len > 0) {
        if (s_incomplete_probe_label[0]) {
            strncpy(s_state_probe_label, s_incomplete_probe_label, sizeof(s_state_probe_label) - 1);
            s_state_probe_label[sizeof(s_state_probe_label) - 1] = '\0';
        }
        s_incomplete_defer_count = STATE_INCOMPLETE_DEFER_MAX;
        parse_state_buffer();
        return;
    }
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
    /* Preset FE streams stomp the live pitch mailbox — never poll while tuning. */
    const nano_tuner_t *tuner = nano_tuner_get();
    if (!from_initial_sync && tuner && tuner->active) {
        if (label && strstr(label, "Tuner") != NULL) {
            s_pending_tuner_sync = true;
        }
        return;
    }

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

    char label[STATE_PROBE_LABEL_MAX];
    if (s_state_probe_label[0]) {
        strncpy(label, s_state_probe_label, sizeof(label) - 1);
        label[sizeof(label) - 1] = '\0';
    } else if (s_incomplete_probe_label[0]) {
        strncpy(label, s_incomplete_probe_label, sizeof(label) - 1);
        label[sizeof(label) - 1] = '\0';
    } else {
        strncpy(label, "ROUTINE", sizeof(label) - 1);
        label[sizeof(label) - 1] = '\0';
    }
    s_state_probe_label[0] = '\0';

    size_t buf_len = s_state_len;
    ESP_LOGI(TAG, "Parsing preset state (%u bytes) [%s]", (unsigned)buf_len, label);
    if (strcmp(label, "INITIAL") == 0) {
        nano_log_proto_fields(TAG, label, s_state_buf, buf_len);
    }

    bool truncated = state_dump_truncated(s_state_buf, buf_len);
    bool has_gate = nano_fx_dump_has_gate_fields(s_state_buf, buf_len);
    bool wait_for_gate = truncated && !has_gate && s_mode == NANO_RX_CURRENT &&
                         s_incomplete_defer_count < STATE_INCOMPLETE_DEFER_MAX;

    nano_amp_t live_amp = s_amp;
    uint8_t live_dirty = s_amp_live_dirty;
    bool cap_dirty = s_cap_vol_live_dirty;
    if (nano_parse_amp(s_state_buf, buf_len, &s_amp)) {
        /*
         * Field 44 often sits in the missing mid-dump gap. Keep a previously
         * known capture volume (notify / write / earlier full dump); only seed
         * the web-editor 0 dB default (raw 128) when still unknown.
         */
        if (s_amp.have_capture_volume) {
            s_cap_vol_kept = s_amp.capture_volume;
            s_cap_vol_kept_valid = true;
        } else if (s_cap_vol_kept_valid) {
            s_amp.capture_volume = s_cap_vol_kept;
            s_amp.have_capture_volume = true;
        } else {
            s_amp.capture_volume = 128;
            s_amp.have_capture_volume = true;
            s_cap_vol_kept = 128;
            s_cap_vol_kept_valid = true;
        }
        /* Dump snapshot can be older than knobs turned during RX — keep live. */
        if (live_amp.valid && live_dirty) {
            uint8_t *dst[] = {
                &s_amp.gain, &s_amp.level, &s_amp.bass, &s_amp.mid, &s_amp.treble,
            };
            const uint8_t *src[] = {
                &live_amp.gain, &live_amp.level, &live_amp.bass, &live_amp.mid, &live_amp.treble,
            };
            for (int i = 0; i < 5; i++) {
                if (live_dirty & (1u << i)) {
                    *dst[i] = *src[i];
                }
            }
        }
        if (cap_dirty && live_amp.valid && live_amp.have_capture_volume) {
            s_amp.capture_volume = live_amp.capture_volume;
            s_amp.have_capture_volume = true;
            s_cap_vol_kept = live_amp.capture_volume;
            s_cap_vol_kept_valid = true;
        }
        s_amp_live_dirty = 0;
        s_cap_vol_live_dirty = false;
        resolve_pending_level_from_dump();
        nano_log_amp(TAG, &s_amp);
        nano_ui_set_amp(&s_amp);
        if (!wait_for_gate) {
            start_live_poll();
        }
    } else {
        ESP_LOGW(TAG, "Could not find amp fields 3–7 in state blob");
    }

    if (wait_for_gate) {
        s_incomplete_defer_count++;
        strncpy(s_incomplete_probe_label, label, sizeof(s_incomplete_probe_label) - 1);
        s_incomplete_probe_label[sizeof(s_incomplete_probe_label) - 1] = '\0';
        ESP_LOGI(TAG,
                 "Incomplete state dump (%u bytes, no gate tail) — waiting (%u/%u)",
                 (unsigned)buf_len, s_incomplete_defer_count, STATE_INCOMPLETE_DEFER_MAX);
        /* Apply whatever FX/gate we already have; keep RX open for late 0x80 tails. */
        nano_fx_sync_from_preset(s_state_buf, buf_len);
        schedule_parse();
        return;
    }

    finish_state_capture();

    bool tuner_active = false;
    nano_tuner_route_t tuner_route = NANO_TUNER_ROUTE_UNKNOWN;
    if (nano_parse_tuner_state(s_state_buf, buf_len, &tuner_active, &tuner_route)) {
        if (strcmp(label, "ROUTE") == 0 || strcmp(label, "TUNER") == 0 || tuner_active ||
            nano_tuner_get()->active) {
            nano_log_tuner_state_fields(TAG, s_state_buf, buf_len);
        }
        nano_tuner_sync_from_state(tuner_active, tuner_route);
    } else {
        if (strcmp(label, "ROUTE") == 0 || strcmp(label, "TUNER") == 0 || nano_tuner_get()->active) {
            nano_log_tuner_state_fields(TAG, s_state_buf, buf_len);
        }
        nano_state_tuner_watch_update(nano_tuner_get()->active);
    }

    nano_fx_sync_from_preset(s_state_buf, buf_len);

    update_preset_label_locked(s_state_buf, buf_len);

    uint16_t bpm = 0;
    if (nano_parse_bpm(s_state_buf, buf_len, &bpm)) {
        ESP_LOGI(TAG, "BPM %u (field 56) [%s]", (unsigned)bpm, label);
        nano_state_on_bpm_found(bpm);
    } else {
        /* Short dumps omit f56 — keep last live tap-tempo BPM instead of clearing. */
        ESP_LOGI(TAG, "BPM field 56 absent len=%u [%s]", (unsigned)buf_len, label);
        if (strcmp(label, "INITIAL") == 0 || buf_len != s_last_bpm_hunt_len) {
            nano_log_bpm_hunt(TAG, s_state_buf, buf_len);
            s_last_bpm_hunt_len = buf_len;
        }
        if (strcmp(label, "INITIAL") == 0 && !s_bpm_known) {
            ESP_LOGW(TAG, "BPM still unknown after INITIAL (FE nested f56 should supply it)");
        }
    }

    if (!nano_fx_dump_has_gate_fields(s_state_buf, buf_len) &&
        s_gate_retry_count < STATE_GATE_RETRY_MAX &&
        (strcmp(label, "INITIAL") == 0 || strcmp(label, "ROUTINE") == 0 ||
         strcmp(label, "PRESET") == 0)) {
        s_gate_retry_count++;
        ESP_LOGI(TAG, "Gate fields absent after parse — retry %u/%u", s_gate_retry_count,
                 STATE_GATE_RETRY_MAX);
        request_current_state_now("Gate sync retry");
    } else if (nano_fx_dump_has_gate_fields(s_state_buf, buf_len)) {
        s_gate_retry_count = 0;
    }

    maybe_retry_tuner_sync();
}

static void parse_worker(void *arg)
{
    (void)arg;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, portMAX_DELAY);
        parse_state_buffer();
    }
}

static void ensure_parse_task(void)
{
    if (s_parse_task) {
        return;
    }
    BaseType_t ok = xTaskCreate(parse_worker, "nano_parse", PARSE_TASK_STACK, NULL, 5, &s_parse_task);
    if (ok != pdPASS) {
        ESP_LOGE(TAG, "Failed to create parse worker task");
        s_parse_task = NULL;
    }
}

static void parse_timer_cb(void *arg)
{
    (void)arg;
    ensure_parse_task();
    if (s_parse_task) {
        xTaskNotifyGive(s_parse_task);
    } else {
        parse_state_buffer();
    }
}

static void refresh_timer_cb(void *arg)
{
    (void)arg;
    request_current_state_now("Debounced amp refresh");
}

static void poll_timer_cb(void *arg)
{
    (void)arg;
    const nano_tuner_t *tuner = nano_tuner_get();
    if (tuner && tuner->active) {
        return;
    }
    if (s_mode == NANO_RX_IDLE && s_amp.valid) {
        request_current_state_now("Live amp poll");
    }
}

static void tuner_fast_sync_timer_cb(void *arg)
{
    (void)arg;
    const nano_tuner_t *tuner = nano_tuner_get();
    if (tuner && tuner->active) {
        s_pending_tuner_sync = true;
        return;
    }
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
    /*
     * High-bit byte1 marks segmented FE/state continuations, but live notifies
     * use lead C0 (also high bit). Classifying C0 as FE stream drops gain/EQ/
     * master updates for the whole dump window — UI freezes until parse.
     */
    if ((mode == NANO_RX_METADATA || mode == NANO_RX_CURRENT) && (data[1] & 0x80) != 0 &&
        data[1] != 0xC0) {
        return true;
    }
    return false;
}

/** Preset state: FE 41 stream chunks, or single-packet dump with byte1 == 0xC1. */
static bool is_state_stream_chunk(const uint8_t *data, size_t len)
{
    if (len < 3) {
        return false;
    }
    if (data[0] == 0xFE && data[1] == 0x41) {
        return true;
    }
    /* Desktop decoder: full current-state dump notifications use second byte 0xC1. */
    if (data[1] == 0xC1) {
        return true;
    }
    return false;
}

static void sniff_unknown_c305(const uint8_t *data, size_t len)
{
    int64_t now = esp_timer_get_time();
    if (now - s_last_sniff_us < SNIFF_MIN_INTERVAL_US) {
        return;
    }
    s_last_sniff_us = now;
    ESP_LOGI(TAG, "BPM hunt: unknown C305 len=%u lead=%02X %02X", (unsigned)len,
             len > 0 ? data[0] : 0, len > 1 ? data[1] : 0);
    nano_log_bytes_full(TAG, "BPM hunt C305", data, len > 64 ? 64 : len);
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
    if (s_mode == NANO_RX_CURRENT) {
        s_amp_live_dirty |= (uint8_t)(1u << knob_id);
    }
    ESP_LOGD(TAG, "Amp %s -> raw %u (%s)", names[knob_id], raw, source);
    nano_ui_set_amp(&s_amp);
    return true;
}

/**
 * Pedal LEVEL notifies: always C0 08 01 18 01 20 <v> 30 01 …
 * Same frame for normal LEVEL and capture-volume mode — no distinct BLE id.
 * Map LEVEL notifies to MASTER only. CAPTURE LV is driven by id 0x0A / field 44.
 */
static void on_shared_level_notify(uint8_t raw)
{
    int64_t now = esp_timer_get_time();
    if (s_master_write_raw >= 0 && (now - s_master_write_us) < (500 * 1000) &&
        (int)raw == s_master_write_raw) {
        apply_knob_update((uint8_t)NANO_KNOB_LEVEL, raw, "master echo");
        s_master_write_raw = -1;
        return;
    }

    apply_knob_update((uint8_t)NANO_KNOB_LEVEL, raw, "hw level");
}

static void resolve_pending_level_from_dump(void)
{
    /* No-op: LEVEL notifies are master-only; capture volume uses id 0x0A. */
    s_level_pending_active = false;
    s_level_pending_raw = -1;
}

static bool try_inline_amp_update(const uint8_t *data, size_t len)
{
    uint8_t knob_id = 0;
    uint8_t raw = 0;

    if (nano_try_parse_capture_volume_notify(data, len, &raw)) {
        if (s_amp.valid && s_amp.have_capture_volume && s_amp.capture_volume == raw) {
            return true;
        }
        s_amp.capture_volume = raw;
        s_amp.have_capture_volume = true;
        s_amp.valid = true;
        s_cap_vol_kept = raw;
        s_cap_vol_kept_valid = true;
        if (s_mode == NANO_RX_CURRENT) {
            s_cap_vol_live_dirty = true;
        }
        ESP_LOGD(TAG, "Capture volume -> raw %u (notify)", raw);
        nano_ui_set_amp(&s_amp);
        return true;
    }

    if (nano_try_parse_amp_hw_notify(data, len, &knob_id, &raw)) {
        if (knob_id == (uint8_t)NANO_KNOB_LEVEL) {
            on_shared_level_notify(raw);
            if (s_refresh_timer) {
                esp_timer_stop(s_refresh_timer);
            }
            return true;
        }
        if (apply_knob_update(knob_id, raw, "hw knob")) {
            if (s_refresh_timer) {
                esp_timer_stop(s_refresh_timer);
            }
        }
        return true;
    }

    if (nano_try_parse_amp_knob_notify(data, len, &knob_id, &raw)) {
        if (knob_id == (uint8_t)NANO_KNOB_LEVEL) {
            on_shared_level_notify(raw);
            return true;
        }
        return apply_knob_update(knob_id, raw, "knob notify");
    }

    return false;
}

bool nano_state_try_amp_notify_fast(const uint8_t *data, size_t len)
{
    return try_inline_amp_update(data, len);
}

static void sync_timer_cb(void *arg)
{
    (void)arg;
    finish_fe_capture();
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
    s_incomplete_probe_label[0] = '\0';
    s_incomplete_defer_count = 0;
    s_gate_retry_count = 0;
    s_tuner_watch_running = false;
    s_pending_tuner_sync = false;
    s_state_rx_since_us = 0;
    s_bpm_known = false;
    s_last_bpm = 0;
    s_tap_count = 0;
    s_tempo_mode_active = false;
    s_tempo_ignore_exit_echo_us = 0;
    memset(&s_amp, 0, sizeof(s_amp));
    s_amp_live_dirty = 0;
    s_cap_vol_live_dirty = false;
    s_cap_vol_kept = 128;
    s_cap_vol_kept_valid = false;
    s_master_write_raw = -1;
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
    nano_fx_reset();
    s_tempo_mode_active = false;
    s_tempo_ignore_exit_echo_us = 0;
    s_tap_count = 0;
    nano_ui_set_bpm(0, false);
    nano_ui_set_tempo_mode(false);
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

    const nano_tuner_t *tuner = nano_tuner_get();
    bool tuner_live = tuner && tuner->active;
    if (tuner_live) {
        nano_tuner_note_c305_rx();
    }

    /*
     * Pitch / tuner frames are length-prefixed: <len> C0 08 01 22 …
     * Natural mid-range 22/01 often uses len=0x10 — do NOT treat that as preset ACK.
     */
    if (len >= 5 && data[1] == 0xC0 && data[2] == 0x08 && data[3] == 0x01 && data[4] == 0x22) {
        if (nano_tuner_feed_c305(data, len)) {
            return;
        }
    }

    if (len >= 2 && data[0] == 0x10 && data[1] == 0xC0) {
        if (tuner_live) {
            ESP_LOGD(TAG, "C305 diverted (preset ACK) len=%u", (unsigned)len);
        } else {
            ESP_LOGD(TAG, "Preset change ACK");
        }
        return;
    }

    /* While tuner is live, ignore preset/amp/FX traffic — pitch must stay snappy. */
    if (is_state_stream_chunk(data, len)) {
        if (tuner_live) {
            ESP_LOGD(TAG, "C305 dropped (state stream FE 41) while tuner live len=%u", (unsigned)len);
            return;
        }
        if (s_mode == NANO_RX_METADATA) {
            ESP_LOGD(TAG, "State stream chunk before request (%u bytes)", (unsigned)len);
            return;
        }
        if (s_mode == NANO_RX_IDLE) {
            s_state_len = 0;
            s_mode = NANO_RX_CURRENT;
            s_state_rx_since_us = esp_timer_get_time();
            s_incomplete_defer_count = 0;
            s_incomplete_probe_label[0] = '\0';
        }
        if (data[1] == 0xC1) {
            ESP_LOGI(TAG, "BPM hunt: C1 state dump chunk len=%u", (unsigned)len);
        }
        append_state_payload(data, len);
        schedule_parse();
        return;
    }

    /*
     * Segmented current-state continuations (desktop decoder):
     *  - byte1 high-bit continuation frames
     *  - additional FE/FD/CE/D0 segments after the first chunk
     *
     * Live notifies use lead byte C0 (high bit set) — never treat those as
     * dump continuations or knob turns pollute the state buffer.
     */
    if (s_mode == NANO_RX_CURRENT && len >= 3 && data[1] != 0xC0 &&
        ((data[1] & 0x80) != 0 || data[0] == 0xFE || data[0] == 0xFD || data[0] == 0xCE ||
         data[0] == 0xD0)) {
        if (tuner_live) {
            ESP_LOGD(TAG, "C305 dropped (state continuation) while tuner live len=%u",
                     (unsigned)len);
            return;
        }
        append_state_payload(data, len);
        schedule_parse();
        return;
    }

    if (is_fe_stream(data, len, s_mode)) {
        if (tuner_live) {
            ESP_LOGD(TAG, "C305 dropped (FE stream) while tuner live len=%u", (unsigned)len);
            return;
        }
        if (s_mode == NANO_RX_METADATA) {
            append_fe_payload(data, len);
            return;
        }
        return;
    }

    if (tuner_live) {
        /* Amp/FX updates wait until tuner exits — do not spend cycles on them. */
        ESP_LOGD(TAG, "C305 unhandled while tuner live len=%u lead=0x%02X", (unsigned)len, data[0]);
        return;
    }

    uint16_t live_bpm = 0;
    bool tap_marker = false;
    if (nano_try_parse_bpm_notify_ex(data, len, &live_bpm, &tap_marker)) {
        on_tempo_notify(live_bpm, tap_marker);
        return;
    }

    if (try_inline_amp_update(data, len)) {
        return;
    }

    if (nano_tuner_feed_c305(data, len)) {
        return;
    }

    if (nano_fx_feed_c305(data, len)) {
        return;
    }

    /* Late dump tails (byte1 high-bit, often 0x80) carry gate f53/f54 after a
     * mid-f34 gap. When RX already closed, salvage gate directly from the payload. */
    if (len > 2 && nano_fx_try_apply_gate_bytes(data + 2, len - 2)) {
        return;
    }
    if (nano_fx_try_apply_gate_bytes(data, len)) {
        return;
    }

    if (s_mode == NANO_RX_IDLE) {
        sniff_unknown_c305(data, len);
        schedule_refresh();
    } else {
        ESP_LOGI(TAG, "BPM hunt: other C305 while mode=%d len=%u lead=%02X", (int)s_mode,
                 (unsigned)len, data[0]);
        nano_log_bytes_full(TAG, "BPM hunt other C305", data, len > 48 ? 48 : len);
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
    if (s_mode == NANO_RX_CURRENT) {
        s_amp_live_dirty |= (uint8_t)(1u << (unsigned)knob);
    }
    if (knob == NANO_KNOB_LEVEL) {
        s_master_write_us = esp_timer_get_time();
        s_master_write_raw = (int)raw;
    }

    uint8_t buf[32];
    size_t n = nano_amp_knob_payload(knob, raw, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }

    char label[32];
    snprintf(label, sizeof(label), "Amp %s = %u", names[knob], raw);
    ESP_LOGD(TAG, "Amp %s -> raw %u (touch)", names[knob], raw);
    return nano_ble_write_c304(label, buf, n) == ESP_OK;
}

bool nano_state_write_capture_volume(uint8_t raw)
{
    if (!nano_ble_connected()) {
        return false;
    }

    s_amp.capture_volume = raw;
    s_amp.have_capture_volume = true;
    s_amp.valid = true;
    s_cap_vol_kept = raw;
    s_cap_vol_kept_valid = true;
    if (s_mode == NANO_RX_CURRENT) {
        s_cap_vol_live_dirty = true;
    }

    uint8_t buf[32];
    size_t n = nano_capture_volume_payload(raw, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }

    ESP_LOGD(TAG, "Capture volume -> raw %u (touch)", raw);
    nano_ui_set_amp(&s_amp);
    return nano_ble_write_c304("Capture volume", buf, n) == ESP_OK;
}

bool nano_state_write_capture_slot(uint8_t slot)
{
    if (slot < 1 || slot > 25 || !nano_ble_connected()) {
        return false;
    }

    s_amp.capture_slot = slot;
    s_amp.have_capture_slot = true;
    const nano_amp_capture_t *capture = nano_amp_capture_by_slot(slot);
    if (capture && capture->name) {
        strncpy(s_amp.capture_name, capture->name, NANO_AMP_CAPTURE_NAME_MAX - 1);
        s_amp.capture_name[NANO_AMP_CAPTURE_NAME_MAX - 1] = '\0';
        s_amp.have_capture_name = true;
    }
    s_amp.valid = true;
    nano_ui_set_amp(&s_amp);

    uint8_t buf[16];
    size_t n = nano_capture_select_payload(slot, buf, sizeof(buf));
    if (n == 0) {
        return false;
    }

    char label[48];
    snprintf(label, sizeof(label), "Capture slot %u", (unsigned)slot);
    ESP_LOGI(TAG, "%s", label);
    if (nano_ble_write_c304(label, buf, n) != ESP_OK) {
        return false;
    }

    nano_state_request_preset_refresh("Capture select");
    return true;
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
    if (!s_fe_buf) {
        s_fe_buf = malloc(STATE_BUF_MAX);
        if (!s_fe_buf) {
            ESP_LOGE(TAG, "Failed to allocate FE metadata buffer");
            return;
        }
    }
    s_fe_len = 0;
    memset(&s_preset_catalog, 0, sizeof(s_preset_catalog));

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
        if (s_tuner_fast_sync_timer) {
            esp_timer_stop(s_tuner_fast_sync_timer);
        }
        if (s_refresh_timer) {
            esp_timer_stop(s_refresh_timer);
        }
        /* Abort an in-flight FE/state capture so leftover chunks can't block pitch. */
        if (s_mode == NANO_RX_CURRENT) {
            ESP_LOGI(TAG, "Aborting preset state RX — tuner live");
            s_mode = NANO_RX_IDLE;
            s_state_len = 0;
            s_state_rx_since_us = 0;
            s_state_probe_label[0] = '\0';
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

void nano_state_request_preset_refresh(const char *label)
{
    ensure_timers();
    if (s_mode == NANO_RX_IDLE && s_amp.valid) {
        request_current_state_now(label ? label : "Preset refresh");
    }
}
