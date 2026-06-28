#include "nano_ui.h"

#include <stdint.h>
#include <stdio.h>

#include "bsp/board.h"
#include "bsp/lvgl_port.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "lvgl.h"

#include "nano_state.h"
#include "nano_tuner.h"

static const char *TAG = "nano_ui";

#define KNOB_COUNT 5
#define DRAG_WRITE_INTERVAL_US (60 * 1000)

typedef struct {
    const char *label;
    lv_obj_t *slider;
    lv_obj_t *value;
} amp_knob_ui_t;

static lv_obj_t *s_status;
static lv_obj_t *s_tuner_note;
static lv_obj_t *s_tuner_cents;
static lv_obj_t *s_tuner_state;
static lv_obj_t *s_tuner_hint;
static lv_obj_t *s_tuner_btn;
static amp_knob_ui_t s_knobs[KNOB_COUNT];
static bool s_ui_sync;
static bool s_ble_connected;
static int s_drag_knob = -1;
static uint8_t s_last_sent[KNOB_COUNT];
static int64_t s_last_send_us[KNOB_COUNT];
static nano_amp_t s_async_amp;
static bool s_async_amp_have;
static nano_tuner_t s_async_tuner;
static bool s_async_tuner_pending;
static bool s_async_ble_pending;
static bool s_async_ble_connected;

static void apply_amp_locked(const nano_amp_t *amp);
static void set_slider_enabled_locked(bool enabled);

static unsigned pct(uint8_t raw)
{
    return (unsigned)((raw * 100U + 127U) / 255U);
}

static void note_label(char *buf, size_t cap, int8_t note)
{
    if (note < 0 || note > 11) {
        snprintf(buf, cap, "?");
        return;
    }

    static const char *names[] = {
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    };
    snprintf(buf, cap, "%s", names[note]);
}

static void tuner_btn_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED || !s_ble_connected) {
        return;
    }

    const nano_tuner_t *t = nano_tuner_get();
    if (!t) {
        return;
    }

    nano_tuner_set_active(!t->active);
}

static void apply_tuner_locked(const nano_tuner_t *tuner)
{
    if (!s_tuner_note) {
        return;
    }

    if (!s_ble_connected) {
        lv_obj_add_state(s_tuner_btn, LV_STATE_DISABLED);
    } else {
        lv_obj_remove_state(s_tuner_btn, LV_STATE_DISABLED);
    }

    if (!tuner || !tuner->active) {
        lv_label_set_text(s_tuner_note, "-");
        lv_label_set_text(s_tuner_cents, "Tuner off");
        lv_label_set_text(s_tuner_state, s_ble_connected ? "Ready" : "Not connected");
        lv_label_set_text(lv_obj_get_child(s_tuner_btn, 0), "ON");
        lv_label_set_text(s_tuner_hint, "Hold right footswitch to start. OFF works over BLE.");
        return;
    }

    if (tuner->has_pitch) {
        char note_buf[8];
        note_label(note_buf, sizeof(note_buf), tuner->note);
        lv_label_set_text(s_tuner_note, note_buf);

        char cents_buf[24];
        if (tuner->cents > 0) {
            snprintf(cents_buf, sizeof(cents_buf), "+%d cents", tuner->cents);
        } else {
            snprintf(cents_buf, sizeof(cents_buf), "%d cents", tuner->cents);
        }
        lv_label_set_text(s_tuner_cents, cents_buf);
    } else {
        lv_label_set_text(s_tuner_note, "...");
        lv_label_set_text(s_tuner_cents, "Listening...");
    }

    if (tuner->route == NANO_TUNER_ROUTE_MUTE || tuner->route == NANO_TUNER_ROUTE_BYPASS) {
        char state_buf[32];
        snprintf(state_buf, sizeof(state_buf), "Tuner active (%s)", nano_tuner_route_label(tuner->route));
        lv_label_set_text(s_tuner_state, state_buf);
    } else {
        lv_label_set_text(s_tuner_state, "Tuner active");
    }
    lv_label_set_text(lv_obj_get_child(s_tuner_btn, 0), "OFF");
    lv_label_set_text(s_tuner_hint, "Tap OFF to exit, or use the Nano footswitch.");
}

static void async_apply_amp_cb(void *arg)
{
    (void)arg;
    apply_amp_locked(s_async_amp_have ? &s_async_amp : NULL);
}

static void async_apply_tuner_cb(void *arg)
{
    (void)arg;
    if (s_async_tuner_pending) {
        apply_tuner_locked(&s_async_tuner);
        s_async_tuner_pending = false;
    }
}

static void async_apply_ble_cb(void *arg)
{
    (void)arg;
    if (!s_async_ble_pending || !s_status) {
        return;
    }
    s_async_ble_pending = false;
    s_ble_connected = s_async_ble_connected;
    lv_label_set_text(s_status, s_ble_connected ? "BLE: connected" : "BLE: scanning...");
    set_slider_enabled_locked(s_ble_connected);
    apply_tuner_locked(nano_tuner_get());
}

static void build_tuner_panel(lv_obj_t *parent)
{
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_flex_flow(parent, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(parent, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(parent, 16, 0);
    lv_obj_set_style_pad_top(parent, 24, 0);

    s_tuner_note = lv_label_create(parent);
    lv_label_set_text(s_tuner_note, "-");
    lv_obj_set_style_text_color(s_tuner_note, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(s_tuner_note, &lv_font_montserrat_24, 0);

    s_tuner_cents = lv_label_create(parent);
    lv_label_set_text(s_tuner_cents, "Tuner off");
    lv_obj_set_style_text_color(s_tuner_cents, lv_color_hex(0xC0C0C0), 0);
    lv_obj_set_style_text_font(s_tuner_cents, &lv_font_montserrat_20, 0);

    s_tuner_state = lv_label_create(parent);
    lv_label_set_text(s_tuner_state, "Ready");
    lv_obj_set_style_text_color(s_tuner_state, lv_color_hex(0xF0A020), 0);
    lv_obj_set_style_text_font(s_tuner_state, &lv_font_montserrat_14, 0);

    s_tuner_btn = lv_button_create(parent);
    lv_obj_set_size(s_tuner_btn, 140, 44);
    lv_obj_add_event_cb(s_tuner_btn, tuner_btn_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_state(s_tuner_btn, LV_STATE_DISABLED);
    lv_obj_t *btn_label = lv_label_create(s_tuner_btn);
    lv_label_set_text(btn_label, "ON");
    lv_obj_center(btn_label);

    s_tuner_hint = lv_label_create(parent);
    lv_label_set_text(s_tuner_hint, "Hold right footswitch to start. OFF works over BLE.");
    lv_obj_set_style_text_color(s_tuner_hint, lv_color_hex(0x808080), 0);
    lv_obj_set_style_text_font(s_tuner_hint, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_align(s_tuner_hint, LV_TEXT_ALIGN_CENTER, 0);
    lv_label_set_long_mode(s_tuner_hint, LV_LABEL_LONG_WRAP);
    lv_obj_set_width(s_tuner_hint, 520);
}

static void set_slider_enabled_locked(bool enabled)
{
    for (int i = 0; i < KNOB_COUNT; i++) {
        if (enabled) {
            lv_obj_remove_state(s_knobs[i].slider, LV_STATE_DISABLED);
        } else {
            lv_obj_add_state(s_knobs[i].slider, LV_STATE_DISABLED);
        }
    }
}

static void try_send_knob_locked(int idx, uint8_t raw, bool force)
{
    if (s_ui_sync || !s_ble_connected || idx < 0 || idx >= KNOB_COUNT) {
        return;
    }
    if (!force && raw == s_last_sent[idx]) {
        return;
    }

    int64_t now = esp_timer_get_time();
    if (!force && now - s_last_send_us[idx] < DRAG_WRITE_INTERVAL_US) {
        return;
    }

    if (nano_state_write_amp_knob((nano_knob_t)idx, raw)) {
        s_last_sent[idx] = raw;
        s_last_send_us[idx] = now;
    }
}

static void slider_event_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    if (idx < 0 || idx >= KNOB_COUNT) {
        return;
    }

    lv_obj_t *slider = lv_event_get_target_obj(e);

    if (code == LV_EVENT_PRESSED) {
        s_drag_knob = idx;
        return;
    }

    if (code == LV_EVENT_RELEASED) {
        uint8_t raw = (uint8_t)lv_slider_get_value(slider);
        try_send_knob_locked(idx, raw, true);
        s_drag_knob = -1;
        return;
    }

    if (code != LV_EVENT_VALUE_CHANGED) {
        return;
    }

    uint8_t raw = (uint8_t)lv_slider_get_value(slider);

    char buf[12];
    snprintf(buf, sizeof(buf), "%u%%", pct(raw));
    lv_label_set_text(s_knobs[idx].value, buf);

    if (s_drag_knob == idx) {
        try_send_knob_locked(idx, raw, false);
    }
}

static void style_screen(lv_obj_t *screen)
{
    lv_obj_set_style_bg_color(screen, lv_color_hex(0x121212), 0);
    lv_obj_set_style_bg_opa(screen, LV_OPA_COVER, 0);
    lv_obj_set_style_pad_all(screen, 16, 0);
}

static lv_obj_t *make_title(lv_obj_t *parent)
{
    lv_obj_t *title = lv_label_create(parent);
    lv_label_set_text(title, "Nano Cortex");
    lv_obj_set_style_text_color(title, lv_color_hex(0xF0F0F0), 0);
    lv_obj_set_style_text_font(title, &lv_font_montserrat_14, 0);
    return title;
}

static lv_obj_t *make_status(lv_obj_t *parent)
{
    lv_obj_t *status = lv_label_create(parent);
    lv_label_set_text(status, "BLE: scanning...");
    lv_obj_set_style_text_color(status, lv_color_hex(0xA0A0A0), 0);
    lv_obj_set_style_text_font(status, &lv_font_montserrat_14, 0);
    return status;
}

static void style_slider(lv_obj_t *slider)
{
    lv_obj_set_size(slider, 28, 180);
    lv_slider_set_range(slider, 0, 255);
    lv_slider_set_orientation(slider, LV_SLIDER_ORIENTATION_VERTICAL);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0x303030), LV_PART_MAIN);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0xF5F5F5), LV_PART_INDICATOR);
    lv_obj_set_style_pad_all(slider, 4, LV_PART_KNOB);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0xFFFFFF), LV_PART_KNOB);
    lv_obj_add_state(slider, LV_STATE_DISABLED);
}

static void make_knob_column(lv_obj_t *parent, amp_knob_ui_t *knob, const char *label, int idx)
{
    knob->label = label;

    lv_obj_t *col = lv_obj_create(parent);
    lv_obj_set_size(col, 140, 300);
    lv_obj_set_style_bg_opa(col, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(col, 0, 0);
    lv_obj_set_style_pad_all(col, 0, 0);
    lv_obj_set_flex_flow(col, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(col, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(col, 8, 0);

    lv_obj_t *name = lv_label_create(col);
    lv_label_set_text(name, label);
    lv_obj_set_style_text_color(name, lv_color_hex(0xD0D0D0), 0);
    lv_obj_set_style_text_font(name, &lv_font_montserrat_14, 0);
    lv_obj_set_style_text_letter_space(name, 1, 0);

    knob->slider = lv_slider_create(col);
    style_slider(knob->slider);
    lv_obj_add_event_cb(knob->slider, slider_event_cb, LV_EVENT_ALL, (void *)(intptr_t)idx);

    knob->value = lv_label_create(col);
    lv_label_set_text(knob->value, "—");
    lv_obj_set_style_text_color(knob->value, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(knob->value, &lv_font_montserrat_14, 0);
}

static void build_amp_panel(lv_obj_t *parent)
{
    lv_obj_set_size(parent, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_style_pad_all(parent, 0, 0);
    lv_obj_set_flex_flow(parent, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(parent, LV_FLEX_ALIGN_SPACE_EVENLY, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);

    make_knob_column(parent, &s_knobs[0], "GAIN", 0);
    make_knob_column(parent, &s_knobs[1], "LEVEL", 1);
    make_knob_column(parent, &s_knobs[2], "BASS", 2);
    make_knob_column(parent, &s_knobs[3], "MID", 3);
    make_knob_column(parent, &s_knobs[4], "TREBLE", 4);
}

static void build_ui(void)
{
    lv_obj_t *screen = lv_screen_active();
    style_screen(screen);

    lv_obj_t *header = lv_obj_create(screen);
    lv_obj_set_size(header, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_style_bg_opa(header, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(header, 0, 0);
    lv_obj_set_style_pad_all(header, 0, 0);
    lv_obj_set_flex_flow(header, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(header, 6, 0);
    lv_obj_align(header, LV_ALIGN_TOP_MID, 0, 0);

    make_title(header);
    s_status = make_status(header);

    lv_obj_t *tabview = lv_tabview_create(screen);
    lv_obj_set_size(tabview, LV_PCT(100), 380);
    lv_obj_align(tabview, LV_ALIGN_BOTTOM_MID, 0, -8);
    lv_obj_set_style_bg_color(tabview, lv_color_hex(0x121212), LV_PART_MAIN);

    lv_obj_t *amp_tab = lv_tabview_add_tab(tabview, "AMP");
    lv_obj_t *tuner_tab = lv_tabview_add_tab(tabview, "TUNER");

    build_amp_panel(amp_tab);
    build_tuner_panel(tuner_tab);
    apply_tuner_locked(nano_tuner_get());
}

static void apply_amp_locked(const nano_amp_t *amp)
{
    s_ui_sync = true;

    if (!amp || !amp->valid) {
        for (int i = 0; i < KNOB_COUNT; i++) {
            if (i == s_drag_knob) {
                continue;
            }
            lv_slider_set_value(s_knobs[i].slider, 0, LV_ANIM_OFF);
            lv_label_set_text(s_knobs[i].value, "—");
            s_last_sent[i] = 0;
        }
        set_slider_enabled_locked(false);
        s_ui_sync = false;
        return;
    }

    const uint8_t raw[] = {amp->gain, amp->level, amp->bass, amp->mid, amp->treble};
    char buf[12];

    for (int i = 0; i < KNOB_COUNT; i++) {
        s_last_sent[i] = raw[i];
        if (i == s_drag_knob) {
            continue;
        }
        lv_slider_set_value(s_knobs[i].slider, raw[i], LV_ANIM_OFF);
        snprintf(buf, sizeof(buf), "%u%%", pct(raw[i]));
        lv_label_set_text(s_knobs[i].value, buf);
    }

    set_slider_enabled_locked(s_ble_connected);
    s_ui_sync = false;
}

esp_err_t nano_ui_start(void)
{
    esp_lcd_panel_handle_t panel = NULL;
    esp_lcd_touch_handle_t touch = NULL;

    ESP_ERROR_CHECK(waveshare_esp32_s3_rgb_lcd_init(&panel, &touch));
    ESP_ERROR_CHECK(lvgl_port_init(panel, touch));
    ESP_ERROR_CHECK(waveshare_rgb_lcd_bl_on());

    if (!lvgl_port_lock(-1)) {
        return ESP_FAIL;
    }
    build_ui();
    lvgl_port_unlock();

    ESP_LOGI(TAG, "UI ready (800x480, AMP + TUNER tabs)");
    return ESP_OK;
}

void nano_ui_set_amp(const nano_amp_t *amp)
{
    if (amp && amp->valid) {
        s_async_amp = *amp;
        s_async_amp_have = true;
    } else {
        s_async_amp_have = false;
    }
    lv_async_call(async_apply_amp_cb, NULL);
}

void nano_ui_set_ble_connected(bool connected)
{
    s_async_ble_connected = connected;
    s_async_ble_pending = true;
    lv_async_call(async_apply_ble_cb, NULL);
}

void nano_ui_set_tuner(const nano_tuner_t *tuner)
{
    if (!tuner || !s_tuner_note) {
        return;
    }
    s_async_tuner = *tuner;
    s_async_tuner_pending = true;
    lv_async_call(async_apply_tuner_cb, NULL);
}
