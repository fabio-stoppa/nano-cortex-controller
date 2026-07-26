#include "nano_tuner.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_timer.h"

#include "nano_ble.h"
#include "nano_protocol.h"
#include "nano_proto.h"
#include "nano_state.h"
#include "nano_ui.h"

static const char *TAG = "nano_tuner";

#define PITCH_MAGIC0 0x23
#define PITCH_MAGIC1 0x2D
#define RX_IGNORE_ACTIVE_US (400 * 1000)
#define PITCH_STALE_US (500 * 1000)
#define PITCH_CENTS_MAX 1200.f
#define PITCH_DISPLAY_EPS 0.8f
#define PITCH_LOG_US (500 * 1000)
#define PITCH_REJECT_LOG_US (300 * 1000)
#define PITCH_GAP_LOG_US (2000 * 1000)
/** After this quiet gap, next packet is a fresh pluck — lock immediately. */
#define PITCH_RELOCK_US (500 * 1000)
#define TUNER_RAW_DEDUP_US (40 * 1000)
#define C305_TUNER_AUTHORITY_US (15000 * 1000)
#define TUNER_RAW_FP_MAX 64

static nano_tuner_t s_tuner;
static volatile uint32_t s_tuner_seq;
static int64_t s_last_pitch_log_us;
static int64_t s_last_pitch_reject_us;
static int64_t s_last_raw_sniff_us;
static int64_t s_last_gap_log_us;
static uint8_t s_last_raw_fp[TUNER_RAW_FP_MAX];
static size_t s_last_raw_fp_len;
static int8_t s_last_logged_note = -1;
static int16_t s_last_logged_cents;
static int64_t s_last_pitch_us;
static int64_t s_last_pitch_packet_us;
static int64_t s_prev_pitch_packet_us;
static float s_last_abs_pitch;
static float s_display_cents_f;
static bool s_have_abs_pitch;
static int64_t s_ignore_active_until_us;
static esp_timer_handle_t s_tick_timer;
static uint8_t s_last_raw_tag;
static int8_t s_last_pitch_kind;
static float s_last_raw_cents;
static int64_t s_c305_authority_until_us;
static bool s_c305_authority_active;
#define GAIN_2D_FP_MAX 24
#define GAIN_2D_RETX_US (400 * 1000)
static uint8_t s_last_gain_fp[GAIN_2D_FP_MAX];
static size_t s_last_gain_fp_len;
static bool s_mode_shape_valid;
static bool s_mode_has_38_01;
static bool s_allow_same_shape_mode;
static int64_t s_last_route_apply_us;

static const char *NOTE_NAMES[] = {
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
};

static void push_ui(void);
static void expire_stale_pitch(void);
static void clear_pitch_reading(void);
static void ensure_tick_timer(void);
static void apply_tuner_mode_c305(bool active, nano_tuner_route_t route);
static bool direct_2d_header_before(const uint8_t *data, size_t len, size_t i_2d);
static bool gain_prefix_before_2d(const uint8_t *data, size_t len, size_t i_2d);

static void note_c305_authority(bool active);

static void tuner_publish(void)
{
    __atomic_add_fetch(&s_tuner_seq, 1u, __ATOMIC_RELEASE);
}

static void mark_tuner_live_from_c305(void)
{
    note_c305_authority(true);
    if (s_tuner.active) {
        return;
    }

    s_tuner.active = true;
    nano_ble_flush_c305_backlog();
    tuner_publish();
    push_ui();
    nano_state_tuner_watch_update(true);
    ESP_LOGI(TAG, "live-path 2026-07-26h armed (age meter, no C306, LVGL low)");
}

/** Wire enum is inverted vs pedal on this unit; labels show pedal truth. */
static const char *route_label(nano_tuner_route_t route)
{
    switch (route) {
    case NANO_TUNER_ROUTE_MUTE:
        return "bypass";
    case NANO_TUNER_ROUTE_BYPASS:
        return "mute";
    default:
        return "?";
    }
}

static const char *route_name(nano_tuner_route_t route)
{
    return route_label(route);
}

const char *nano_tuner_route_label(nano_tuner_route_t route)
{
    return route_label(route);
}

/**
 * Mode button alternates trailer shape (short 7F vs 38 01 7F); same shape repeat
 * while live = heartbeat. Footswitch ON announces route via gain-prefixed 2D.
 * Shape change while live toggles mute ↔ bypass.
 */
static void clear_gain_2d_route_state(void)
{
    s_last_gain_fp_len = 0;
    s_mode_shape_valid = false;
    s_allow_same_shape_mode = false;
    s_last_route_apply_us = 0;
}

static bool gain_2d_packet_is_duplicate(const uint8_t *data, size_t len)
{
    if (!data || s_last_gain_fp_len == 0 || len == 0) {
        return false;
    }

    size_t n = len < GAIN_2D_FP_MAX ? len : GAIN_2D_FP_MAX;
    return n == s_last_gain_fp_len && memcmp(data, s_last_gain_fp, n) == 0;
}

static void store_gain_2d_fingerprint(const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        return;
    }

    s_last_gain_fp_len = len < GAIN_2D_FP_MAX ? len : GAIN_2D_FP_MAX;
    memcpy(s_last_gain_fp, data, s_last_gain_fp_len);
}

static bool gain_2d_is_rapid_retransmit(const uint8_t *data, size_t len)
{
    if (!gain_2d_packet_is_duplicate(data, len)) {
        return false;
    }

    return esp_timer_get_time() - s_last_route_apply_us < GAIN_2D_RETX_US;
}

static nano_tuner_route_t route_from_trailer(bool has_38_01)
{
    return has_38_01 ? NANO_TUNER_ROUTE_BYPASS : NANO_TUNER_ROUTE_MUTE;
}

/**
 * Return true when a live gain-2D notify is a keepalive, not a mode/footswitch event.
 */
static bool gain_2d_is_heartbeat(bool has_38_01, bool was_active)
{
    if (!was_active || !s_mode_shape_valid) {
        return false;
    }

    if (has_38_01 != s_mode_has_38_01) {
        return false;
    }

    if (s_allow_same_shape_mode) {
        return false;
    }

    return route_from_trailer(has_38_01) == s_tuner.route;
}

static nano_tuner_route_t route_toggle(nano_tuner_route_t current)
{
    if (current == NANO_TUNER_ROUTE_MUTE) {
        return NANO_TUNER_ROUTE_BYPASS;
    }
    if (current == NANO_TUNER_ROUTE_BYPASS) {
        return NANO_TUNER_ROUTE_MUTE;
    }
    return NANO_TUNER_ROUTE_UNKNOWN;
}

static nano_tuner_route_t resolve_gain_2d_route(bool has_38_01, bool was_active, bool same_shape_after_on)
{
    if (!was_active || s_tuner.route == NANO_TUNER_ROUTE_UNKNOWN) {
        return route_from_trailer(has_38_01);
    }

    if (same_shape_after_on) {
        return route_toggle(s_tuner.route);
    }

    return route_from_trailer(has_38_01);
}

static const char *trailer_shape_name(bool has_38_01)
{
    return has_38_01 ? "long" : "short";
}

static bool is_direct_2d_off(const uint8_t *data, size_t len, size_t i_2d)
{
    if (!direct_2d_header_before(data, len, i_2d) || gain_prefix_before_2d(data, len, i_2d)) {
        return false;
    }

    if (i_2d + 8 <= len && data[i_2d + 5] == 0x38 && data[i_2d + 6] == 0x01 && data[i_2d + 7] == 0x7F) {
        return true;
    }

    return i_2d + 6 <= len && data[i_2d + 5] == 0x7F;
}

static bool is_gain_2d_route(const uint8_t *data, size_t len, size_t i_2d, bool *has_38_01_out)
{
    if (!has_38_01_out || !gain_prefix_before_2d(data, len, i_2d)) {
        return false;
    }

    if (i_2d + 8 <= len && data[i_2d + 5] == 0x38 && data[i_2d + 6] == 0x01) {
        *has_38_01_out = true;
        return true;
    }

    if (i_2d + 6 <= len && data[i_2d + 5] == 0x7F) {
        *has_38_01_out = false;
        return true;
    }

    return false;
}

static void note_c305_authority(bool active)
{
    s_c305_authority_active = active;
    s_c305_authority_until_us = esp_timer_get_time() + C305_TUNER_AUTHORITY_US;
}

bool nano_tuner_may_apply_state(bool state_active)
{
    if (esp_timer_get_time() >= s_c305_authority_until_us) {
        return true;
    }
    return state_active == s_c305_authority_active;
}

static void apply_tuner_mode_c305(bool active, nano_tuner_route_t route)
{
    ensure_tick_timer();
    note_c305_authority(active);

    bool changed = s_tuner.active != active;
    if (active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route != route) {
        changed = true;
    }

    if (!changed) {
        if (active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route != route) {
            s_tuner.route = route;
            ESP_LOGI(TAG, "Tuner route -> %s (C305)", route_name(route));
            push_ui();
        } else if (active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route == NANO_TUNER_ROUTE_UNKNOWN) {
            s_tuner.route = route;
            ESP_LOGI(TAG, "Tuner route -> %s (C305)", route_name(route));
            push_ui();
        }
        if (active) {
            nano_state_tuner_watch_update(true);
        }
        return;
    }

    nano_tuner_route_t prev_route = s_tuner.route;
    bool prev_active = s_tuner.active;

    s_tuner.active = active;
    if (!active) {
        s_tuner.has_pitch = false;
        s_tuner.note = -1;
        s_tuner.cents = 0;
        s_tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
        clear_gain_2d_route_state();
        s_last_raw_tag = 0;
        s_last_pitch_kind = 0;
        s_last_raw_cents = 0.f;
    } else if (route != NANO_TUNER_ROUTE_UNKNOWN) {
        s_tuner.route = route;
    }

    if (!active && prev_active) {
        ESP_LOGI(TAG, "Tuner OFF (C305)");
    } else if (active && !prev_active) {
        s_ignore_active_until_us = 0;
        nano_ble_flush_c305_backlog();
        if (s_tuner.route != NANO_TUNER_ROUTE_UNKNOWN) {
            ESP_LOGI(TAG, "Tuner ON — route: %s", route_name(s_tuner.route));
        } else {
            ESP_LOGI(TAG, "Tuner ON — route: unknown (awaiting C305 / preset state)");
        }
    } else if (active && route != NANO_TUNER_ROUTE_UNKNOWN && route != prev_route) {
        ESP_LOGI(TAG, "Tuner route -> %s (C305)", route_name(route));
    }

    tuner_publish();
    push_ui();
    nano_state_tuner_watch_update(s_tuner.active);

    if (!active) {
        nano_state_request_tuner_sync();
    }
}

static void tick_timer_cb(void *arg)
{
    (void)arg;
    expire_stale_pitch();
}

static void ensure_tick_timer(void)
{
    if (s_tick_timer) {
        return;
    }

    const esp_timer_create_args_t args = {
        .callback = tick_timer_cb,
        .name = "tuner_tick",
    };
    if (esp_timer_create(&args, &s_tick_timer) == ESP_OK) {
        esp_timer_start_periodic(s_tick_timer, 250 * 1000);
    }
}

static void expire_stale_pitch(void)
{
    if (!s_tuner.active) {
        return;
    }

    int64_t now = esp_timer_get_time();

    /* Heartbeat while live with no C305 — proves silence vs dropped logs. */
    if (s_last_raw_sniff_us > 0 && now - s_last_raw_sniff_us >= PITCH_GAP_LOG_US &&
        now - s_last_gap_log_us >= (PITCH_GAP_LOG_US * 4)) {
        s_last_gap_log_us = now;
        ESP_LOGD(TAG,
                 "C305 quiet for %d ms (tuner live%s)",
                 (int)((now - s_last_raw_sniff_us) / 1000),
                 s_tuner.has_pitch ? ", holding last pitch" : "");
    }

    if (!s_tuner.has_pitch) {
        return;
    }

    if (now - s_last_pitch_us <= PITCH_STALE_US) {
        return;
    }

    clear_pitch_reading();
    push_ui();
}

static void push_ui(void)
{
    tuner_publish();
    nano_ui_set_tuner(&s_tuner);
}

static bool rx_active_blocked(void)
{
    return esp_timer_get_time() < s_ignore_active_until_us;
}

static float read_f32_le(const uint8_t *p)
{
    uint32_t u = (uint8_t)p[0] | ((uint8_t)p[1] << 8) | ((uint8_t)p[2] << 16) | ((uint8_t)p[3] << 24);
    float f = 0.f;
    memcpy(&f, &u, sizeof(f));
    return f;
}

static bool match_header(const uint8_t *data, size_t len, size_t i, uint8_t b3, uint8_t b4)
{
    return i + 4 < len && data[i] == 0xC0 && data[i + 1] == 0x08 && data[i + 2] == 0x01 && data[i + 3] == b3 &&
           data[i + 4] == b4;
}

/** Tag byte after 22 02 / 22 01 is an ASCII note letter (A–G). */
static bool note_anchor_from_tag(uint8_t tag, int8_t *anchor)
{
    if (!anchor) {
        return false;
    }

    char c = (char)tag;
    if (c >= 'a' && c <= 'g') {
        c = (char)(c - ('a' - 'A'));
    }
    if (c < 'A' || c > 'G') {
        return false;
    }

    switch (c) {
    case 'A':
        *anchor = 9;
        return true;
    case 'B':
        *anchor = 11;
        return true;
    case 'C':
        *anchor = 0;
        return true;
    case 'D':
        *anchor = 2;
        return true;
    case 'E':
        *anchor = 4;
        return true;
    case 'F':
        *anchor = 5;
        return true;
    case 'G':
        *anchor = 7;
        return true;
    default:
        return false;
    }
}

static bool cents_valid(float cents_f)
{
    return isfinite(cents_f) && fabsf(cents_f) <= PITCH_CENTS_MAX;
}

static void log_pitch_update(uint8_t tag, float cents_f, const char *kind)
{
    int64_t now = esp_timer_get_time();
    if (now - s_last_pitch_log_us < PITCH_LOG_US) {
        return;
    }
    s_last_pitch_log_us = now;
    s_last_logged_note = s_tuner.note;
    s_last_logged_cents = s_tuner.cents;

    char proto_ch = (tag >= 0x20 && tag < 0x7F) ? (char)tag : '?';
    /* Never ESP_LOGI on the pitch hot path — USB-JTAG logging in the GATT
     * callback stalls Bluedroid and recreates ~4s of notify backlog. */
    ESP_LOGD(TAG,
             "Pitch %s %c raw %+.1f -> %s %+d",
             kind ? kind : "?",
             proto_ch,
             cents_f,
             (s_tuner.note >= 0 && s_tuner.note <= 11) ? NOTE_NAMES[s_tuner.note] : "?",
             s_tuner.cents);
}

static bool find_tuner_sub(const uint8_t *data, size_t len, uint8_t *sub_out)
{
    if (!data || !sub_out) {
        return false;
    }

    for (size_t i = 0; i + 4 <= len; i++) {
        if (data[i] != 0xC0 || data[i + 1] != 0x08 || data[i + 2] != 0x01) {
            continue;
        }

        uint8_t sub = data[i + 3];
        if (sub == 0x2D || sub == 0x22) {
            *sub_out = sub;
            return true;
        }

        /* Combined notify: C0 08 01 20 <varint> 2D <440 Hz> ... */
        if (sub == 0x20) {
            uint32_t val = 0;
            size_t next = 0;
            if (nano_read_varint(data, len, i + 4, &val, &next) && next + 5 <= len && data[next] == 0x2D &&
                fabsf(read_f32_le(data + next + 1) - 440.f) <= 1.f) {
                *sub_out = 0x2D;
                return true;
            }
        }
    }

    /* Bare 2D + A440 anywhere in the notify. */
    for (size_t i = 0; i + 6 <= len; i++) {
        if (data[i] != 0x2D) {
            continue;
        }
        float ref = read_f32_le(data + i + 1);
        if (fabsf(ref - 440.f) <= 1.f) {
            *sub_out = 0x2D;
            return true;
        }
    }

    return false;
}

/**
 * Track C305 activity for quiet detection. Full hex dumps were hunt-only and
 * stalled the RX worker on UART (~4s backlog through a 48-deep queue).
 */
static void sniff_tuner_c305(const uint8_t *data, size_t len, bool parsed)
{
    if (!data || len == 0) {
        return;
    }

    uint8_t sub = 0;
    bool tuner_shaped = find_tuner_sub(data, len, &sub);
    if (!s_tuner.active && !tuner_shaped) {
        return;
    }

    int64_t now = esp_timer_get_time();
    size_t fp_len = len < TUNER_RAW_FP_MAX ? len : TUNER_RAW_FP_MAX;
    if (fp_len == s_last_raw_fp_len && memcmp(data, s_last_raw_fp, fp_len) == 0 &&
        now - s_last_raw_sniff_us < TUNER_RAW_DEDUP_US) {
        return;
    }

    s_last_raw_sniff_us = now;
    s_last_raw_fp_len = fp_len;
    memcpy(s_last_raw_fp, data, fp_len);

    /* Parsed pitch is covered by Pitch … logs; skip wire spam. */
    if (parsed && sub == 0x22) {
        return;
    }

    /* Mode / odd frames only — debug level so UART stays off the hot path. */
    if (tuner_shaped) {
        ESP_LOGD(TAG,
                 "C305 RX [%s] sub=0x%02X len=%u %s",
                 s_tuner.active ? "live" : "idle",
                 sub,
                 (unsigned)len,
                 parsed ? "parsed" : "raw");
    } else if (s_tuner.active) {
        ESP_LOGD(TAG, "C305 RX [live] len=%u other", (unsigned)len);
    }
}

static void log_unhandled_tuner(const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        return;
    }

    for (size_t i = 0; i + 6 <= len; i++) {
        if (data[i] != 0x2D) {
            continue;
        }
        if (fabsf(read_f32_le(data + i + 1) - 440.f) > 1.f) {
            continue;
        }
        ESP_LOGD(TAG, "Tuner C305 2D unparsed (len=%u)", (unsigned)len);
        return;
    }
}

static void log_pitch_reject(const char *why, uint8_t tag, float cents_f, const uint8_t *data, size_t len, size_t i)
{
    int64_t now = esp_timer_get_time();
    if (now - s_last_pitch_reject_us < PITCH_REJECT_LOG_US) {
        return;
    }
    s_last_pitch_reject_us = now;

    if (i + 12 <= len) {
        ESP_LOGD(TAG,
                 "Pitch reject (%s) tag 0x%02X cents=%g b5..b11: %02X %02X %02X %02X %02X %02X %02X",
                 why,
                 tag,
                 cents_f,
                 data[i + 5],
                 data[i + 6],
                 data[i + 7],
                 data[i + 8],
                 data[i + 9],
                 data[i + 10],
                 data[i + 11]);
    } else {
        ESP_LOGD(TAG, "Pitch reject (%s) tag 0x%02X cents=%g (short %u)", why, tag, cents_f, (unsigned)len);
    }
}

/**
 * Wire tag + accidental + cents → continuous pitch in semitone units (C=0).
 * 22/02# D +40 and 22/01 E −60 both become 3.40 — same sounding pitch.
 */
static float abs_pitch_from_wire(int8_t anchor, int accidental, float cents_f)
{
    return (float)anchor + (float)accidental + cents_f / 100.f;
}

static void note_from_pitch(float pitch, int8_t *note, int16_t *display_cents, float *display_cents_f)
{
    int rounded = (int)floorf(pitch + 0.5f);
    float residual = pitch - (float)rounded;
    float cf = residual * 100.f;
    if (cf > 50.f) {
        cf = 50.f;
    } else if (cf < -50.f) {
        cf = -50.f;
    }

    *note = (int8_t)(((rounded % 12) + 12) % 12);
    if (display_cents_f) {
        *display_cents_f = cf;
    }
    *display_cents = (int16_t)lroundf(cf);
}

static void clear_pitch_reading(void)
{
    s_tuner.has_pitch = false;
    s_tuner.note = -1;
    s_tuner.cents = 0;
    s_last_raw_tag = 0;
    s_last_pitch_kind = 0;
    s_last_raw_cents = 0.f;
    s_display_cents_f = 0.f;
    s_have_abs_pitch = false;
    tuner_publish();
}

static bool apply_pitch_reading(uint8_t tag, float cents_f, bool form_2201, bool sharp, int8_t anchor)
{
    int8_t kind = form_2201 ? 1 : (sharp ? 3 : 2);
    int64_t now = esp_timer_get_time();
    int accidental = sharp ? 1 : 0;
    float abs_pitch = abs_pitch_from_wire(anchor, accidental, cents_f);

    /*
     * Always nearest-note — same path as a fresh pluck after silence.
     * Continuous hysteresis made sustained tuning feel seconds behind while
     * values were "correct" once the held spelling finally flipped.
     */
    int8_t note = -1;
    int16_t display_cents = 0;
    float display_cents_f = 0.f;
    note_from_pitch(abs_pitch, &note, &display_cents, &display_cents_f);

    s_prev_pitch_packet_us = s_last_pitch_packet_us;
    s_last_pitch_packet_us = now;
    s_last_pitch_us = now;
    s_last_abs_pitch = abs_pitch;
    s_display_cents_f = display_cents_f;
    s_have_abs_pitch = true;
    s_last_raw_tag = tag;
    s_last_pitch_kind = kind;
    s_last_raw_cents = cents_f;

    /* Always publish — fractional cents move even when the int label does not. */
    s_tuner.has_pitch = true;
    s_tuner.note = note;
    s_tuner.cents = display_cents;
    tuner_publish();

    const char *kind_name = form_2201 ? "22/01" : (sharp ? "22/02#" : "22/02");
    log_pitch_update(tag, cents_f, kind_name);
    return true;
}

static bool find_pitch_cents(const uint8_t *data, size_t len, size_t i, size_t *cents_off,
                             bool *sharp_out)
{
    if (sharp_out) {
        *sharp_out = false;
    }

    /* 22 02 <letter> '#' 2D <cents> — 0x23 is ASCII sharp. */
    if (i + 6 < len && data[i + 6] == PITCH_MAGIC0 && i + 7 < len && data[i + 7] == PITCH_MAGIC1) {
        *cents_off = i + 8;
        if (sharp_out) {
            *sharp_out = true;
        }
        return true;
    }
    if (i + 6 < len && data[i + 6] == PITCH_MAGIC1) {
        *cents_off = i + 7;
        return true;
    }
    for (size_t j = i + 5; j + 5 <= len && j <= i + 8; j++) {
        if (data[j] == PITCH_MAGIC1) {
            *cents_off = j + 1;
            if (sharp_out && j > i + 5 && data[j - 1] == PITCH_MAGIC0) {
                *sharp_out = true;
            }
            return true;
        }
    }
    return false;
}

static bool try_parse_pitch_at(const uint8_t *data, size_t len, uint8_t form)
{
    if (rx_active_blocked()) {
        return false;
    }

    for (size_t i = 0; i + 10 <= len; i++) {
        if (!match_header(data, len, i, 0x22, form)) {
            continue;
        }

        size_t cents_off = 0;
        bool sharp = false;
        uint8_t tag = data[i + 5];
        if (!find_pitch_cents(data, len, i, &cents_off, &sharp)) {
            log_pitch_reject(form == 0x01 ? "22/01 no 2D" : "22/02 no 2D", tag, 0.f, data, len, i);
            continue;
        }

        /*
         * 0x23 ('#') is a sharp accidental only on 22/02.
         * On 22/01 it is optional framing before the cents float — never a sharp.
         */
        if (form == 0x01) {
            sharp = false;
        }

        if (cents_off + 4 > len) {
            log_pitch_reject("truncated float", tag, 0.f, data, len, i);
            continue;
        }

        float wire = read_f32_le(data + cents_off);
        /* Both forms carry real cents; 22/01 is just a narrow fine-lock window. */
        float cents_f = wire;
        if (!cents_valid(cents_f)) {
            log_pitch_reject("bad cents", tag, wire, data, len, i);
            continue;
        }

        int8_t anchor = -1;
        if (!note_anchor_from_tag(tag, &anchor)) {
            log_pitch_reject("bad tag", tag, cents_f, data, len, i);
            continue;
        }

        if (!s_tuner.active) {
            ESP_LOGI(TAG, "Tuner ON (device, 22/%02X pitch)", form);
        }
        mark_tuner_live_from_c305();
        return apply_pitch_reading(tag, cents_f, form == 0x01, sharp, anchor);
    }

    return false;
}

static bool try_parse_pitch(const uint8_t *data, size_t len)
{
    if (try_parse_pitch_at(data, len, 0x02)) {
        return true;
    }
    return try_parse_pitch_at(data, len, 0x01);
}

/**
 * Footswitch ON often sends gain then 2D in one notify:
 *   C0 08 01 20 01 2D <440> 7F        (short mute — no 38 01 prefix)
 * Footswitch OFF sends direct 2D:
 *   C0 08 01 2D <440> 7F
 */
static bool gain_prefix_before_2d(const uint8_t *data, size_t len, size_t i_2d)
{
    if (i_2d < 6) {
        return false;
    }

    for (size_t h = 0; h + 5 <= i_2d; h++) {
        if (data[h] != 0xC0 || data[h + 1] != 0x08 || data[h + 2] != 0x01 || data[h + 3] != 0x20) {
            continue;
        }

        uint32_t val = 0;
        size_t next = 0;
        if (nano_read_varint(data, len, h + 4, &val, &next) && next == i_2d) {
            return true;
        }
    }

    return false;
}

static bool direct_2d_header_before(const uint8_t *data, size_t len, size_t i_2d)
{
    if (i_2d < 3) {
        return false;
    }

    /* C0 08 01 2D — 2D sits at i_2d (may follow a length byte). */
    size_t h = i_2d - 3;
    return h + 4 <= len && data[h] == 0xC0 && data[h + 1] == 0x08 && data[h + 2] == 0x01 &&
           data[h + 3] == 0x2D;
}

static void apply_gain_2d_route(const uint8_t *data, size_t len, bool has_38_01, bool was_active)
{
    ESP_LOGD(TAG,
             "Tuner gain-2D rx len=%u %s trailer=%s (last=%s route=%s)",
             (unsigned)len,
             was_active ? "live" : "ON",
             trailer_shape_name(has_38_01),
             s_mode_shape_valid ? trailer_shape_name(s_mode_has_38_01) : "none",
             route_name(s_tuner.route));

    if (was_active && gain_2d_is_rapid_retransmit(data, len)) {
        ESP_LOGD(TAG, "Tuner route skip: BLE retransmit");
        return;
    }

    bool same_shape = s_mode_shape_valid && has_38_01 == s_mode_has_38_01;
    bool same_shape_after_on = same_shape && s_allow_same_shape_mode;

    if (gain_2d_is_heartbeat(has_38_01, was_active)) {
        ESP_LOGD(TAG, "Tuner route skip: heartbeat (trailer=%s)", trailer_shape_name(has_38_01));
        return;
    }

    s_allow_same_shape_mode = false;

    nano_tuner_route_t route = resolve_gain_2d_route(has_38_01, was_active, same_shape_after_on);
    apply_tuner_mode_c305(true, route);

    if (route == NANO_TUNER_ROUTE_UNKNOWN) {
        ESP_LOGD(TAG, "Tuner route skip: unknown");
        return;
    }

    s_mode_has_38_01 = has_38_01;
    s_mode_shape_valid = true;
    s_last_route_apply_us = esp_timer_get_time();
    store_gain_2d_fingerprint(data, len);
    if (!was_active) {
        s_allow_same_shape_mode = true;
    }
}

/**
 * Scan for 2D + 440 Hz anywhere in the notify.
 * Handles combined packets: C0 08 01 20 01 2D 00 00 DC 43 38 01 7F
 */
static bool try_parse_2d_events(const uint8_t *data, size_t len)
{
    for (size_t i = 0; i + 6 <= len; i++) {
        if (data[i] != 0x2D) {
            continue;
        }

        float ref = read_f32_le(data + i + 1);
        if (fabsf(ref - 440.f) > 1.f) {
            continue;
        }

        if (is_direct_2d_off(data, len, i)) {
            s_ignore_active_until_us = esp_timer_get_time() + RX_IGNORE_ACTIVE_US;
            apply_tuner_mode_c305(false, NANO_TUNER_ROUTE_UNKNOWN);
            return true;
        }

        bool has_38_01 = false;
        if (!is_gain_2d_route(data, len, i, &has_38_01)) {
            if (gain_prefix_before_2d(data, len, i)) {
                ESP_LOGD(TAG, "Tuner 2D ignored: gain prefix but unknown trailer");
            }
            continue;
        }

        apply_gain_2d_route(data, len, has_38_01, s_tuner.active);
        return true;
    }

    return false;
}

/** Mute/bypass toggle may only change the 38 01 XX trailer on an existing 2D packet. */
static bool try_parse_route_trailer(const uint8_t *data, size_t len)
{
    if (!data || len < 3) {
        return false;
    }

    for (size_t i = 0; i + 3 <= len; i++) {
        if (data[i] != 0x38 || data[i + 1] != 0x01) {
            continue;
        }

        /* Pitch keepalives also contain 38 01 80 — only treat as route in 2D context. */
        size_t anchor_2d = 0;
        bool in_2d = false;
        for (size_t j = (i >= 8 ? i - 8 : 0); j + 5 <= i && j + 5 <= len; j++) {
            if (data[j] == 0x2D && fabsf(read_f32_le(data + j + 1) - 440.f) <= 1.f) {
                anchor_2d = j;
                in_2d = true;
                break;
            }
        }
        if (!in_2d || !gain_prefix_before_2d(data, len, anchor_2d)) {
            continue;
        }

        apply_gain_2d_route(data, len, true, s_tuner.active);
        return true;
    }

    return false;
}

static bool try_parse_on(const uint8_t *data, size_t len)
{
    if (rx_active_blocked()) {
        return false;
    }

    for (size_t i = 0; i + 5 <= len; i++) {
        if (!match_header(data, len, i, 0x22, 0x01)) {
            continue;
        }

        if (s_tuner.active) {
            return false;
        }

        ESP_LOGI(TAG, "Tuner ON (device, 22/01 header)");
        apply_tuner_mode_c305(true, NANO_TUNER_ROUTE_UNKNOWN);
        return true;
    }

    return false;
}

void nano_tuner_reset(void)
{
    ensure_tick_timer();
    s_tuner.active = false;
    s_tuner.has_pitch = false;
    s_tuner.note = -1;
    s_tuner.cents = 0;
    s_tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
    s_last_pitch_log_us = 0;
    s_last_pitch_reject_us = 0;
    s_last_raw_sniff_us = 0;
    s_last_raw_fp_len = 0;
    memset(s_last_raw_fp, 0, sizeof(s_last_raw_fp));
    s_last_logged_note = -1;
    s_last_logged_cents = 0;
    s_last_pitch_us = 0;
    s_last_pitch_packet_us = 0;
    s_prev_pitch_packet_us = 0;
    s_display_cents_f = 0.f;
    s_have_abs_pitch = false;
    s_last_abs_pitch = 0.f;
    s_ignore_active_until_us = 0;
    s_last_raw_tag = 0;
    s_last_pitch_kind = 0;
    s_last_raw_cents = 0.f;
    s_c305_authority_until_us = 0;
    s_c305_authority_active = false;
    clear_gain_2d_route_state();
    tuner_publish();
    push_ui();
    nano_state_tuner_watch_update(false);
}

void nano_tuner_tick(void)
{
    expire_stale_pitch();
}

bool nano_tuner_feed_c305(const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        expire_stale_pitch();
        return false;
    }

    expire_stale_pitch();

    bool parsed = false;

    if (try_parse_2d_events(data, len)) {
        parsed = true;
    } else if (try_parse_route_trailer(data, len)) {
        parsed = true;
    } else if (try_parse_pitch(data, len)) {
        parsed = true;
    } else if (try_parse_on(data, len)) {
        parsed = true;
    } else {
        log_unhandled_tuner(data, len);
    }

    sniff_tuner_c305(data, len, parsed);

    if (parsed) {
        return true;
    }

    uint8_t sub = 0;
    return find_tuner_sub(data, len, &sub);
}

bool nano_tuner_feed_pitch_fast(const uint8_t *data, size_t len)
{
    if (!data || len == 0 || rx_active_blocked()) {
        return false;
    }

    /* Pitch only — no sniff, no 2D/mode, safe to call from GATT notify context. */
    if (try_parse_pitch(data, len)) {
        s_last_raw_sniff_us = esp_timer_get_time();
        return true;
    }
    return false;
}

int nano_tuner_pitch_age_ms(void)
{
    if (s_last_pitch_packet_us <= 0) {
        return -1;
    }
    int64_t age = esp_timer_get_time() - s_last_pitch_packet_us;
    if (age < 0) {
        return 0;
    }
    return (int)(age / 1000);
}

int nano_tuner_pitch_dt_ms(void)
{
    if (s_last_pitch_packet_us <= 0 || s_prev_pitch_packet_us <= 0) {
        return -1;
    }
    int64_t dt = s_last_pitch_packet_us - s_prev_pitch_packet_us;
    if (dt < 0) {
        return -1;
    }
    return (int)(dt / 1000);
}

float nano_tuner_cents_f(void)
{
    return s_display_cents_f;
}

void nano_tuner_note_c305_rx(void)
{
    s_last_raw_sniff_us = esp_timer_get_time();
}

const nano_tuner_t *nano_tuner_get(void)
{
    return &s_tuner;
}

void nano_tuner_snapshot(nano_tuner_t *out)
{
    if (!out) {
        return;
    }

    uint32_t s1;
    uint32_t s2;
    do {
        s1 = __atomic_load_n(&s_tuner_seq, __ATOMIC_ACQUIRE);
        *out = s_tuner;
        s2 = __atomic_load_n(&s_tuner_seq, __ATOMIC_ACQUIRE);
    } while (s1 != s2);
}

void nano_tuner_sync_from_state(bool active, nano_tuner_route_t route)
{
    ensure_tick_timer();

    /* Mute/bypass can change in preset state while tuner stays on. */
    if (s_tuner.active && active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route != route) {
        s_tuner.route = route;
        ESP_LOGI(TAG, "Tuner route -> %s (preset state)", route_name(route));
        push_ui();
        nano_state_tuner_watch_update(true);
        return;
    }

    if (!nano_tuner_may_apply_state(active)) {
        if (s_tuner.active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route != route) {
            s_tuner.route = route;
            ESP_LOGI(TAG, "Tuner route from preset state -> %s (C305 held)", route_name(route));
            push_ui();
        } else {
            ESP_LOGD(TAG, "Preset state tuner %s ignored (C305 authority)", active ? "ON" : "OFF");
        }
        nano_state_tuner_watch_update(s_tuner.active);
        return;
    }

    bool ui = false;

    if (s_tuner.active != active) {
        s_tuner.active = active;
        ui = true;
        if (active && route != NANO_TUNER_ROUTE_UNKNOWN) {
            s_tuner.route = route;
        } else if (!active) {
            s_tuner.has_pitch = false;
            s_tuner.note = -1;
            s_tuner.cents = 0;
            s_tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
            s_last_raw_tag = 0;
            s_last_pitch_kind = 0;
            s_last_raw_cents = 0.f;
        }
    }

    if (s_tuner.active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route != route) {
        s_tuner.route = route;
        ui = true;
    }

    if (ui) {
        if (s_tuner.active && s_tuner.route != NANO_TUNER_ROUTE_UNKNOWN) {
            ESP_LOGI(TAG, "Tuner sync from preset state -> ON (%s)", route_name(s_tuner.route));
        } else if (s_tuner.active) {
            ESP_LOGI(TAG, "Tuner sync from preset state -> ON (route unknown)");
        } else {
            ESP_LOGI(TAG, "Tuner sync from preset state -> OFF");
        }
        push_ui();
    } else if (s_tuner.active && route != NANO_TUNER_ROUTE_UNKNOWN && s_tuner.route == NANO_TUNER_ROUTE_UNKNOWN) {
        s_tuner.route = route;
        ESP_LOGI(TAG, "Tuner route -> %s (preset state)", route_name(route));
        push_ui();
    }

    nano_state_tuner_watch_update(s_tuner.active);
}

bool nano_tuner_set_active(bool active)
{
    if (!nano_ble_connected()) {
        return false;
    }

    uint8_t buf[24];

    if (active) {
        /* BLE enable is unconfirmed; footswitch is reliable. Try 2D toggle once. */
        size_t n2d = nano_tuner_enable_payload(buf, sizeof(buf));
        if (n2d == 0) {
            return false;
        }
        nano_log_bytes(TAG, "Tuner ON (tx)", buf, n2d);
        if (nano_ble_write_c304("Tuner ON", buf, n2d) != ESP_OK) {
            return false;
        }
        s_tuner.active = true;
        s_tuner.has_pitch = false;
        s_tuner.route = NANO_TUNER_ROUTE_MUTE;
        note_c305_authority(true);
        nano_ble_flush_c305_backlog();
        tuner_publish();
        push_ui();
        nano_state_tuner_watch_update(true);
        return true;
    }

    size_t n = nano_tuner_disable_payload(buf, sizeof(buf));
    if (n == 0) {
        return false;
    }
    nano_log_bytes(TAG, "Tuner OFF (tx)", buf, n);
    if (nano_ble_write_c304("Tuner OFF", buf, n) != ESP_OK) {
        return false;
    }

    s_tuner.active = false;
    s_tuner.has_pitch = false;
    s_tuner.note = -1;
    s_tuner.cents = 0;
    s_tuner.route = NANO_TUNER_ROUTE_UNKNOWN;
    s_ignore_active_until_us = esp_timer_get_time() + RX_IGNORE_ACTIVE_US;
    note_c305_authority(false);
    push_ui();
    nano_state_tuner_watch_update(false);
    nano_state_request_tuner_sync();
    return true;
}
