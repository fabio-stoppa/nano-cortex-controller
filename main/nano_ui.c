#include "nano_ui.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "bsp/board.h"
#include "bsp/lvgl_port.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/queue.h"
#include "lvgl.h"

#include "nano_state.h"
#include "nano_tuner.h"
#include "nano_fx.h"

static const char *TAG = "nano_ui";

#define KNOB_COUNT 5
#define DRAG_WRITE_INTERVAL_US (60 * 1000)
#define FX_MODEL_OPTIONS_MAX 2048

#define UI_DISP_AMP   (1u << 0)
#define UI_DISP_BLE   (1u << 1)
#define UI_DISP_TUNER (1u << 2)
#define UI_DISP_FX    (1u << 3)

typedef struct {
    uint8_t wake;
} ui_wake_t;

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

static lv_obj_t *s_gate_state;
static lv_obj_t *s_gate_btn;
static lv_obj_t *s_gate_slider;
static lv_obj_t *s_gate_value;
static int s_gate_drag;

static lv_obj_t *s_fx_block_btns[NANO_FX_BLOCK_COUNT];
static lv_obj_t *s_fx_model_label;
static lv_obj_t *s_fx_model_dd;
static char s_fx_model_options[FX_MODEL_OPTIONS_MAX];
static bool s_fx_dd_sync;
static lv_obj_t *s_fx_state_label;
static lv_obj_t *s_fx_load_label;
static lv_obj_t *s_fx_btn;
static lv_obj_t *s_fx_param_rows[NANO_FX_MAX_PARAMS];
static lv_obj_t *s_fx_param_sliders[NANO_FX_MAX_PARAMS];
static lv_obj_t *s_fx_param_values[NANO_FX_MAX_PARAMS];
static nano_fx_block_id_t s_fx_selected = NANO_FX_POST2;
static bool s_fx_ui_sync;
static int s_fx_drag_param = -1;
static int s_fx_last_sent[NANO_FX_MAX_PARAMS];
static int64_t s_fx_last_send_us[NANO_FX_MAX_PARAMS];

static nano_fx_state_t s_async_fx;
static bool s_async_fx_pending;

static QueueHandle_t s_ui_wake_queue;
static volatile uint8_t s_ui_dispatch_mask;

static void apply_amp_locked(const nano_amp_t *amp);
static void apply_fx_locked(const nano_fx_state_t *fx);
static void set_slider_enabled_locked(bool enabled);
static void fx_model_dropdown_refresh(void);
static void ui_request_dispatch(void);
static void ui_init_dispatch(void);
static void async_apply_amp_cb(void *arg);
static void async_apply_tuner_cb(void *arg);
static void async_apply_ble_cb(void *arg);
static void async_apply_fx_cb(void *arg);

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
    apply_fx_locked(s_async_fx_pending ? &s_async_fx : nano_fx_get());
}

static void ui_run_pending_updates(uint8_t mask)
{
    if ((mask & UI_DISP_BLE) && s_async_ble_pending && s_status) {
        async_apply_ble_cb(NULL);
        return;
    }
    if (mask & UI_DISP_AMP) {
        async_apply_amp_cb(NULL);
    }
    if (mask & UI_DISP_TUNER) {
        async_apply_tuner_cb(NULL);
    }
    if (mask & UI_DISP_FX) {
        async_apply_fx_cb(NULL);
    }
}

void nano_ui_dispatch_if_pending(void)
{
    if (!s_ui_wake_queue) {
        return;
    }

    ui_wake_t wake;
    if (xQueueReceive(s_ui_wake_queue, &wake, 0) != pdTRUE) {
        return;
    }

    while (xQueueReceive(s_ui_wake_queue, &wake, 0) == pdTRUE) {
    }

    uint8_t mask = s_ui_dispatch_mask;
    s_ui_dispatch_mask = 0;
    ui_run_pending_updates(mask);
}

static void ui_request_dispatch(void)
{
    if (!s_ui_wake_queue) {
        return;
    }

    ui_wake_t wake = {0};
    xQueueSend(s_ui_wake_queue, &wake, 0);
}

static void ui_init_dispatch(void)
{
    s_ui_wake_queue = xQueueCreate(8, sizeof(ui_wake_t));
    if (!s_ui_wake_queue) {
        ESP_LOGE(TAG, "UI wake queue create failed");
    }
}

static void async_apply_fx_cb(void *arg)
{
    (void)arg;
    if (!s_async_fx_pending) {
        return;
    }
    if (s_fx_model_dd && lv_dropdown_is_open(s_fx_model_dd)) {
        return;
    }
    apply_fx_locked(&s_async_fx);
    s_async_fx_pending = false;
}

static void gate_btn_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED || !s_ble_connected) {
        return;
    }
    const nano_fx_state_t *fx = nano_fx_get();
    bool on = fx ? fx->gate.on : false;
    nano_fx_write_gate_enabled(!on);
}

static void gate_slider_event_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);
    if (!s_gate_slider || !s_ble_connected) {
        return;
    }

    if (code == LV_EVENT_PRESSED) {
        s_gate_drag = 1;
        return;
    }
    if (code == LV_EVENT_RELEASED) {
        uint8_t pct = (uint8_t)lv_slider_get_value(s_gate_slider);
        nano_fx_write_gate_reduction(pct);
        s_gate_drag = 0;
        return;
    }
    if (code != LV_EVENT_VALUE_CHANGED || s_gate_drag == 0) {
        return;
    }

    uint8_t pct = (uint8_t)lv_slider_get_value(s_gate_slider);
    char buf[12];
    snprintf(buf, sizeof(buf), "%u%%", pct);
    lv_label_set_text(s_gate_value, buf);
}

static void fx_block_btn_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED) {
        return;
    }
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    if (idx < 0 || idx >= NANO_FX_BLOCK_COUNT) {
        return;
    }
    s_fx_selected = (nano_fx_block_id_t)idx;
    apply_fx_locked(nano_fx_get());
}

static void fx_toggle_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED || !s_ble_connected) {
        return;
    }
    const nano_fx_state_t *fx = nano_fx_get();
    if (!fx) {
        return;
    }
    const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
    nano_fx_write_block_enabled(s_fx_selected, !block->on);
}

static void fx_model_dd_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_VALUE_CHANGED || s_fx_dd_sync || !s_ble_connected) {
        return;
    }

    uint32_t sel = lv_dropdown_get_selected(lv_event_get_target(e));
    const nano_fx_model_def_t *model = nano_fx_get_allowed_model(s_fx_selected, sel);
    if (model) {
        nano_fx_write_block_model(s_fx_selected, model);
    }
}

static void fx_model_dropdown_refresh(void)
{
    if (!s_fx_model_dd || lv_dropdown_is_open(s_fx_model_dd)) {
        return;
    }

    const nano_fx_state_t *fx = nano_fx_get();
    const nano_fx_block_t *cur = fx ? &fx->blocks[s_fx_selected] : NULL;
    size_t count = nano_fx_allowed_model_count(s_fx_selected);

    char *out = s_fx_model_options;
    size_t remain = sizeof(s_fx_model_options);
    uint32_t selected = 0;

    for (size_t i = 0; i < count; i++) {
        const nano_fx_model_def_t *model = nano_fx_get_allowed_model(s_fx_selected, i);
        if (!model || !model->name) {
            continue;
        }
        if (cur) {
            if (cur->model == model) {
                selected = (uint32_t)i;
            } else if (cur->has_model_id && model->id_len > 0 &&
                       cur->model_id_len == model->id_len &&
                       memcmp(cur->model_id_raw, model->id, model->id_len) == 0) {
                selected = (uint32_t)i;
            }
        }
        int n = snprintf(out, remain, "%s\n", model->name);
        if (n <= 0 || (size_t)n >= remain) {
            break;
        }
        out += n;
        remain -= (size_t)n;
    }

    if (out > s_fx_model_options) {
        out[-1] = '\0';
    } else {
        s_fx_model_options[0] = '\0';
    }

    s_fx_dd_sync = true;
    lv_dropdown_set_options(s_fx_model_dd, s_fx_model_options);
    if (count > 0) {
        lv_dropdown_set_selected(s_fx_model_dd, selected);
    }
    s_fx_dd_sync = false;
}

static void fx_short_param_name(const char *full, char *out, size_t cap)
{
    if (!full || !out || cap == 0) {
        return;
    }

    size_t n = 0;
    for (size_t i = 0; full[i] && n + 1 < cap; i++) {
        if (full[i] == '(') {
            break;
        }
        out[n++] = full[i];
    }
    while (n > 0 && out[n - 1] == ' ') {
        n--;
    }
    out[n] = '\0';
    if (n == 0) {
        snprintf(out, cap, "%s", full);
    }
}

static void try_send_fx_param_locked(int idx, int slider_val, bool force)
{
    if (s_fx_ui_sync || !s_ble_connected || idx < 0 || idx >= NANO_FX_MAX_PARAMS) {
        return;
    }

    const nano_fx_state_t *fx = nano_fx_get();
    if (!fx) {
        return;
    }
    const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
    if (!block->model || (uint8_t)idx >= block->param_count) {
        return;
    }

    const char *pname = block->model->param_names[idx];
    int64_t now = esp_timer_get_time();
    if (!force && slider_val == s_fx_last_sent[idx] &&
        now - s_fx_last_send_us[idx] < DRAG_WRITE_INTERVAL_US) {
        return;
    }

    float display = nano_fx_slider_to_display(pname, slider_val);
    if (nano_fx_write_param_display(s_fx_selected, (uint8_t)idx, display)) {
        s_fx_last_sent[idx] = slider_val;
        s_fx_last_send_us[idx] = now;
    }
}

static void fx_param_slider_event_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    if (idx < 0 || idx >= NANO_FX_MAX_PARAMS) {
        return;
    }

    lv_obj_t *slider = lv_event_get_target_obj(e);

    if (code == LV_EVENT_PRESSED) {
        s_fx_drag_param = idx;
        return;
    }
    if (code == LV_EVENT_RELEASED) {
        try_send_fx_param_locked(idx, (int)lv_slider_get_value(slider), true);
        s_fx_drag_param = -1;
        return;
    }
    if (code != LV_EVENT_VALUE_CHANGED) {
        return;
    }

    int slider_val = (int)lv_slider_get_value(slider);
    const nano_fx_state_t *fx = nano_fx_get();
    if (fx && fx->blocks[s_fx_selected].model && (uint8_t)idx < fx->blocks[s_fx_selected].param_count) {
        const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
        char val[24];
        nano_fx_block_t tmp = *block;
        tmp.params[idx].display = nano_fx_slider_to_display(block->model->param_names[idx], slider_val);
        tmp.params[idx].valid = true;
        nano_fx_format_param(&tmp, (uint8_t)idx, val, sizeof(val));
        lv_label_set_text(s_fx_param_values[idx], val);
    }

    if (s_fx_drag_param == idx) {
        try_send_fx_param_locked(idx, slider_val, false);
    }
}

static void style_h_slider(lv_obj_t *slider, int32_t min_v, int32_t max_v)
{
    lv_obj_set_width(slider, 280);
    lv_slider_set_range(slider, min_v, max_v);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0x303030), LV_PART_MAIN);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0xF5F5F5), LV_PART_INDICATOR);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0xFFFFFF), LV_PART_KNOB);
}

static void apply_fx_locked(const nano_fx_state_t *fx)
{
    s_fx_ui_sync = true;

    if (s_gate_btn) {
        if (!s_ble_connected) {
            lv_obj_add_state(s_gate_btn, LV_STATE_DISABLED);
            lv_obj_add_state(s_gate_slider, LV_STATE_DISABLED);
        } else {
            lv_obj_remove_state(s_gate_btn, LV_STATE_DISABLED);
            lv_obj_remove_state(s_gate_slider, LV_STATE_DISABLED);
        }
    }

    if (s_gate_state) {
        if (!fx || !fx->gate.valid) {
            lv_label_set_text(s_gate_state, "Gate: —");
            lv_label_set_text(lv_obj_get_child(s_gate_btn, 0), "ON");
            if (s_gate_drag == 0) {
                lv_slider_set_value(s_gate_slider, 50, LV_ANIM_OFF);
                lv_label_set_text(s_gate_value, "—");
            }
        } else {
            lv_label_set_text(s_gate_state, fx->gate.on ? "Gate: ON" : "Gate: OFF");
            lv_label_set_text(lv_obj_get_child(s_gate_btn, 0), fx->gate.on ? "OFF" : "ON");
            if (s_gate_drag == 0) {
                lv_slider_set_value(s_gate_slider, fx->gate.reduction_pct, LV_ANIM_OFF);
                char buf[12];
                snprintf(buf, sizeof(buf), "%u%%", fx->gate.reduction_pct);
                lv_label_set_text(s_gate_value, buf);
            }
        }
    }

    if (!s_fx_model_label) {
        s_fx_ui_sync = false;
        return;
    }

    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        if (!s_fx_block_btns[i]) {
            continue;
        }
        if (i == (int)s_fx_selected) {
            lv_obj_add_state(s_fx_block_btns[i], LV_STATE_CHECKED);
        } else {
            lv_obj_remove_state(s_fx_block_btns[i], LV_STATE_CHECKED);
        }

        const nano_fx_block_t *b = fx ? &fx->blocks[i] : NULL;
        if (b && b->valid && b->on) {
            lv_obj_set_style_border_color(s_fx_block_btns[i], lv_color_hex(0x40C060), LV_PART_MAIN);
            lv_obj_set_style_border_width(s_fx_block_btns[i], 2, LV_PART_MAIN);
        } else {
            lv_obj_set_style_border_width(s_fx_block_btns[i], 0, LV_PART_MAIN);
        }
    }

    const nano_fx_block_t *block = fx ? &fx->blocks[s_fx_selected] : NULL;

    if (!fx || !block || !block->valid) {
        lv_label_set_text(s_fx_model_label, "Model: —");
        lv_label_set_text(s_fx_state_label, "—");
        lv_label_set_text(s_fx_load_label, "");
        lv_label_set_text(lv_obj_get_child(s_fx_btn, 0), "ON");
        for (int i = 0; i < NANO_FX_MAX_PARAMS; i++) {
            lv_obj_add_flag(s_fx_param_rows[i], LV_OBJ_FLAG_HIDDEN);
        }
        if (s_fx_btn) {
            lv_obj_add_state(s_fx_btn, LV_STATE_DISABLED);
        }
        if (s_fx_model_dd) {
            lv_obj_add_state(s_fx_model_dd, LV_STATE_DISABLED);
        }
        s_fx_ui_sync = false;
        return;
    }

    if (s_fx_model_dd) {
        if (s_ble_connected) {
            lv_obj_remove_state(s_fx_model_dd, LV_STATE_DISABLED);
        } else {
            lv_obj_add_state(s_fx_model_dd, LV_STATE_DISABLED);
        }
    }

    if (s_fx_btn) {
        if (s_ble_connected) {
            lv_obj_remove_state(s_fx_btn, LV_STATE_DISABLED);
        } else {
            lv_obj_add_state(s_fx_btn, LV_STATE_DISABLED);
        }
    }

    char model_text[56];
    char model_buf[64];
    nano_fx_block_model_text(block, model_text, sizeof(model_text));
    snprintf(model_buf, sizeof(model_buf), "Model: %s", model_text);
    lv_label_set_text(s_fx_model_label, model_buf);

    char state_buf[48];
    snprintf(state_buf, sizeof(state_buf), "%s — %s", nano_fx_block_label(s_fx_selected),
             block->on ? "ON" : "OFF");
    lv_label_set_text(s_fx_state_label, state_buf);
    lv_label_set_text(lv_obj_get_child(s_fx_btn, 0), block->on ? "OFF" : "ON");

    if (block->on && !block->params_loaded &&
        (block->param_count > 0 || block->has_model_id)) {
        lv_label_set_text(s_fx_load_label, "Loading params...");
    } else if (!block->has_model_id && block->on && !block->params_loaded) {
        lv_label_set_text(s_fx_load_label, "Syncing block...");
    } else {
        lv_label_set_text(s_fx_load_label, "");
    }

    for (int i = 0; i < NANO_FX_MAX_PARAMS; i++) {
        if (i >= block->param_count || block->param_count == 0) {
            lv_obj_add_flag(s_fx_param_rows[i], LV_OBJ_FLAG_HIDDEN);
            continue;
        }

        if (!block->model) {
            lv_obj_add_flag(s_fx_param_rows[i], LV_OBJ_FLAG_HIDDEN);
            continue;
        }

        lv_obj_remove_flag(s_fx_param_rows[i], LV_OBJ_FLAG_HIDDEN);

        const char *pname = block->model->param_names[i];
        char short_name[32];
        fx_short_param_name(pname, short_name, sizeof(short_name));
        lv_obj_t *name = lv_obj_get_child(s_fx_param_rows[i], 0);
        lv_label_set_text(name, short_name);

        int smin = nano_fx_slider_min(pname);
        int smax = nano_fx_slider_max(pname);
        lv_slider_set_range(s_fx_param_sliders[i], smin, smax);

        if (i != s_fx_drag_param) {
            float display =
                block->params[i].valid ? block->params[i].display
                                       : nano_fx_slider_to_display(pname, (smin + smax) / 2);
            int slider_val = nano_fx_display_to_slider(pname, display);
            lv_slider_set_value(s_fx_param_sliders[i], slider_val, LV_ANIM_OFF);
            char val[24];
            nano_fx_block_t tmp = *block;
            if (!tmp.params[i].valid) {
                tmp.params[i].display = display;
                tmp.params[i].valid = true;
            }
            nano_fx_format_param(&tmp, (uint8_t)i, val, sizeof(val));
            lv_label_set_text(s_fx_param_values[i], val);
            s_fx_last_sent[i] = slider_val;
        }

        if (s_ble_connected && block->on && block->param_count > 0) {
            lv_obj_remove_state(s_fx_param_sliders[i], LV_STATE_DISABLED);
        } else {
            lv_obj_add_state(s_fx_param_sliders[i], LV_STATE_DISABLED);
        }
    }

    fx_model_dropdown_refresh();
    s_fx_ui_sync = false;
}

static void build_gate_panel(lv_obj_t *parent)
{
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_flex_flow(parent, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(parent, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(parent, 16, 0);
    lv_obj_set_style_pad_top(parent, 24, 0);

    s_gate_state = lv_label_create(parent);
    lv_label_set_text(s_gate_state, "Gate: —");
    lv_obj_set_style_text_color(s_gate_state, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(s_gate_state, &lv_font_montserrat_20, 0);

    s_gate_btn = lv_button_create(parent);
    lv_obj_set_size(s_gate_btn, 140, 44);
    lv_obj_add_event_cb(s_gate_btn, gate_btn_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_state(s_gate_btn, LV_STATE_DISABLED);
    lv_obj_t *gate_btn_label = lv_label_create(s_gate_btn);
    lv_label_set_text(gate_btn_label, "ON");
    lv_obj_center(gate_btn_label);

    lv_obj_t *red_row = lv_obj_create(parent);
    lv_obj_set_size(red_row, 420, 60);
    lv_obj_set_style_bg_opa(red_row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(red_row, 0, 0);
    lv_obj_set_flex_flow(red_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(red_row, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(red_row, 12, 0);

    lv_obj_t *red_label = lv_label_create(red_row);
    lv_label_set_text(red_label, "Reduction");
    lv_obj_set_style_text_color(red_label, lv_color_hex(0xC0C0C0), 0);

    s_gate_slider = lv_slider_create(red_row);
    style_h_slider(s_gate_slider, 0, 100);
    lv_obj_add_event_cb(s_gate_slider, gate_slider_event_cb, LV_EVENT_ALL, NULL);
    lv_obj_add_state(s_gate_slider, LV_STATE_DISABLED);

    s_gate_value = lv_label_create(red_row);
    lv_label_set_text(s_gate_value, "—");
    lv_obj_set_style_text_color(s_gate_value, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_width(s_gate_value, 48);
}

static void build_fx_panel(lv_obj_t *parent)
{
    lv_obj_set_size(parent, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_style_pad_all(parent, 4, 0);
    lv_obj_set_flex_flow(parent, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(parent, 6, 0);

    lv_obj_t *block_row = lv_obj_create(parent);
    lv_obj_set_size(block_row, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_style_bg_opa(block_row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(block_row, 0, 0);
    lv_obj_set_flex_flow(block_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(block_row, LV_FLEX_ALIGN_SPACE_EVENLY, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(block_row, 4, 0);

    static const char *block_short[] = {"PRE1", "PRE2", "P1", "P2", "P3"};
    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        s_fx_block_btns[i] = lv_button_create(block_row);
        lv_obj_set_size(s_fx_block_btns[i], 72, 32);
        lv_obj_add_flag(s_fx_block_btns[i], LV_OBJ_FLAG_CHECKABLE);
        lv_obj_add_event_cb(s_fx_block_btns[i], fx_block_btn_event_cb, LV_EVENT_CLICKED,
                            (void *)(intptr_t)i);
        lv_obj_t *lbl = lv_label_create(s_fx_block_btns[i]);
        lv_label_set_text(lbl, block_short[i]);
        lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, 0);
        lv_obj_center(lbl);
    }

    s_fx_model_label = lv_label_create(parent);
    lv_label_set_text(s_fx_model_label, "—");
    lv_obj_set_style_text_color(s_fx_model_label, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(s_fx_model_label, &lv_font_montserrat_14, 0);

    lv_obj_t *model_row = lv_obj_create(parent);
    lv_obj_set_size(model_row, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_style_bg_opa(model_row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(model_row, 0, 0);
    lv_obj_set_flex_flow(model_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(model_row, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(model_row, 8, 0);
    lv_obj_move_to_index(model_row, lv_obj_get_index(s_fx_model_label));

    lv_obj_set_parent(s_fx_model_label, model_row);
    lv_obj_set_flex_grow(s_fx_model_label, 1);

    s_fx_model_dd = lv_dropdown_create(model_row);
    lv_obj_set_width(s_fx_model_dd, 120);
    lv_dropdown_set_text(s_fx_model_dd, "Switch");
    lv_obj_set_style_text_font(s_fx_model_dd, &lv_font_montserrat_14, LV_PART_MAIN);
    lv_obj_add_event_cb(s_fx_model_dd, fx_model_dd_event_cb, LV_EVENT_VALUE_CHANGED, NULL);
    lv_obj_add_state(s_fx_model_dd, LV_STATE_DISABLED);

    lv_obj_t *head_row = lv_obj_create(parent);
    lv_obj_set_size(head_row, LV_PCT(100), LV_SIZE_CONTENT);
    lv_obj_set_style_bg_opa(head_row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(head_row, 0, 0);
    lv_obj_set_flex_flow(head_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(head_row, LV_FLEX_ALIGN_SPACE_BETWEEN, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);

    s_fx_state_label = lv_label_create(head_row);
    lv_label_set_text(s_fx_state_label, "—");
    lv_obj_set_style_text_color(s_fx_state_label, lv_color_hex(0xA0A0A0), 0);
    lv_obj_set_style_text_font(s_fx_state_label, &lv_font_montserrat_14, 0);

    s_fx_btn = lv_button_create(head_row);
    lv_obj_set_size(s_fx_btn, 80, 32);
    lv_obj_add_event_cb(s_fx_btn, fx_toggle_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_state(s_fx_btn, LV_STATE_DISABLED);
    lv_obj_t *fx_btn_label = lv_label_create(s_fx_btn);
    lv_label_set_text(fx_btn_label, "ON");
    lv_obj_center(fx_btn_label);

    s_fx_load_label = lv_label_create(parent);
    lv_label_set_text(s_fx_load_label, "");
    lv_obj_set_style_text_color(s_fx_load_label, lv_color_hex(0xF0A020), 0);
    lv_obj_set_style_text_font(s_fx_load_label, &lv_font_montserrat_14, 0);

    lv_obj_t *scroll = lv_obj_create(parent);
    lv_obj_set_size(scroll, LV_PCT(100), 200);
    lv_obj_set_style_bg_opa(scroll, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(scroll, 0, 0);
    lv_obj_set_flex_flow(scroll, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(scroll, 4, 0);
    lv_obj_add_flag(scroll, LV_OBJ_FLAG_SCROLLABLE);

    for (int i = 0; i < NANO_FX_MAX_PARAMS; i++) {
        s_fx_param_rows[i] = lv_obj_create(scroll);
        lv_obj_set_size(s_fx_param_rows[i], LV_PCT(100), 34);
        lv_obj_set_style_bg_opa(s_fx_param_rows[i], LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_width(s_fx_param_rows[i], 0, 0);
        lv_obj_set_flex_flow(s_fx_param_rows[i], LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(s_fx_param_rows[i], LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER,
                              LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_column(s_fx_param_rows[i], 6, 0);
        lv_obj_add_flag(s_fx_param_rows[i], LV_OBJ_FLAG_HIDDEN);

        lv_obj_t *name = lv_label_create(s_fx_param_rows[i]);
        lv_label_set_text(name, "Param");
        lv_obj_set_width(name, 96);
        lv_label_set_long_mode(name, LV_LABEL_LONG_DOT);
        lv_obj_set_style_text_color(name, lv_color_hex(0xC0C0C0), 0);
        lv_obj_set_style_text_font(name, &lv_font_montserrat_14, 0);

        s_fx_param_sliders[i] = lv_slider_create(s_fx_param_rows[i]);
        style_h_slider(s_fx_param_sliders[i], 0, 100);
        lv_obj_set_width(s_fx_param_sliders[i], 240);
        lv_obj_add_event_cb(s_fx_param_sliders[i], fx_param_slider_event_cb, LV_EVENT_ALL,
                            (void *)(intptr_t)i);
        lv_obj_add_state(s_fx_param_sliders[i], LV_STATE_DISABLED);

        s_fx_param_values[i] = lv_label_create(s_fx_param_rows[i]);
        lv_label_set_text(s_fx_param_values[i], "—");
        lv_obj_set_width(s_fx_param_values[i], 72);
        lv_obj_set_style_text_color(s_fx_param_values[i], lv_color_hex(0xFFFFFF), 0);
        lv_obj_set_style_text_font(s_fx_param_values[i], &lv_font_montserrat_14, 0);
    }
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
    lv_obj_t *gate_tab = lv_tabview_add_tab(tabview, "GATE");
    lv_obj_t *fx_tab = lv_tabview_add_tab(tabview, "FX");

    build_amp_panel(amp_tab);
    build_tuner_panel(tuner_tab);
    build_gate_panel(gate_tab);
    build_fx_panel(fx_tab);
    ui_init_dispatch();
    apply_tuner_locked(nano_tuner_get());
    apply_fx_locked(nano_fx_get());
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

    ESP_LOGI(TAG, "UI ready (800x480, AMP + TUNER + GATE + FX tabs)");
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
    s_ui_dispatch_mask |= UI_DISP_AMP;
    ui_request_dispatch();
}

void nano_ui_set_ble_connected(bool connected)
{
    s_async_ble_connected = connected;
    s_async_ble_pending = true;
    s_ui_dispatch_mask |= UI_DISP_BLE;
    ui_request_dispatch();
}

void nano_ui_set_tuner(const nano_tuner_t *tuner)
{
    if (!tuner || !s_tuner_note) {
        return;
    }
    s_async_tuner = *tuner;
    s_async_tuner_pending = true;
    s_ui_dispatch_mask |= UI_DISP_TUNER;
    ui_request_dispatch();
}

void nano_ui_set_fx(const nano_fx_state_t *fx)
{
    if (fx) {
        s_async_fx = *fx;
        s_async_fx_pending = true;
    } else {
        s_async_fx_pending = false;
        memset(&s_async_fx, 0, sizeof(s_async_fx));
    }
    s_ui_dispatch_mask |= UI_DISP_FX;
    ui_request_dispatch();
}
