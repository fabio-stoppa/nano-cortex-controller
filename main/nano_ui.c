#include "nano_ui.h"

#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "bsp/board.h"
#include "bsp/lvgl_port.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "lvgl.h"

#include "nano_ble.h"
#include "nano_amp.h"
#include "nano_proto.h"
#include "nano_state.h"
#include "nano_tuner.h"
#include "nano_fx.h"
#include "nano_fonts.h"
#include "ui_icons.h"

static const char *TAG = "nano_ui";

#define KNOB_COUNT 5
#define DRAG_WRITE_INTERVAL_US (60 * 1000)

#define FX_MODEL_PICKER_COLS  3
#define FX_MODEL_PICKER_GAP   8
#define FX_MODEL_PICKER_PAD   12
#define FX_MODEL_PICKER_TILE_H 64
#define FX_MODEL_PICKER_SCROLL_PAD 16

#define UI_DISP_AMP   (1u << 0)
#define UI_DISP_BLE   (1u << 1)
#define UI_DISP_TUNER (1u << 2)
#define UI_DISP_FX    (1u << 3)
#define UI_DISP_BPM     (1u << 4)
#define UI_DISP_PRESET  (1u << 5)

#define CHAIN_BLOCK_COUNT 6
#define UI_EDGE_PAD       1
#define HEADER_H          48
#define HEADER_DD_H       (HEADER_H - 4)
#define HEADER_RIGHT_PAD  4
#define CHAIN_RAIL_PAD    4
#define CHAIN_TAB_GAP     8
#define CHAIN_RAIL_BOTTOM_PAD 4
#define CONTENT_CHAIN_GAP   4
#define CHAIN_TAB_BORDER_W 1
#define CHAIN_TAB_BORDER_SEL 2

/* Set to 1: chain tabs in a left rail; 0: bottom horizontal bar */
#define NANO_UI_CHAIN_LEFT 1

#if NANO_UI_CHAIN_LEFT
#define CHAIN_RAIL_W      112
#define CHAIN_TAB_W       104
#define CHAIN_TAB_H       64
#define CHAIN_BTN_W       100
#define CHAIN_BTN_INSET   4
#define CHAIN_ICON_W      94
#else
#define CHAIN_BAR_H       80
#define CHAIN_TAB_W       118
#define CHAIN_TAB_H       76
#define CHAIN_BTN_W       114
#define CHAIN_BTN_H       72
#define CHAIN_ICON_W      108
#endif

typedef enum {
    CHAIN_PRE1 = 0,
    CHAIN_PRE2,
    CHAIN_AMP,
    CHAIN_PFX1,
    CHAIN_PFX2,
    CHAIN_PFX3,
} chain_slot_t;

typedef enum {
    UI_ICON_GATE = 0,
    UI_ICON_AMP,
    UI_ICON_GUITAR_OD,
    UI_ICON_BASS_OD,
    UI_ICON_COMPRESSOR,
    UI_ICON_PITCH,
    UI_ICON_FILTER,
    UI_ICON_WAH,
    UI_ICON_UTILITY,
} ui_icon_kind_t;

#define CHAIN_LABEL_MAX 12

typedef struct {
    lv_obj_t *col;
    lv_obj_t *btn;
    lv_obj_t *icon;
    char icon_text[CHAIN_LABEL_MAX];
    lv_color_t icon_color;
    bool last_active;
    bool last_selected;
    lv_color_t last_accent;
} chain_block_ui_t;

typedef struct {
    uint8_t wake;
} ui_wake_t;

#define PARAM_GRID_COLS   4
#define PARAM_GRID_ROWS   4
#define PEDAL_PARAM_GRID_VISIBLE (PARAM_GRID_COLS * PARAM_GRID_ROWS)
#define AMP_PEDAL_PARAM_COUNT 7
#define PARAM_GRID_GAP    4
#define PARAM_GRID_PAD    2
#define PARAM_CELL_PAD    4
#define PARAM_CELL_ROW_GAP 7
#define PARAM_LABEL_MAX_LEN 11
#define PARAM_SLIDER_INSET_H 14
#define PARAM_GESTURE_SETTLE_MS 150
#define PARAM_GESTURE_MOVE_THRESH 12
#define PARAM_GESTURE_SCROLL_MIN_DY 8
#define PARAM_GESTURE_TAP_THRESH 10
#define PARAM_GRID_SCROLLBAR_W 8
#define PARAM_CELL_AMP_TAG  0x100
#define PARAM_CELL_GATE_TAG 0x200
#define PARAM_CELL_GATE_ON_TAG 0x400
#define UI_FONT_CELL      UI_FONT_MD
#define TUNER_METER_W     560
#define TUNER_METER_H     36
#define TUNER_NEEDLE_W    8
#define TUNER_CENTS_SPAN  50.f
#define TUNER_IN_TUNE_CENTS 2.f
#define TUNER_COLOR_OK    0x3DCC7A
#define TUNER_COLOR_IDLE  0x606060
#define TUNER_COLOR_NEEDLE 0xFFFFFF

typedef struct {
    const char *label;
    lv_obj_t *slider;
    lv_obj_t *value;
} amp_knob_ui_t;

typedef struct {
    lv_obj_t *col;
    lv_obj_t *name;
    lv_obj_t *slider;
    lv_obj_t *value;
} gate_cell_ui_t;

typedef struct {
    lv_obj_t *col;
    lv_obj_t *name;
    lv_obj_t *slider;
    lv_obj_t *value;
} fx_param_ui_t;

static lv_obj_t *s_ble_icon;
static lv_obj_t *s_bpm_lbl;
static lv_obj_t *s_bpm_dot;
static lv_timer_t *s_ble_icon_timer;
static lv_timer_t *s_bpm_beat_timer;
static bool s_ble_blink_on;
static bool s_bpm_beat_lit;
static uint16_t s_bpm_beat_bpm;
static lv_obj_t *s_tuner_note;
static lv_obj_t *s_tuner_cents;
static lv_obj_t *s_tuner_state;
static lv_obj_t *s_tuner_btn;
static lv_obj_t *s_tuner_meter;
static lv_obj_t *s_tuner_meter_center;
static lv_obj_t *s_tuner_meter_needle;
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

static gate_cell_ui_t s_gate_on;
static gate_cell_ui_t s_gate;
static int s_gate_on_drag;
static int s_gate_on_last_sent = -1;
static int64_t s_gate_on_last_send_us;
static int s_gate_drag;
static int s_gate_last_sent = -1;
static int64_t s_gate_last_send_us;
static int s_chain_suppress_click = -1;

static lv_obj_t *s_header_bar;
static lv_obj_t *s_preset_lbl;
static lv_obj_t *s_chain_bar;
static lv_obj_t *s_content_root;
static lv_coord_t s_content_w_normal;
static lv_coord_t s_content_x_normal;
static lv_coord_t s_body_h;
static lv_obj_t *s_amp_panel;
static lv_obj_t *s_fx_panel;
static lv_obj_t *s_tuner_panel;
static lv_obj_t *s_tuner_hdr_btn;
static bool s_tuner_view;

static chain_block_ui_t s_chain[CHAIN_BLOCK_COUNT];
static chain_slot_t s_chain_selected = CHAIN_AMP;

static lv_obj_t *s_fx_model_btn;
static lv_obj_t *s_fx_model_btn_lbl;
static lv_obj_t *s_fx_model_picker;
static lv_obj_t *s_fx_model_picker_grid;
static bool s_fx_model_picker_open;
typedef enum {
    HEADER_PICKER_FX = 0,
    HEADER_PICKER_AMP,
} header_picker_mode_t;
static header_picker_mode_t s_header_picker_mode;
static lv_style_t s_fx_model_btn_style;
static bool s_fx_model_btn_style_inited;
static lv_obj_t *s_amp_params_row;
static lv_obj_t *s_fx_params_row;
static lv_timer_t *s_fx_build_timer;
static lv_coord_t s_param_cell_w;
static lv_coord_t s_param_cell_h;
static fx_param_ui_t s_fx_params[NANO_FX_MAX_PARAMS];
static nano_fx_block_id_t s_fx_selected = NANO_FX_PRE1;
static nano_fx_block_id_t s_fx_scroll_anchor = NANO_FX_PRE1;
static bool s_fx_ui_sync;
static int s_fx_drag_param = -1;
static int s_fx_last_sent[NANO_FX_MAX_PARAMS];
static int64_t s_fx_last_send_us[NANO_FX_MAX_PARAMS];

static nano_fx_state_t s_async_fx;
static bool s_async_fx_pending;
static uint16_t s_async_bpm;
static bool s_async_bpm_valid;
static bool s_async_bpm_pending;
static bool s_async_bpm_rephase;
static uint16_t s_ui_bpm_cache;
static bool s_ui_bpm_cache_valid;
static char s_async_preset[NANO_PRESET_NAME_MAX];
static char s_ui_preset_cache[NANO_PRESET_NAME_MAX];
static bool s_ui_preset_cache_valid;
static bool s_async_preset_pending;
static nano_amp_t s_ui_amp_cache;
static bool s_ui_amp_cache_valid;
static nano_fx_state_t s_ui_fx_cache;
static bool s_ui_fx_cache_valid;
static lv_obj_t *s_pedal_params_scroll_row;
static bool s_pedal_params_scroll_on;
static bool s_pedal_params_scroll_inited;
static bool s_gate_style_valid;
static bool s_gate_style_on;

typedef enum {
    PARAM_GESTURE_NONE = 0,
    PARAM_GESTURE_PENDING,
    PARAM_GESTURE_SCROLL,
    PARAM_GESTURE_ADJUST,
} param_gesture_mode_t;

typedef struct {
    bool active;
    param_gesture_mode_t mode;
    lv_point_t origin;
    int64_t press_us;
} param_gesture_t;

static param_gesture_t s_param_gesture;

static QueueHandle_t s_ui_wake_queue;
static lv_timer_t *s_ui_dispatch_timer;
static volatile uint8_t s_ui_dispatch_mask;

/** Keep LVGL awake for live tuner (~12.5 Hz pitch → paint a bit faster). */
#define UI_DISPATCH_PERIOD_MS 8

static SemaphoreHandle_t s_ui_build_done;

static void ui_scroll_container(lv_obj_t *obj, lv_dir_t dir);
static void build_fx_param_cell(int idx);
static void ensure_fx_params_up_to(int count);
static bool fx_panel_is_visible(void);
static void ui_build_timer_cb(lv_timer_t *timer);
static void apply_amp_locked(const nano_amp_t *amp);
static void apply_fx_locked(const nano_fx_state_t *fx);
static void set_slider_enabled_locked(bool enabled);
static void fx_model_picker_close(void);
static void fx_model_picker_show(void);
static void build_fx_model_picker(lv_obj_t *screen);
static void update_pedal_params_grid_scroll(lv_obj_t *row, uint8_t param_count);
static lv_obj_t *create_pedal_params_grid(lv_obj_t *panel);
static lv_obj_t *pedal_params_row_active(void);
static bool pedal_params_can_scroll(void);
static void style_grid_scrollbar(lv_obj_t *obj, bool active);
static void fx_model_picker_update_scrollbar(void);
static void ui_request_dispatch(void);
static void ui_init_dispatch(void);
static void async_apply_amp_cb(void *arg);
static void async_apply_tuner_cb(void *arg);
static void async_apply_ble_cb(void *arg);
static void async_apply_preset_cb(void *arg);
static void async_apply_bpm_cb(void *arg);
static void async_apply_fx_cb(void *arg);
static void show_content_for_chain(chain_slot_t slot);
static void show_tuner_view(bool on);
static void update_chain_blocks_locked(const nano_amp_t *amp, const nano_fx_state_t *fx);
static ui_icon_kind_t category_to_icon(nano_fx_category_t cat);
static lv_color_t icon_kind_color(ui_icon_kind_t kind);
static void chain_style_block(chain_block_ui_t *block, bool selected, bool active, lv_color_t accent);
static void ui_passive_container(lv_obj_t *obj);
static void ui_no_scroll_only(lv_obj_t *obj);
static void ui_bring_chrome_to_front(void);
static lv_coord_t calc_param_cell_width(lv_coord_t area_w, int cols);
static lv_coord_t calc_param_cell_height(lv_coord_t area_h, int rows);
static lv_coord_t param_slider_width(void);
static void style_param_grid_container(lv_obj_t *obj);
static void sync_header_model_locked(const nano_fx_state_t *fx);
static void refresh_preset_label_locked(void);
static void style_fx_model_header_btn(lv_obj_t *btn);
static void try_send_knob_locked(int idx, uint8_t raw, bool force);
static void param_cell_event_cb(lv_event_t *e);
static void apply_fx_param_locked(int i, const nano_fx_block_t *block);
static void ui_set_label_if_changed(lv_obj_t *lbl, const char *txt);
static void ui_set_slider_if_changed(lv_obj_t *slider, int val);
static void paint_tuner_meter_locked(float cents_f, bool has_pitch);
static void layout_chrome_for_tuner(bool tuner_on);

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

static void paint_tuner_meter_locked(float cents_f, bool has_pitch)
{
    if (!s_tuner_meter || !s_tuner_meter_needle) {
        return;
    }

    float c = cents_f;
    if (c > TUNER_CENTS_SPAN) {
        c = TUNER_CENTS_SPAN;
    } else if (c < -TUNER_CENTS_SPAN) {
        c = -TUNER_CENTS_SPAN;
    }

    bool in_tune = has_pitch && fabsf(c) <= TUNER_IN_TUNE_CENTS;
    lv_coord_t x;
    lv_color_t color;
    if (!has_pitch) {
        color = lv_color_hex(TUNER_COLOR_IDLE);
        x = (TUNER_METER_W - TUNER_NEEDLE_W) / 2;
    } else {
        float t = (c + TUNER_CENTS_SPAN) / (2.f * TUNER_CENTS_SPAN);
        lv_coord_t travel = TUNER_METER_W - TUNER_NEEDLE_W;
        x = (lv_coord_t)lroundf(t * (float)travel);
        color = lv_color_hex(in_tune ? TUNER_COLOR_OK : TUNER_COLOR_NEEDLE);
    }

    if (lv_obj_get_x(s_tuner_meter_needle) != x) {
        lv_obj_set_x(s_tuner_meter_needle, x);
    }
    lv_obj_set_style_bg_color(s_tuner_meter_needle, color, 0);
    if (s_tuner_meter_center) {
        lv_obj_set_style_bg_opa(s_tuner_meter_center, in_tune ? LV_OPA_80 : LV_OPA_30, 0);
    }
    if (s_tuner_note) {
        lv_obj_set_style_text_color(s_tuner_note,
                                    lv_color_hex(in_tune ? TUNER_COLOR_OK : 0xFFFFFF), 0);
    }
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
        ui_set_label_if_changed(s_tuner_note, "-");
        lv_obj_set_style_text_color(s_tuner_note, lv_color_hex(0xFFFFFF), 0);
        ui_set_label_if_changed(s_tuner_cents, "Tuner off");
        paint_tuner_meter_locked(0.f, false);
        lv_label_set_text(lv_obj_get_child(s_tuner_btn, 0), "ON");
        return;
    }

    if (tuner->has_pitch) {
        char note_buf[8];
        note_label(note_buf, sizeof(note_buf), tuner->note);
        ui_set_label_if_changed(s_tuner_note, note_buf);

        float cf = nano_tuner_cents_f();
        char cents_buf[24];
        snprintf(cents_buf, sizeof(cents_buf), "%+.1f cents", cf);
        ui_set_label_if_changed(s_tuner_cents, cents_buf);
        paint_tuner_meter_locked(cf, true);
    } else {
        ui_set_label_if_changed(s_tuner_note, "...");
        lv_obj_set_style_text_color(s_tuner_note, lv_color_hex(0xFFFFFF), 0);
        ui_set_label_if_changed(s_tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.f, false);
    }
    lv_label_set_text(lv_obj_get_child(s_tuner_btn, 0), "OFF");
}

static void async_apply_amp_cb(void *arg)
{
    (void)arg;
    apply_amp_locked(s_async_amp_have ? &s_async_amp : NULL);
}

static void async_apply_tuner_cb(void *arg)
{
    (void)arg;
    /* Mode on/off still arrives via snapshot; pitch is sampled live below. */
    if (s_async_tuner_pending) {
        nano_tuner_t t = s_async_tuner;
        s_async_tuner_pending = false;
        apply_tuner_locked(&t);
        if (t.active && !s_tuner_view) {
            show_tuner_view(true);
        } else if (!t.active && s_tuner_view) {
            show_tuner_view(false);
        }
    }
}

static void paint_tuner_live_locked(void)
{
    if (!s_tuner_note || !s_tuner_view) {
        return;
    }

    nano_tuner_t t;
    nano_tuner_snapshot(&t);
    if (!t.active) {
        return;
    }

    if (t.has_pitch) {
        char note_buf[8];
        note_label(note_buf, sizeof(note_buf), t.note);
        ui_set_label_if_changed(s_tuner_note, note_buf);

        float cf = nano_tuner_cents_f();
        char cents_buf[24];
        snprintf(cents_buf, sizeof(cents_buf), "%+.1f cents", cf);
        ui_set_label_if_changed(s_tuner_cents, cents_buf);
        paint_tuner_meter_locked(cf, true);
    } else {
        ui_set_label_if_changed(s_tuner_note, "...");
        lv_obj_set_style_text_color(s_tuner_note, lv_color_hex(0xFFFFFF), 0);
        ui_set_label_if_changed(s_tuner_cents, "Listening...");
        paint_tuner_meter_locked(0.f, false);
    }
}

static void refresh_ble_icon_locked(void)
{
    if (!s_ble_icon) {
        return;
    }

    if (s_ble_connected) {
        lv_obj_set_style_opa(s_ble_icon, LV_OPA_COVER, 0);
        lv_obj_set_style_text_color(s_ble_icon, lv_color_hex(0x80C0FF), 0);
        return;
    }

    if (nano_ble_scanning()) {
        s_ble_blink_on = !s_ble_blink_on;
        lv_obj_set_style_opa(s_ble_icon, s_ble_blink_on ? LV_OPA_COVER : LV_OPA_30, 0);
        lv_obj_set_style_text_color(s_ble_icon, lv_color_hex(0x80C0FF), 0);
        return;
    }

    lv_obj_set_style_opa(s_ble_icon, LV_OPA_30, 0);
    lv_obj_set_style_text_color(s_ble_icon, lv_color_hex(0x505050), 0);
}

static void ble_icon_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    refresh_ble_icon_locked();
}

static void async_apply_ble_cb(void *arg)
{
    (void)arg;
    if (!s_async_ble_pending) {
        return;
    }
    s_async_ble_pending = false;
    s_ble_connected = s_async_ble_connected;
    refresh_ble_icon_locked();
    set_slider_enabled_locked(s_ble_connected);
    if (s_async_tuner_pending) {
        apply_tuner_locked(&s_async_tuner);
    } else {
        apply_tuner_locked(nano_tuner_get());
    }
    apply_fx_locked(s_async_fx_pending ? &s_async_fx : nano_fx_get());
}

static void ui_run_pending_updates(uint8_t mask)
{
    if ((mask & UI_DISP_BLE) && s_async_ble_pending) {
        async_apply_ble_cb(NULL);
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
    if (mask & UI_DISP_BPM) {
        async_apply_bpm_cb(NULL);
    }
    if (mask & UI_DISP_PRESET) {
        async_apply_preset_cb(NULL);
    }
}

void nano_ui_dispatch_if_pending(void)
{
    if (!s_ui_wake_queue) {
        return;
    }

    ui_wake_t wake;
    bool have_wake = xQueueReceive(s_ui_wake_queue, &wake, 0) == pdTRUE;
    if (have_wake) {
        while (xQueueReceive(s_ui_wake_queue, &wake, 0) == pdTRUE) {
        }
    }

    uint8_t mask = s_ui_dispatch_mask;
    if (mask == 0) {
        /* Wake can be lost when the queue is full; still paint hot pending snapshots. */
        if (s_async_tuner_pending) {
            mask = UI_DISP_TUNER;
        } else if (s_async_ble_pending) {
            mask = UI_DISP_BLE;
        } else if (s_async_fx_pending) {
            mask = UI_DISP_FX;
        } else if (s_async_bpm_pending) {
            mask = UI_DISP_BPM;
        } else if (s_async_preset_pending) {
            mask = UI_DISP_PRESET;
        } else {
            return;
        }
    }

    s_ui_dispatch_mask = 0;
    ui_run_pending_updates(mask);
}

static void ui_dispatch_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    nano_ui_dispatch_if_pending();

    nano_tuner_t t;
    nano_tuner_snapshot(&t);
    if (t.active && !s_tuner_view) {
        show_tuner_view(true);
        apply_tuner_locked(&t);
    } else if (!t.active && s_tuner_view) {
        show_tuner_view(false);
        apply_tuner_locked(&t);
    }

    paint_tuner_live_locked();
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
    if (!s_ui_wake_queue) {
        s_ui_wake_queue = xQueueCreate(16, sizeof(ui_wake_t));
        if (!s_ui_wake_queue) {
            ESP_LOGE(TAG, "UI wake queue create failed");
        }
    }
    if (!s_ui_dispatch_timer) {
        s_ui_dispatch_timer = lv_timer_create(ui_dispatch_timer_cb, UI_DISPATCH_PERIOD_MS, NULL);
        if (!s_ui_dispatch_timer) {
            ESP_LOGE(TAG, "UI dispatch timer create failed");
        }
    }
}

static void async_apply_fx_cb(void *arg)
{
    (void)arg;
    if (!s_async_fx_pending) {
        return;
    }
    if (s_fx_model_picker_open) {
        return;
    }
    apply_fx_locked(&s_async_fx);
    s_async_fx_pending = false;
}

#define BPM_BEAT_FLASH_MS 100u
#define BPM_DOT_SIZE 14
#define BPM_DOT_COLOR 0x00FF66
#define BPM_DOT_GAP 8

static uint32_t bpm_beat_period_ms(uint16_t bpm)
{
    if (bpm < 20) {
        bpm = 20;
    } else if (bpm > 400) {
        bpm = 400;
    }
    uint32_t ms = 60000u / (uint32_t)bpm;
    if (ms < 150u) {
        ms = 150u;
    }
    return ms;
}

static void bpm_beat_set_lit(bool lit)
{
    if (!s_bpm_dot) {
        return;
    }
    s_bpm_beat_lit = lit;
    lv_obj_set_style_bg_opa(s_bpm_dot, lit ? LV_OPA_COVER : LV_OPA_20, 0);
}

static void bpm_beat_timer_cb(lv_timer_t *timer)
{
    (void)timer;
    if (!s_bpm_dot || !s_ui_bpm_cache_valid || s_bpm_beat_bpm < 20) {
        return;
    }

    if (!s_bpm_beat_lit) {
        bpm_beat_set_lit(true);
        lv_timer_set_period(s_bpm_beat_timer, BPM_BEAT_FLASH_MS);
        return;
    }

    bpm_beat_set_lit(false);
    uint32_t beat_ms = bpm_beat_period_ms(s_bpm_beat_bpm);
    uint32_t rest = (beat_ms > BPM_BEAT_FLASH_MS) ? (beat_ms - BPM_BEAT_FLASH_MS) : beat_ms;
    lv_timer_set_period(s_bpm_beat_timer, rest);
}

static void bpm_beat_sync_locked(uint16_t bpm, bool valid)
{
    if (!s_bpm_dot) {
        return;
    }

    if (!valid || bpm < 20) {
        s_bpm_beat_bpm = 0;
        bpm_beat_set_lit(false);
        lv_obj_add_flag(s_bpm_dot, LV_OBJ_FLAG_HIDDEN);
        if (s_bpm_beat_timer) {
            lv_timer_pause(s_bpm_beat_timer);
        }
        return;
    }

    s_bpm_beat_bpm = bpm;
    lv_obj_clear_flag(s_bpm_dot, LV_OBJ_FLAG_HIDDEN);

    if (!s_bpm_beat_timer) {
        s_bpm_beat_timer = lv_timer_create(bpm_beat_timer_cb, BPM_BEAT_FLASH_MS, NULL);
    }

    /* Downbeat now — phase-lock to tap / tempo change. */
    bpm_beat_set_lit(true);
    lv_timer_set_period(s_bpm_beat_timer, BPM_BEAT_FLASH_MS);
    lv_timer_reset(s_bpm_beat_timer);
    lv_timer_resume(s_bpm_beat_timer);
}

static void apply_bpm_locked(uint16_t bpm, bool valid)
{
    if (!s_bpm_lbl) {
        s_async_bpm_rephase = false;
        return;
    }

    bool rephase = s_async_bpm_rephase;
    s_async_bpm_rephase = false;

    char buf[16];
    if (!valid) {
        ui_set_label_if_changed(s_bpm_lbl, "- BPM");
        bpm_beat_sync_locked(0, false);
        return;
    }

    snprintf(buf, sizeof(buf), "%u BPM", (unsigned)bpm);
    ui_set_label_if_changed(s_bpm_lbl, buf);

    if (rephase || s_bpm_beat_bpm != bpm || !s_bpm_beat_timer) {
        bpm_beat_sync_locked(bpm, true);
    }
}

static void refresh_preset_label_locked(void)
{
    if (!s_preset_lbl) {
        return;
    }

    char text[NANO_PRESET_NAME_MAX + 32];
    const char *preset = (s_ui_preset_cache_valid && s_ui_preset_cache[0]) ? s_ui_preset_cache : NULL;

    char bank_line[28];
    bank_line[0] = '\0';
    const nano_amp_t *amp = nano_state_get_amp();
    if (amp && amp->have_capture_slot && amp->capture_slot >= 1 &&
        amp->capture_slot <= NANO_AMP_CAPTURE_COUNT) {
        nano_amp_format_bank_line(amp->capture_slot, bank_line, sizeof(bank_line));
    }

    if (bank_line[0] && preset) {
        snprintf(text, sizeof(text), "%s / %s", bank_line, preset);
    } else if (bank_line[0]) {
        strncpy(text, bank_line, sizeof(text) - 1);
        text[sizeof(text) - 1] = '\0';
    } else if (preset) {
        strncpy(text, preset, sizeof(text) - 1);
        text[sizeof(text) - 1] = '\0';
    } else if (s_tuner_view) {
        strncpy(text, "Tuner", sizeof(text) - 1);
        text[sizeof(text) - 1] = '\0';
    } else {
        strncpy(text, "-", sizeof(text) - 1);
        text[sizeof(text) - 1] = '\0';
    }

    ui_set_label_if_changed(s_preset_lbl, text);
}

static void apply_preset_locked(const char *name)
{
    (void)name;
    refresh_preset_label_locked();
}

static void async_apply_preset_cb(void *arg)
{
    (void)arg;
    if (!s_async_preset_pending) {
        return;
    }
    s_async_preset_pending = false;
    apply_preset_locked(s_async_preset);
}

static void async_apply_bpm_cb(void *arg)
{
    (void)arg;
    if (!s_async_bpm_pending) {
        return;
    }
    s_async_bpm_pending = false;
    apply_bpm_locked(s_async_bpm, s_async_bpm_valid);
}

static ui_icon_kind_t category_to_icon(nano_fx_category_t cat)
{
    switch (cat) {
    case NANO_FX_CAT_GUITAR_OD:
        return UI_ICON_GUITAR_OD;
    case NANO_FX_CAT_BASS_OD:
        return UI_ICON_BASS_OD;
    case NANO_FX_CAT_COMPRESSOR:
        return UI_ICON_COMPRESSOR;
    case NANO_FX_CAT_PITCH:
        return UI_ICON_PITCH;
    case NANO_FX_CAT_FILTER:
        return UI_ICON_FILTER;
    case NANO_FX_CAT_WAH:
        return UI_ICON_WAH;
    case NANO_FX_CAT_UTILITY:
        return UI_ICON_UTILITY;
    case NANO_FX_CAT_MODULATION:
    case NANO_FX_CAT_DELAY:
    case NANO_FX_CAT_REVERB:
        return UI_ICON_UTILITY;
    default:
        return UI_ICON_UTILITY;
    }
}

static lv_color_t icon_kind_color(ui_icon_kind_t kind)
{
    switch (kind) {
    case UI_ICON_GATE:
        return lv_color_hex(0xB0B8C0);
    case UI_ICON_AMP:
        return lv_color_hex(0xE85040);
    case UI_ICON_GUITAR_OD:
        return lv_color_hex(0xE87030);
    case UI_ICON_BASS_OD:
        return lv_color_hex(0xC87828);
    case UI_ICON_COMPRESSOR:
        return lv_color_hex(0x5090E8);
    case UI_ICON_PITCH:
        return lv_color_hex(0x40C8E0);
    case UI_ICON_FILTER:
        return lv_color_hex(0xE0C040);
    case UI_ICON_WAH:
        return lv_color_hex(0xA868E8);
    case UI_ICON_UTILITY:
    default:
        return lv_color_hex(0x909090);
    }
}

static void ui_passive_container(lv_obj_t *obj)
{
    if (!obj) {
        return;
    }
    lv_obj_remove_flag(obj, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_SCROLL_CHAIN | LV_OBJ_FLAG_GESTURE_BUBBLE);
}

static void ui_scroll_container(lv_obj_t *obj, lv_dir_t dir)
{
    if (!obj) {
        return;
    }
    lv_obj_add_flag(obj, LV_OBJ_FLAG_SCROLLABLE | LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_scroll_dir(obj, dir);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLL_CHAIN);
    lv_obj_add_flag(obj, LV_OBJ_FLAG_GESTURE_BUBBLE);
}

static void ui_param_cell_container(lv_obj_t *obj)
{
    if (!obj) {
        return;
    }
    lv_obj_add_flag(obj, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(obj, LV_OBJ_FLAG_SCROLL_CHAIN_VER | LV_OBJ_FLAG_GESTURE_BUBBLE);
}

static void ui_no_scroll_only(lv_obj_t *obj)
{
    if (!obj) {
        return;
    }
    lv_obj_clear_flag(obj, LV_OBJ_FLAG_SCROLLABLE);
}

static void ui_bring_chrome_to_front(void)
{
    if (s_chain_bar) {
        lv_obj_move_foreground(s_chain_bar);
    }
    if (s_fx_model_picker_open && s_fx_model_picker) {
        lv_obj_move_foreground(s_fx_model_picker);
    }
    if (s_header_bar) {
        lv_obj_move_foreground(s_header_bar);
    }
}

static lv_obj_t *make_content_panel(lv_obj_t *parent)
{
    lv_obj_t *panel = lv_obj_create(parent);
    lv_obj_set_size(panel, LV_PCT(100), LV_PCT(100));
    lv_obj_align(panel, LV_ALIGN_TOP_MID, 0, 0);
    lv_obj_set_style_bg_opa(panel, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(panel, 0, 0);
    lv_obj_set_style_pad_all(panel, 0, 0);
    ui_passive_container(panel);
    return panel;
}

static const char *icon_kind_glyph(ui_icon_kind_t kind)
{
    switch (kind) {
    case UI_ICON_GATE:
        return "GATE";
    case UI_ICON_AMP:
        return "AMP";
    case UI_ICON_GUITAR_OD:
        return "GTR OD";
    case UI_ICON_BASS_OD:
        return "BASS";
    case UI_ICON_COMPRESSOR:
        return "COMP";
    case UI_ICON_PITCH:
        return "PITCH";
    case UI_ICON_FILTER:
        return "FILTER";
    case UI_ICON_WAH:
        return "WAH";
    case UI_ICON_UTILITY:
    default:
        return "UTIL";
    }
}

static void chain_set_block_label(chain_block_ui_t *block, const char *text, lv_color_t color)
{
    if (!block || !block->icon || !text) {
        return;
    }

    if (strcmp(block->icon_text, text) == 0 && block->icon_color.red == color.red &&
        block->icon_color.green == color.green && block->icon_color.blue == color.blue) {
        return;
    }

    strncpy(block->icon_text, text, sizeof(block->icon_text) - 1);
    block->icon_text[sizeof(block->icon_text) - 1] = '\0';
    block->icon_color = color;
    lv_label_set_text(block->icon, text);
    lv_obj_set_style_text_font(block->icon, UI_FONT_CHAIN, 0);
    lv_obj_set_style_text_color(block->icon, color, 0);
    lv_obj_set_style_shadow_width(block->icon, 0, 0);
}

static void chain_btn_prepare(lv_obj_t *btn)
{
    lv_obj_remove_style_all(btn);
    lv_obj_remove_theme(btn, LV_PART_ANY | LV_STATE_ANY);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_clear_flag(btn, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_style_bg_opa(btn, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(btn, 0, 0);
    lv_obj_set_style_shadow_width(btn, 0, 0);
    lv_obj_set_style_pad_all(btn, 0, 0);
}

static void chain_style_block(chain_block_ui_t *block, bool selected, bool active, lv_color_t accent)
{
    if (!block || !block->btn) {
        return;
    }

    if (block->last_selected == selected && block->last_active == active &&
        block->last_accent.red == accent.red &&
        block->last_accent.green == accent.green &&
        block->last_accent.blue == accent.blue) {
        return;
    }

    block->last_selected = selected;
    block->last_active = active;
    block->last_accent = accent;

    lv_opa_t opa = selected ? LV_OPA_COVER : (active ? LV_OPA_COVER : LV_OPA_40);
    lv_obj_set_style_opa(block->col, opa, 0);

    lv_color_t outline = active ? accent : lv_color_hex(0x404040);

    if (selected) {
        lv_obj_set_style_bg_opa(block->col, LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_color(block->col, lv_color_hex(0xFFFFFF), 0);
        lv_obj_set_style_border_width(block->col, CHAIN_TAB_BORDER_SEL, 0);
        lv_obj_set_style_border_opa(block->col, LV_OPA_COVER, 0);
        lv_obj_set_style_border_side(block->col, LV_BORDER_SIDE_FULL, 0);
        lv_obj_set_style_border_post(block->col, true, 0);
    } else {
        lv_obj_set_style_bg_opa(block->col, LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_color(block->col, outline, 0);
        lv_obj_set_style_border_width(block->col, CHAIN_TAB_BORDER_W, 0);
        lv_obj_set_style_border_opa(block->col, LV_OPA_COVER, 0);
        lv_obj_set_style_border_side(block->col, LV_BORDER_SIDE_FULL, 0);
        lv_obj_set_style_border_post(block->col, false, 0);
    }

    lv_obj_invalidate(block->col);
}

static nano_fx_block_id_t chain_slot_to_fx_block(chain_slot_t slot)
{
    switch (slot) {
    case CHAIN_PRE1:
        return NANO_FX_PRE1;
    case CHAIN_PRE2:
        return NANO_FX_PRE2;
    case CHAIN_PFX1:
        return NANO_FX_POST1;
    case CHAIN_PFX2:
        return NANO_FX_POST2;
    case CHAIN_PFX3:
        return NANO_FX_POST3;
    default:
        return NANO_FX_PRE1;
    }
}

static bool chain_slot_is_fx(chain_slot_t slot)
{
    return slot == CHAIN_PRE1 || slot == CHAIN_PRE2 || slot == CHAIN_PFX1 ||
           slot == CHAIN_PFX2 || slot == CHAIN_PFX3;
}

static void show_content_for_chain(chain_slot_t slot)
{
    s_tuner_view = false;
    fx_model_picker_close();

    if (s_amp_panel) {
        lv_obj_add_flag(s_amp_panel, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_fx_panel) {
        lv_obj_add_flag(s_fx_panel, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_tuner_panel) {
        lv_obj_add_flag(s_tuner_panel, LV_OBJ_FLAG_HIDDEN);
    }

    lv_obj_t *shown = NULL;
    switch (slot) {
    case CHAIN_AMP:
        shown = s_amp_panel;
        break;
    case CHAIN_PRE1:
    case CHAIN_PRE2:
    case CHAIN_PFX1:
    case CHAIN_PFX2:
    case CHAIN_PFX3:
        shown = s_fx_panel;
        break;
    default:
        break;
    }

    if (shown) {
        lv_obj_remove_flag(shown, LV_OBJ_FLAG_HIDDEN);
        lv_obj_move_foreground(shown);
    }

    ui_bring_chrome_to_front();
    s_pedal_params_scroll_inited = false;
    if (slot == CHAIN_AMP) {
        update_pedal_params_grid_scroll(s_amp_params_row, AMP_PEDAL_PARAM_COUNT);
    } else if (chain_slot_is_fx(slot)) {
        const nano_fx_state_t *fx = nano_fx_get();
        const nano_fx_block_t *block = fx ? &fx->blocks[s_fx_selected] : NULL;
        uint8_t count = block && block->valid ? block->param_count : 0;
        update_pedal_params_grid_scroll(s_fx_params_row, count);
    }
    sync_header_model_locked(nano_fx_get());
}

static void layout_chrome_for_tuner(bool tuner_on)
{
    if (!s_content_root) {
        return;
    }

#if NANO_UI_CHAIN_LEFT
    if (s_chain_bar) {
        if (tuner_on) {
            lv_obj_add_flag(s_chain_bar, LV_OBJ_FLAG_HIDDEN);
        } else {
            lv_obj_remove_flag(s_chain_bar, LV_OBJ_FLAG_HIDDEN);
        }
    }

    if (tuner_on) {
        lv_coord_t full_w = LV_HOR_RES - 2 * UI_EDGE_PAD;
        lv_obj_set_size(s_content_root, full_w, s_body_h > 0 ? s_body_h : lv_obj_get_height(s_content_root));
        lv_obj_align(s_content_root, LV_ALIGN_TOP_LEFT, 0, HEADER_H);
    } else {
        lv_coord_t w = s_content_w_normal > 0 ? s_content_w_normal : lv_obj_get_width(s_content_root);
        lv_coord_t h = s_body_h > 0 ? s_body_h : lv_obj_get_height(s_content_root);
        lv_obj_set_size(s_content_root, w, h);
        lv_obj_align(s_content_root, LV_ALIGN_TOP_LEFT, s_content_x_normal, HEADER_H);
    }
#else
    if (s_chain_bar) {
        if (tuner_on) {
            lv_obj_add_flag(s_chain_bar, LV_OBJ_FLAG_HIDDEN);
        } else {
            lv_obj_remove_flag(s_chain_bar, LV_OBJ_FLAG_HIDDEN);
        }
    }
    (void)tuner_on;
#endif
}

static void show_tuner_view(bool on)
{
    s_tuner_view = on;
    if (on) {
        fx_model_picker_close();
    }
    if (s_tuner_hdr_btn) {
        if (on) {
            lv_obj_add_state(s_tuner_hdr_btn, LV_STATE_CHECKED);
        } else {
            lv_obj_remove_state(s_tuner_hdr_btn, LV_STATE_CHECKED);
        }
    }
    if (s_amp_panel) {
        lv_obj_add_flag(s_amp_panel, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_fx_panel) {
        lv_obj_add_flag(s_fx_panel, LV_OBJ_FLAG_HIDDEN);
    }
    if (s_tuner_panel) {
        if (on) {
            lv_obj_remove_flag(s_tuner_panel, LV_OBJ_FLAG_HIDDEN);
            lv_obj_move_foreground(s_tuner_panel);
            layout_chrome_for_tuner(true);
            sync_header_model_locked(nano_fx_get());
            refresh_preset_label_locked();
            ui_bring_chrome_to_front();
        } else {
            lv_obj_add_flag(s_tuner_panel, LV_OBJ_FLAG_HIDDEN);
            layout_chrome_for_tuner(false);
            show_content_for_chain(s_chain_selected);
        }
    }
}

static void update_chain_blocks_locked(const nano_amp_t *amp, const nano_fx_state_t *fx)
{
    for (int i = 0; i < CHAIN_BLOCK_COUNT; i++) {
        chain_block_ui_t *cb = &s_chain[i];
        if (!cb->btn) {
            continue;
        }

        bool selected = ((chain_slot_t)i == s_chain_selected) && !s_tuner_view;
        bool active = false;
        ui_icon_kind_t icon = UI_ICON_UTILITY;
        lv_color_t accent = lv_color_hex(0x808080);
        char block_label[CHAIN_LABEL_MAX] = "-";

        switch ((chain_slot_t)i) {
        case CHAIN_AMP:
            icon = UI_ICON_AMP;
            accent = icon_kind_color(UI_ICON_AMP);
            active = amp && amp->valid;
            snprintf(block_label, sizeof(block_label), "AMP");
            break;
        case CHAIN_PRE1:
        case CHAIN_PRE2:
        case CHAIN_PFX1:
        case CHAIN_PFX2:
        case CHAIN_PFX3: {
            nano_fx_block_id_t bid = chain_slot_to_fx_block((chain_slot_t)i);
            const nano_fx_block_t *b = fx ? &fx->blocks[bid] : NULL;
            nano_fx_category_t cat = b && b->model ? nano_fx_model_category(b->model) : NANO_FX_CAT_NONE;
            icon = category_to_icon(cat);
            accent = icon_kind_color(icon);
            active = b && b->valid && b->on;
            if (b && b->valid) {
                nano_fx_model_chain_acronym(b, block_label, sizeof(block_label));
            } else {
                snprintf(block_label, sizeof(block_label), "%s", icon_kind_glyph(icon));
            }
            break;
        }
        default:
            break;
        }

        lv_color_t label_color = lv_color_hex(0x505050);
        if (selected) {
            label_color = active ? accent : lv_color_hex(0x909090);
        } else if (active) {
            label_color = accent;
        }
        chain_set_block_label(cb, block_label, label_color);
        chain_style_block(cb, selected, active || (chain_slot_t)i == CHAIN_AMP, accent);
    }
}

static void chain_toggle_slot(chain_slot_t slot)
{
    if (!s_ble_connected) {
        return;
    }

    const nano_fx_state_t *fx = nano_fx_get();

    switch (slot) {
    case CHAIN_PRE1:
    case CHAIN_PRE2:
    case CHAIN_PFX1:
    case CHAIN_PFX2:
    case CHAIN_PFX3: {
        if (!fx) {
            return;
        }
        nano_fx_block_id_t bid = chain_slot_to_fx_block(slot);
        nano_fx_write_block_enabled(bid, !fx->blocks[bid].on);
        break;
    }
    default:
        break;
    }
}

static void chain_block_btn_event_cb(lv_event_t *e)
{
    lv_event_code_t code = lv_event_get_code(e);
    int idx = (int)(intptr_t)lv_event_get_user_data(e);
    if (idx < 0 || idx >= CHAIN_BLOCK_COUNT) {
        return;
    }

    if (code == LV_EVENT_LONG_PRESSED) {
        chain_slot_t slot = (chain_slot_t)idx;
        if (slot == CHAIN_AMP) {
            return;
        }
        s_chain_suppress_click = idx;
        chain_toggle_slot(slot);
        update_chain_blocks_locked(nano_state_get_amp(), nano_fx_get());
        return;
    }

    if (code != LV_EVENT_CLICKED) {
        return;
    }

    if (s_chain_suppress_click == idx) {
        s_chain_suppress_click = -1;
        return;
    }

    chain_slot_t slot = (chain_slot_t)idx;
    s_chain_selected = slot;
    s_tuner_view = false;

    if (chain_slot_is_fx(slot)) {
        s_fx_selected = chain_slot_to_fx_block(slot);
    }

    show_content_for_chain(slot);

    if (chain_slot_is_fx(slot)) {
        apply_fx_locked(nano_fx_get());
    }

    update_chain_blocks_locked(nano_state_get_amp(), nano_fx_get());
}

static void tuner_hdr_btn_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED) {
        return;
    }
    show_tuner_view(!s_tuner_view);
    sync_header_model_locked(nano_fx_get());
    update_chain_blocks_locked(nano_state_get_amp(), nano_fx_get());
}

static void try_send_gate_locked(uint8_t pct, bool force_send)
{
    if (s_fx_ui_sync || !s_ble_connected) {
        return;
    }

    int64_t now = esp_timer_get_time();
    if (!force_send && pct == (uint8_t)s_gate_last_sent &&
        (now - s_gate_last_send_us) < DRAG_WRITE_INTERVAL_US) {
        return;
    }

    if (nano_fx_write_gate_reduction(pct)) {
        s_gate_last_sent = pct;
        s_gate_last_send_us = now;
    }
}

static bool fx_block_matches_model(const nano_fx_block_t *block, const nano_fx_model_def_t *model)
{
    if (!block || !model) {
        return false;
    }
    if (block->model == model) {
        return true;
    }
    return block->has_model_id && model->id_len > 0 &&
           block->model_id_len == model->id_len &&
           memcmp(block->model_id_raw, model->id, model->id_len) == 0;
}

static void fx_model_picker_backdrop_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED) {
        return;
    }
    fx_model_picker_close();
}

static void fx_model_picker_tile_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED) {
        return;
    }

    size_t idx = (size_t)(intptr_t)lv_event_get_user_data(e);
    if (s_ble_connected) {
        if (s_header_picker_mode == HEADER_PICKER_AMP) {
            nano_state_write_capture_slot((uint8_t)idx);
        } else {
            const nano_fx_model_def_t *model = nano_fx_get_allowed_model(s_fx_selected, idx);
            if (model) {
                nano_fx_write_block_model(s_fx_selected, model);
            }
        }
    }
    fx_model_picker_close();
}

static void fx_model_picker_lock_page_scroll(bool lock)
{
    lv_obj_t *screen = lv_screen_active();
    if (screen) {
        if (lock) {
            lv_obj_clear_flag(screen, LV_OBJ_FLAG_SCROLLABLE);
        }
    }
    if (s_content_root) {
        if (lock) {
            lv_obj_clear_flag(s_content_root, LV_OBJ_FLAG_SCROLLABLE);
        }
    }
    if (s_fx_params_row && lock) {
        lv_obj_clear_flag(s_fx_params_row, LV_OBJ_FLAG_SCROLLABLE);
    }
    if (s_amp_params_row && lock) {
        lv_obj_clear_flag(s_amp_params_row, LV_OBJ_FLAG_SCROLLABLE);
    }
}

static void model_picker_add_tile(lv_obj_t *parent, lv_coord_t tile_w, const char *name,
                                  size_t picker_idx, bool selected)
{
    lv_obj_t *tile = lv_button_create(parent);
    lv_obj_set_size(tile, tile_w, FX_MODEL_PICKER_TILE_H);
    lv_obj_set_style_bg_color(tile, selected ? lv_color_hex(0x1A3040) : lv_color_hex(0x141414), 0);
    lv_obj_set_style_bg_opa(tile, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(tile, selected ? lv_color_hex(0xFFFFFF) : lv_color_hex(0x383838), 0);
    lv_obj_set_style_border_width(tile, selected ? 2 : 1, 0);
    lv_obj_set_style_radius(tile, 8, 0);
    lv_obj_set_style_shadow_width(tile, 0, 0);
    lv_obj_add_event_cb(tile, fx_model_picker_tile_event_cb, LV_EVENT_CLICKED,
                        (void *)(intptr_t)picker_idx);

    lv_obj_t *lbl = lv_label_create(tile);
    lv_label_set_text(lbl, name);
    lv_obj_set_width(lbl, tile_w - 16);
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_WRAP);
    lv_obj_set_style_text_font(lbl, UI_FONT_SM, 0);
    lv_obj_set_style_text_color(lbl, selected ? lv_color_hex(0xFFFFFF) : lv_color_hex(0xD8D8D8), 0);
    lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_center(lbl);
}

static void fx_model_picker_add_tile(lv_obj_t *parent, lv_coord_t tile_w,
                                     const nano_fx_model_def_t *model, size_t model_idx,
                                     bool selected)
{
    model_picker_add_tile(parent, tile_w, model->name, model_idx, selected);
}

static void amp_model_picker_populate(void)
{
    if (!s_fx_model_picker_grid) {
        return;
    }

    static const nano_amp_bank_t bank_order[] = {
        NANO_AMP_BANK_CLEAN,
        NANO_AMP_BANK_2,
        NANO_AMP_BANK_3,
        NANO_AMP_BANK_HIGH_GAIN,
        NANO_AMP_BANK_BASS,
    };

    lv_obj_clean(s_fx_model_picker_grid);
    lv_obj_scroll_to(s_fx_model_picker_grid, 0, 0, LV_ANIM_OFF);

    const nano_amp_t *amp = nano_state_get_amp();
    uint8_t selected_slot = (amp && amp->have_capture_slot) ? amp->capture_slot : 0;

    lv_coord_t grid_w = lv_obj_get_content_width(s_fx_model_picker_grid);
    if (grid_w < 80) {
        grid_w = LV_HOR_RES - 4 * FX_MODEL_PICKER_PAD;
    }
    grid_w -= FX_MODEL_PICKER_SCROLL_PAD;
    lv_coord_t gaps = (FX_MODEL_PICKER_COLS - 1) * FX_MODEL_PICKER_GAP;
    lv_coord_t tile_w = (grid_w - gaps) / FX_MODEL_PICKER_COLS;
    if (tile_w < 64) {
        tile_w = 64;
    }

    for (size_t b = 0; b < sizeof(bank_order) / sizeof(bank_order[0]); b++) {
        nano_amp_bank_t bank = bank_order[b];
        bool section_has_models = false;

        for (size_t i = 0; i < nano_amp_catalog_count; i++) {
            if (nano_amp_catalog[i].bank == bank) {
                section_has_models = true;
                break;
            }
        }
        if (!section_has_models) {
            continue;
        }

        lv_obj_t *hdr = lv_label_create(s_fx_model_picker_grid);
        lv_label_set_text(hdr, nano_amp_bank_label(bank));
        lv_obj_set_width(hdr, grid_w);
        lv_obj_set_style_text_font(hdr, UI_FONT_MD, 0);
        lv_obj_set_style_text_color(hdr, lv_color_hex(0x909090), 0);
        lv_obj_set_style_pad_top(hdr, b == 0 ? 0 : 10, 0);
        lv_obj_set_style_pad_bottom(hdr, 4, 0);

        lv_obj_t *tiles = lv_obj_create(s_fx_model_picker_grid);
        lv_obj_set_width(tiles, grid_w);
        lv_obj_set_height(tiles, LV_SIZE_CONTENT);
        lv_obj_set_style_bg_opa(tiles, LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_width(tiles, 0, 0);
        lv_obj_set_style_pad_all(tiles, 0, 0);
        lv_obj_set_style_pad_row(tiles, FX_MODEL_PICKER_GAP, 0);
        lv_obj_set_style_pad_column(tiles, FX_MODEL_PICKER_GAP, 0);
        lv_obj_set_flex_flow(tiles, LV_FLEX_FLOW_ROW_WRAP);
        lv_obj_set_flex_align(tiles, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
        ui_no_scroll_only(tiles);

        for (size_t i = 0; i < nano_amp_catalog_count; i++) {
            const nano_amp_capture_t *capture = &nano_amp_catalog[i];
            if (capture->bank != bank || !capture->name) {
                continue;
            }
            bool selected = selected_slot > 0 && capture->slot == selected_slot;
            model_picker_add_tile(tiles, tile_w, capture->name, capture->slot, selected);
        }
    }

    fx_model_picker_update_scrollbar();
}

static void fx_model_picker_populate(void)
{
    if (!s_fx_model_picker_grid) {
        return;
    }

    static const nano_fx_category_t cat_order[] = {
        NANO_FX_CAT_GUITAR_OD,
        NANO_FX_CAT_BASS_OD,
        NANO_FX_CAT_WAH,
        NANO_FX_CAT_FILTER,
        NANO_FX_CAT_COMPRESSOR,
        NANO_FX_CAT_PITCH,
        NANO_FX_CAT_MODULATION,
        NANO_FX_CAT_DELAY,
        NANO_FX_CAT_REVERB,
        NANO_FX_CAT_UTILITY,
    };

    lv_obj_clean(s_fx_model_picker_grid);
    lv_obj_scroll_to(s_fx_model_picker_grid, 0, 0, LV_ANIM_OFF);

    const nano_fx_state_t *fx = nano_fx_get();
    const nano_fx_block_t *cur = fx ? &fx->blocks[s_fx_selected] : NULL;
    size_t count = nano_fx_allowed_model_count(s_fx_selected);

    lv_coord_t grid_w = lv_obj_get_content_width(s_fx_model_picker_grid);
    if (grid_w < 80) {
        grid_w = LV_HOR_RES - 4 * FX_MODEL_PICKER_PAD;
    }
    grid_w -= FX_MODEL_PICKER_SCROLL_PAD;
    lv_coord_t gaps = (FX_MODEL_PICKER_COLS - 1) * FX_MODEL_PICKER_GAP;
    lv_coord_t tile_w = (grid_w - gaps) / FX_MODEL_PICKER_COLS;
    if (tile_w < 64) {
        tile_w = 64;
    }

    for (size_t c = 0; c < sizeof(cat_order) / sizeof(cat_order[0]); c++) {
        nano_fx_category_t cat = cat_order[c];
        bool section_has_models = false;

        for (size_t i = 0; i < count; i++) {
            const nano_fx_model_def_t *model = nano_fx_get_allowed_model(s_fx_selected, i);
            if (model && model->name && nano_fx_model_category(model) == cat) {
                section_has_models = true;
                break;
            }
        }
        if (!section_has_models) {
            continue;
        }

        lv_obj_t *hdr = lv_label_create(s_fx_model_picker_grid);
        lv_label_set_text(hdr, nano_fx_category_label(cat));
        lv_obj_set_width(hdr, grid_w);
        lv_obj_set_style_text_font(hdr, UI_FONT_MD, 0);
        lv_obj_set_style_text_color(hdr, lv_color_hex(0x909090), 0);
        lv_obj_set_style_pad_top(hdr, c == 0 ? 0 : 10, 0);
        lv_obj_set_style_pad_bottom(hdr, 4, 0);

        lv_obj_t *tiles = lv_obj_create(s_fx_model_picker_grid);
        lv_obj_set_width(tiles, grid_w);
        lv_obj_set_height(tiles, LV_SIZE_CONTENT);
        lv_obj_set_style_bg_opa(tiles, LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_width(tiles, 0, 0);
        lv_obj_set_style_pad_all(tiles, 0, 0);
        lv_obj_set_style_pad_row(tiles, FX_MODEL_PICKER_GAP, 0);
        lv_obj_set_style_pad_column(tiles, FX_MODEL_PICKER_GAP, 0);
        lv_obj_set_flex_flow(tiles, LV_FLEX_FLOW_ROW_WRAP);
        lv_obj_set_flex_align(tiles, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
        ui_no_scroll_only(tiles);

        for (size_t i = 0; i < count; i++) {
            const nano_fx_model_def_t *model = nano_fx_get_allowed_model(s_fx_selected, i);
            if (!model || !model->name || nano_fx_model_category(model) != cat) {
                continue;
            }
            bool selected = fx_block_matches_model(cur, model);
            fx_model_picker_add_tile(tiles, tile_w, model, i, selected);
        }
    }

    fx_model_picker_update_scrollbar();
}

static void fx_model_picker_close(void)
{
    if (!s_fx_model_picker || !s_fx_model_picker_open) {
        return;
    }
    s_fx_model_picker_open = false;
    lv_obj_add_flag(s_fx_model_picker, LV_OBJ_FLAG_HIDDEN);
    fx_model_picker_lock_page_scroll(false);

    const nano_fx_state_t *fx = nano_fx_get();
    const nano_fx_block_t *block = fx ? &fx->blocks[s_fx_selected] : NULL;
    if (block && block->valid && fx_panel_is_visible()) {
        s_pedal_params_scroll_inited = false;
        update_pedal_params_grid_scroll(s_fx_params_row, block->param_count);
    } else if (s_chain_selected == CHAIN_AMP) {
        s_pedal_params_scroll_inited = false;
        update_pedal_params_grid_scroll(s_amp_params_row, AMP_PEDAL_PARAM_COUNT);
    }
}

static void fx_model_picker_show(void)
{
    if (!s_fx_model_picker || !s_fx_model_btn) {
        return;
    }
    if (lv_obj_has_state(s_fx_model_btn, LV_STATE_DISABLED)) {
        return;
    }

    if (s_chain_selected == CHAIN_AMP) {
        s_header_picker_mode = HEADER_PICKER_AMP;
        amp_model_picker_populate();
    } else if (chain_slot_is_fx(s_chain_selected)) {
        s_header_picker_mode = HEADER_PICKER_FX;
        fx_model_picker_populate();
    } else {
        return;
    }

    s_fx_model_picker_open = true;
    fx_model_picker_lock_page_scroll(true);
    lv_obj_remove_flag(s_fx_model_picker, LV_OBJ_FLAG_HIDDEN);
    ui_bring_chrome_to_front();
}

static void fx_model_header_btn_event_cb(lv_event_t *e)
{
    if (lv_event_get_code(e) != LV_EVENT_CLICKED) {
        return;
    }
    if (s_fx_model_picker_open) {
        fx_model_picker_close();
        return;
    }
    fx_model_picker_show();
}

static void build_fx_model_picker(lv_obj_t *screen)
{
    s_fx_model_picker = lv_obj_create(screen);
    lv_obj_set_size(s_fx_model_picker, LV_PCT(100), LV_VER_RES - HEADER_H);
    lv_obj_align(s_fx_model_picker, LV_ALIGN_TOP_MID, 0, HEADER_H);
    lv_obj_set_style_bg_color(s_fx_model_picker, lv_color_hex(0x000000), 0);
    lv_obj_set_style_bg_opa(s_fx_model_picker, LV_OPA_70, 0);
    lv_obj_set_style_border_width(s_fx_model_picker, 0, 0);
    lv_obj_set_style_pad_all(s_fx_model_picker, FX_MODEL_PICKER_PAD, 0);
    lv_obj_add_flag(s_fx_model_picker, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(s_fx_model_picker, fx_model_picker_backdrop_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_flag(s_fx_model_picker, LV_OBJ_FLAG_HIDDEN);
    ui_no_scroll_only(s_fx_model_picker);
    lv_obj_clear_flag(s_fx_model_picker, LV_OBJ_FLAG_SCROLL_CHAIN);

    lv_obj_t *panel = lv_obj_create(s_fx_model_picker);
    lv_obj_set_size(panel, LV_PCT(98), LV_PCT(96));
    lv_obj_center(panel);
    lv_obj_set_style_bg_color(panel, lv_color_hex(0x0A0A0A), 0);
    lv_obj_set_style_bg_opa(panel, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(panel, 1, 0);
    lv_obj_set_style_border_color(panel, lv_color_hex(0x303030), 0);
    lv_obj_set_style_radius(panel, 8, 0);
    lv_obj_set_style_pad_all(panel, FX_MODEL_PICKER_PAD, 0);
    lv_obj_clear_flag(panel, LV_OBJ_FLAG_CLICKABLE);
    ui_no_scroll_only(panel);
    lv_obj_clear_flag(panel, LV_OBJ_FLAG_SCROLL_CHAIN);

    s_fx_model_picker_grid = lv_obj_create(panel);
    lv_obj_set_size(s_fx_model_picker_grid, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(s_fx_model_picker_grid, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(s_fx_model_picker_grid, 0, 0);
    lv_obj_set_style_pad_all(s_fx_model_picker_grid, 0, 0);
    lv_obj_set_style_pad_right(s_fx_model_picker_grid, FX_MODEL_PICKER_SCROLL_PAD, 0);
    lv_obj_set_style_pad_row(s_fx_model_picker_grid, FX_MODEL_PICKER_GAP, 0);
    lv_obj_set_flex_flow(s_fx_model_picker_grid, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(s_fx_model_picker_grid, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START,
                          LV_FLEX_ALIGN_START);
    lv_obj_set_scroll_dir(s_fx_model_picker_grid, LV_DIR_VER);
    lv_obj_add_flag(s_fx_model_picker_grid, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_clear_flag(s_fx_model_picker_grid, LV_OBJ_FLAG_SCROLL_CHAIN);
    style_grid_scrollbar(s_fx_model_picker_grid, false);
}

static void fx_short_param_name(const char *full, char *out, size_t cap)
{
    if (!full || !out || cap == 0) {
        return;
    }

    if (strcmp(full, "Feedback Depth") == 0) {
        snprintf(out, cap, "Fbk Depth");
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
    if (n > (size_t)PARAM_LABEL_MAX_LEN) {
        n = (size_t)PARAM_LABEL_MAX_LEN;
    }
    out[n] = '\0';
    if (n == 0) {
        snprintf(out, cap, "%s", full);
        if (strlen(out) > (size_t)PARAM_LABEL_MAX_LEN) {
            out[PARAM_LABEL_MAX_LEN] = '\0';
        }
    }
}

static lv_coord_t param_label_width(void)
{
    return s_param_cell_w - 2 * PARAM_CELL_PAD;
}

static void style_param_name_label(lv_obj_t *lbl)
{
    lv_obj_set_width(lbl, param_label_width());
    lv_label_set_long_mode(lbl, LV_LABEL_LONG_DOT);
    lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_set_style_text_color(lbl, lv_color_hex(0xB0B0B0), 0);
    lv_obj_set_style_text_font(lbl, UI_FONT_CELL, 0);
}

static void style_param_value_label(lv_obj_t *lbl)
{
    lv_obj_set_width(lbl, param_label_width());
    lv_obj_set_style_text_align(lbl, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_set_style_text_color(lbl, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(lbl, UI_FONT_CELL, 0);
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

static void style_grid_scrollbar(lv_obj_t *obj, bool active)
{
    if (!obj) {
        return;
    }

    lv_obj_set_scrollbar_mode(obj, LV_SCROLLBAR_MODE_ON);
    lv_obj_set_style_width(obj, 6, LV_PART_SCROLLBAR);
    lv_obj_set_style_pad_all(obj, 0, LV_PART_SCROLLBAR);
    lv_obj_set_style_radius(obj, 3, LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_color(obj, lv_color_hex(active ? 0x686868 : 0x252525), LV_PART_SCROLLBAR);
    lv_obj_set_style_bg_opa(obj, active ? LV_OPA_COVER : LV_OPA_30, LV_PART_SCROLLBAR);
}

static bool scroll_area_has_overflow(lv_obj_t *obj)
{
    if (!obj) {
        return false;
    }

    lv_obj_update_layout(obj);
    return lv_obj_get_scroll_bottom(obj) > 0;
}

static void fx_model_picker_update_scrollbar(void)
{
    if (!s_fx_model_picker_grid) {
        return;
    }

    style_grid_scrollbar(s_fx_model_picker_grid, scroll_area_has_overflow(s_fx_model_picker_grid));
}

static void update_pedal_params_grid_scroll(lv_obj_t *row, uint8_t param_count)
{
    if (!row) {
        return;
    }

    bool overflow = param_count > (uint8_t)PEDAL_PARAM_GRID_VISIBLE;
    if (s_pedal_params_scroll_inited && s_pedal_params_scroll_row == row &&
        overflow == s_pedal_params_scroll_on) {
        return;
    }

    s_pedal_params_scroll_row = row;
    s_pedal_params_scroll_on = overflow;
    s_pedal_params_scroll_inited = true;

    ui_scroll_container(row, LV_DIR_VER);
    style_grid_scrollbar(row, overflow);
    if (!overflow) {
        lv_obj_scroll_to(row, 0, 0, LV_ANIM_OFF);
    }
}

static lv_obj_t *pedal_params_row_active(void)
{
    if (s_chain_selected == CHAIN_AMP) {
        return s_amp_params_row;
    }
    if (chain_slot_is_fx(s_chain_selected)) {
        return s_fx_params_row;
    }
    return NULL;
}

static bool pedal_params_can_scroll(void)
{
    lv_obj_t *row = pedal_params_row_active();
    if (!row || !s_pedal_params_scroll_on || s_pedal_params_scroll_row != row) {
        return false;
    }
    return true;
}

static lv_obj_t *create_pedal_params_grid(lv_obj_t *panel)
{
    lv_obj_set_size(panel, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(panel, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(panel, 0, 0);
    lv_obj_set_style_pad_all(panel, 0, 0);
    ui_passive_container(panel);

    lv_obj_t *row = lv_obj_create(panel);
    lv_obj_set_size(row, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(row, 0, 0);
    style_param_grid_container(row);
    ui_scroll_container(row, LV_DIR_VER);
    style_grid_scrollbar(row, false);
    return row;
}

static void ui_set_label_if_changed(lv_obj_t *lbl, const char *txt)
{
    if (!lbl || !txt) {
        return;
    }
    const char *cur = lv_label_get_text(lbl);
    if (cur && strcmp(cur, txt) == 0) {
        return;
    }
    lv_label_set_text(lbl, txt);
}

static void ui_set_slider_if_changed(lv_obj_t *slider, int val)
{
    if (!slider) {
        return;
    }
    if (lv_slider_get_value(slider) == val) {
        return;
    }
    lv_slider_set_value(slider, val, LV_ANIM_OFF);
}

static void style_param_cell(lv_obj_t *cell, lv_coord_t w, lv_coord_t h)
{
    lv_obj_set_size(cell, w, h);
    lv_obj_set_style_bg_color(cell, lv_color_hex(0x1A1A1A), 0);
    lv_obj_set_style_bg_opa(cell, LV_OPA_COVER, 0);
    lv_obj_set_style_border_color(cell, lv_color_hex(0x404040), 0);
    lv_obj_set_style_border_width(cell, 1, 0);
    lv_obj_set_style_radius(cell, 4, 0);
    lv_obj_set_style_pad_all(cell, PARAM_CELL_PAD, 0);
    lv_obj_set_style_pad_row(cell, PARAM_CELL_ROW_GAP, 0);
}

static void style_compact_h_slider(lv_obj_t *slider, lv_coord_t width, int32_t min_v, int32_t max_v)
{
    lv_obj_set_width(slider, width);
    lv_obj_set_height(slider, 18);
    lv_slider_set_range(slider, min_v, max_v);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0x1E1E1E), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(slider, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_border_color(slider, lv_color_hex(0x707070), LV_PART_MAIN);
    lv_obj_set_style_border_width(slider, 1, LV_PART_MAIN);
    lv_obj_set_style_radius(slider, 3, LV_PART_MAIN);
    lv_obj_set_style_bg_color(slider, lv_color_hex(0xB8C0C8), LV_PART_INDICATOR);
    lv_obj_set_style_radius(slider, 2, LV_PART_INDICATOR);
    lv_obj_set_style_pad_all(slider, 0, LV_PART_KNOB);
    lv_obj_set_style_width(slider, 0, LV_PART_KNOB);
    lv_obj_set_style_height(slider, 0, LV_PART_KNOB);
    lv_obj_set_style_bg_opa(slider, LV_OPA_TRANSP, LV_PART_KNOB);
    lv_obj_set_style_border_width(slider, 0, LV_PART_KNOB);
    lv_obj_remove_flag(slider, LV_OBJ_FLAG_CLICKABLE);
}

static int param_cell_x_to_slider(lv_obj_t *cell, lv_obj_t *slider, lv_coord_t abs_x)
{
    lv_area_t area;
    lv_obj_get_coords(cell, &area);
    lv_coord_t rel_x = abs_x - area.x1 - PARAM_CELL_PAD - PARAM_SLIDER_INSET_H;
    lv_coord_t track_w = s_param_cell_w - 2 * PARAM_CELL_PAD - 2 * PARAM_SLIDER_INSET_H;
    (void)slider;
    if (track_w < 1) {
        track_w = 1;
    }
    if (rel_x < 0) {
        rel_x = 0;
    }
    if (rel_x > track_w) {
        rel_x = track_w;
    }

    int32_t min_v = lv_slider_get_min_value(slider);
    int32_t max_v = lv_slider_get_max_value(slider);
    if (max_v <= min_v) {
        return min_v;
    }

    return (int)(min_v + (int32_t)rel_x * (max_v - min_v) / track_w);
}

static int param_cell_value_from_x(lv_obj_t *cell, lv_obj_t *slider, lv_coord_t abs_x, bool binary_half)
{
    if (!binary_half) {
        return param_cell_x_to_slider(cell, slider, abs_x);
    }

    lv_area_t area;
    lv_obj_get_coords(cell, &area);
    lv_coord_t rel_x = abs_x - area.x1;
    lv_coord_t cell_w = lv_area_get_width(&area);
    if (cell_w < 1) {
        cell_w = 1;
    }

    int32_t min_v = lv_slider_get_min_value(slider);
    int32_t max_v = lv_slider_get_max_value(slider);
    return (rel_x >= cell_w / 2) ? max_v : min_v;
}

static void try_send_gate_on_locked(bool on, bool force_send)
{
    int64_t now = esp_timer_get_time();
    int sent = on ? 1 : 0;
    if (!force_send && sent == s_gate_on_last_sent &&
        (now - s_gate_on_last_send_us) < DRAG_WRITE_INTERVAL_US) {
        return;
    }

    if (nano_fx_write_gate_enabled(on)) {
        s_gate_on_last_sent = sent;
        s_gate_on_last_send_us = now;
    }
}

static void param_cell_apply_gate_on(lv_obj_t *cell, bool force_send)
{
    if (!s_gate_on.slider) {
        return;
    }

    lv_indev_t *indev = lv_indev_active();
    if (!indev) {
        return;
    }

    lv_point_t pt;
    lv_indev_get_point(indev, &pt);
    int val = param_cell_value_from_x(cell, s_gate_on.slider, pt.x, true);
    lv_slider_set_value(s_gate_on.slider, val, LV_ANIM_OFF);
    lv_label_set_text(s_gate_on.value, val ? "On" : "Off");

    if (s_gate_on_drag) {
        try_send_gate_on_locked(val != 0, force_send);
    }
}

static void param_cell_apply_gate(lv_obj_t *cell, bool force_send)
{
    if (!s_gate.slider) {
        return;
    }

    lv_indev_t *indev = lv_indev_active();
    if (!indev) {
        return;
    }

    lv_point_t pt;
    lv_indev_get_point(indev, &pt);
    int val = param_cell_x_to_slider(cell, s_gate.slider, pt.x);
    lv_slider_set_value(s_gate.slider, val, LV_ANIM_OFF);

    char buf[12];
    snprintf(buf, sizeof(buf), "%u%%", (unsigned)val);
    lv_label_set_text(s_gate.value, buf);

    if (s_gate_drag) {
        try_send_gate_locked((uint8_t)val, force_send);
    }
}

static void param_cell_apply_slider(lv_obj_t *cell, lv_obj_t *slider, bool is_amp, int idx, bool force_send)
{
    lv_indev_t *indev = lv_indev_active();
    if (!indev) {
        return;
    }

    lv_point_t pt;
    lv_indev_get_point(indev, &pt);

    bool binary_half = false;
    if (!is_amp) {
        const nano_fx_state_t *fx = nano_fx_get();
        if (fx) {
            const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
            if (block->model && (uint8_t)idx < block->param_count) {
                const char *pname = block->model->param_names[idx];
                if (nano_fx_param_is_enum(pname) && nano_fx_param_enum_count(pname) == 2) {
                    binary_half = true;
                }
            }
        }
    }

    int val = param_cell_value_from_x(cell, slider, pt.x, binary_half);
    lv_slider_set_value(slider, val, LV_ANIM_OFF);

    if (is_amp) {
        char buf[12];
        snprintf(buf, sizeof(buf), "%u%%", pct((uint8_t)val));
        lv_label_set_text(s_knobs[idx].value, buf);
        if (s_drag_knob == idx) {
            try_send_knob_locked(idx, (uint8_t)val, force_send);
        }
        return;
    }

    const nano_fx_state_t *fx = nano_fx_get();
    if (fx && fx->blocks[s_fx_selected].model && (uint8_t)idx < fx->blocks[s_fx_selected].param_count) {
        const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
        char val_txt[24];
        nano_fx_block_t tmp = *block;
        tmp.params[idx].display = nano_fx_slider_to_display(block->model->param_names[idx], val);
        tmp.params[idx].valid = true;
        nano_fx_format_param(&tmp, (uint8_t)idx, val_txt, sizeof(val_txt));
        lv_label_set_text(s_fx_params[idx].value, val_txt);
    }

    if (s_fx_drag_param == idx) {
        try_send_fx_param_locked(idx, val, force_send);
    }
}

static void param_gesture_begin(void)
{
    lv_indev_t *indev = lv_indev_active();
    if (!indev) {
        s_param_gesture.active = false;
        s_param_gesture.mode = PARAM_GESTURE_NONE;
        return;
    }

    s_param_gesture.active = true;
    s_param_gesture.mode = PARAM_GESTURE_PENDING;
    s_param_gesture.press_us = esp_timer_get_time();
    lv_indev_get_point(indev, &s_param_gesture.origin);
}

static void param_gesture_cancel_drag(void)
{
    s_fx_drag_param = -1;
    s_drag_knob = -1;
    s_gate_drag = 0;
    s_gate_on_drag = 0;
}

static void param_gesture_motion(lv_coord_t *adx, lv_coord_t *ady)
{
    lv_indev_t *indev = lv_indev_active();
    if (!indev || !s_param_gesture.active) {
        if (adx) {
            *adx = 0;
        }
        if (ady) {
            *ady = 0;
        }
        return;
    }

    lv_point_t pt;
    lv_indev_get_point(indev, &pt);
    lv_coord_t dx = pt.x - s_param_gesture.origin.x;
    lv_coord_t dy = pt.y - s_param_gesture.origin.y;
    if (adx) {
        *adx = LV_ABS(dx);
    }
    if (ady) {
        *ady = LV_ABS(dy);
    }
}

static param_gesture_mode_t param_gesture_update(void)
{
    if (!s_param_gesture.active) {
        return PARAM_GESTURE_NONE;
    }

    lv_coord_t adx = 0;
    lv_coord_t ady = 0;
    param_gesture_motion(&adx, &ady);

    if (s_param_gesture.mode == PARAM_GESTURE_SCROLL) {
        return PARAM_GESTURE_SCROLL;
    }

    if (s_param_gesture.mode == PARAM_GESTURE_ADJUST) {
        if (pedal_params_can_scroll() && ady > adx + 4) {
            s_param_gesture.mode = PARAM_GESTURE_SCROLL;
            param_gesture_cancel_drag();
            return PARAM_GESTURE_SCROLL;
        }
        return PARAM_GESTURE_ADJUST;
    }

    bool settled = (esp_timer_get_time() - s_param_gesture.press_us) >=
                   (int64_t)PARAM_GESTURE_SETTLE_MS * 1000;

    if (pedal_params_can_scroll() && pedal_params_row_active() &&
        ady >= PARAM_GESTURE_SCROLL_MIN_DY && ady > adx) {
        s_param_gesture.mode = PARAM_GESTURE_SCROLL;
        param_gesture_cancel_drag();
        return PARAM_GESTURE_SCROLL;
    }

    if (adx >= PARAM_GESTURE_MOVE_THRESH && adx > ady + 6) {
        s_param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    if (settled && adx >= PARAM_GESTURE_MOVE_THRESH && adx >= ady) {
        s_param_gesture.mode = PARAM_GESTURE_ADJUST;
        return PARAM_GESTURE_ADJUST;
    }

    return PARAM_GESTURE_PENDING;
}

static bool param_gesture_should_apply_on_release(void)
{
    param_gesture_mode_t mode = param_gesture_update();
    lv_coord_t adx = 0;
    lv_coord_t ady = 0;
    param_gesture_motion(&adx, &ady);

    if (mode == PARAM_GESTURE_SCROLL) {
        return false;
    }
    if (mode == PARAM_GESTURE_ADJUST) {
        return true;
    }
    if (adx >= PARAM_GESTURE_TAP_THRESH || ady >= PARAM_GESTURE_TAP_THRESH) {
        return false;
    }
    if (pedal_params_can_scroll() && ady > adx) {
        return false;
    }
    return true;
}

static void param_gesture_end(void)
{
    s_param_gesture.active = false;
    s_param_gesture.mode = PARAM_GESTURE_NONE;
}

static void param_cell_event_cb(lv_event_t *e)
{
    uintptr_t packed = (uintptr_t)lv_event_get_user_data(e);
    lv_obj_t *cell = lv_event_get_current_target_obj(e);
    lv_event_code_t code = lv_event_get_code(e);

    if (packed & PARAM_CELL_GATE_ON_TAG) {
        if (!s_gate_on.slider || lv_obj_has_flag(s_gate_on.slider, LV_OBJ_FLAG_HIDDEN)) {
            return;
        }
        if (code == LV_EVENT_PRESSED) {
            param_gesture_begin();
            return;
        }
        if (code == LV_EVENT_PRESSING) {
            param_gesture_mode_t mode = param_gesture_update();
            if (mode == PARAM_GESTURE_SCROLL) {
                return;
            }
            if (mode == PARAM_GESTURE_ADJUST) {
                s_gate_on_drag = 1;
                param_cell_apply_gate_on(cell, false);
            }
            return;
        }
        if (code == LV_EVENT_RELEASED) {
            if (param_gesture_should_apply_on_release()) {
                param_cell_apply_gate_on(cell, true);
            }
            s_gate_on_drag = 0;
            param_gesture_end();
        }
        return;
    }

    if (packed & PARAM_CELL_GATE_TAG) {
        if (!s_gate.slider || lv_obj_has_flag(s_gate.slider, LV_OBJ_FLAG_HIDDEN)) {
            return;
        }
        if (code == LV_EVENT_PRESSED) {
            param_gesture_begin();
            return;
        }
        if (code == LV_EVENT_PRESSING) {
            param_gesture_mode_t mode = param_gesture_update();
            if (mode == PARAM_GESTURE_SCROLL) {
                return;
            }
            if (mode == PARAM_GESTURE_ADJUST) {
                s_gate_drag = 1;
                param_cell_apply_gate(cell, false);
            }
            return;
        }
        if (code == LV_EVENT_RELEASED) {
            if (param_gesture_should_apply_on_release()) {
                param_cell_apply_gate(cell, true);
            }
            s_gate_drag = 0;
            param_gesture_end();
        }
        return;
    }

    int idx = (int)(packed & 0xFF);
    bool is_amp = (packed & PARAM_CELL_AMP_TAG) != 0;

    lv_obj_t *slider = is_amp ? s_knobs[idx].slider : s_fx_params[idx].slider;
    if (!slider || lv_obj_has_flag(slider, LV_OBJ_FLAG_HIDDEN)) {
        return;
    }

    if (code == LV_EVENT_PRESSED) {
        param_gesture_begin();
        return;
    }

    if (code == LV_EVENT_PRESSING) {
        param_gesture_mode_t mode = param_gesture_update();
        if (mode == PARAM_GESTURE_SCROLL) {
            return;
        }
        if (mode == PARAM_GESTURE_ADJUST) {
            if (is_amp) {
                s_drag_knob = idx;
            } else {
                s_fx_drag_param = idx;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, false);
        }
        return;
    }

    if (code == LV_EVENT_RELEASED) {
        if (param_gesture_should_apply_on_release()) {
            if (is_amp) {
                s_drag_knob = idx;
            } else {
                s_fx_drag_param = idx;
            }
            param_cell_apply_slider(cell, slider, is_amp, idx, true);
        }
        s_drag_knob = -1;
        s_fx_drag_param = -1;
        param_gesture_end();
    }
}

static void sync_header_model_locked(const nano_fx_state_t *fx)
{
    if (!s_fx_model_btn || !s_fx_model_btn_lbl) {
        return;
    }

    bool enabled = false;
    char model_text[56];
    const char *text = "-";

    if (s_tuner_view) {
        text = "Tuner";
        ui_set_label_if_changed(s_fx_model_btn_lbl, text);
        lv_obj_remove_state(s_fx_model_btn, LV_STATE_DISABLED);
        lv_obj_remove_flag(s_fx_model_btn, LV_OBJ_FLAG_CLICKABLE);
        lv_obj_set_style_bg_color(s_fx_model_btn, lv_color_hex(0x080808), LV_PART_MAIN);
        lv_obj_set_style_bg_opa(s_fx_model_btn, LV_OPA_COVER, LV_PART_MAIN);
        lv_obj_set_style_text_color(s_fx_model_btn_lbl, lv_color_hex(0xECECEC), 0);
        return;
    }

    lv_obj_add_flag(s_fx_model_btn, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_set_style_bg_color(s_fx_model_btn, lv_color_hex(0x080808), LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fx_model_btn, LV_OPA_COVER, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(s_fx_model_btn, LV_OPA_40, LV_PART_MAIN | LV_STATE_DISABLED);

    if (s_chain_selected == CHAIN_AMP) {
        const nano_amp_t *amp = nano_state_get_amp();
        if (amp && amp->valid) {
            nano_amp_capture_header_text(amp, model_text, sizeof(model_text));
            text = model_text;
            enabled = s_ble_connected;
        } else {
            text = "Amp";
        }
    } else if (chain_slot_is_fx(s_chain_selected)) {
        const nano_fx_block_t *block = fx ? &fx->blocks[s_fx_selected] : NULL;
        if (fx && block && block->valid) {
            nano_fx_block_model_text(block, model_text, sizeof(model_text));
            text = model_text;
            enabled = s_ble_connected;
        }
    }

    ui_set_label_if_changed(s_fx_model_btn_lbl, text);

    if (enabled) {
        lv_obj_remove_state(s_fx_model_btn, LV_STATE_DISABLED);
        lv_obj_set_style_text_color(s_fx_model_btn_lbl, lv_color_hex(0xECECEC), 0);
    } else {
        lv_obj_add_state(s_fx_model_btn, LV_STATE_DISABLED);
        lv_obj_set_style_text_color(s_fx_model_btn_lbl, lv_color_hex(0x606060), 0);
    }
}

static void apply_fx_param_locked(int i, const nano_fx_block_t *block)
{
    if (i < 0 || i >= NANO_FX_MAX_PARAMS || !block || !block->model) {
        return;
    }

    fx_param_ui_t *param = &s_fx_params[i];
    if (!param->col) {
        return;
    }

    if (i >= block->param_count || block->param_count == 0) {
        lv_obj_add_flag(param->col, LV_OBJ_FLAG_HIDDEN);
        return;
    }

    lv_obj_remove_flag(param->col, LV_OBJ_FLAG_HIDDEN);

    const char *pname = block->model->param_names[i];
    char short_name[32];
    fx_short_param_name(pname, short_name, sizeof(short_name));
    ui_set_label_if_changed(param->name, short_name);

    float display =
        block->params[i].valid ? block->params[i].display
                               : nano_fx_slider_to_display(pname,
                                                           (nano_fx_slider_min(pname) +
                                                            nano_fx_slider_max(pname)) /
                                                               2);

    int smin = nano_fx_slider_min(pname);
    int smax = nano_fx_slider_max(pname);
    if (lv_slider_get_min_value(param->slider) != smin || lv_slider_get_max_value(param->slider) != smax) {
        lv_slider_set_range(param->slider, smin, smax);
    }

    if (i != s_fx_drag_param) {
        int slider_val = nano_fx_display_to_slider(pname, display);
        ui_set_slider_if_changed(param->slider, slider_val);
        char val[24];
        nano_fx_block_t tmp = *block;
        if (!tmp.params[i].valid) {
            tmp.params[i].display = display;
            tmp.params[i].valid = true;
        }
        nano_fx_format_param(&tmp, (uint8_t)i, val, sizeof(val));
        ui_set_label_if_changed(param->value, val);
        s_fx_last_sent[i] = slider_val;
    }

    if (s_ble_connected && block->on && block->param_count > 0) {
        lv_obj_remove_state(param->slider, LV_STATE_DISABLED);
    } else {
        lv_obj_add_state(param->slider, LV_STATE_DISABLED);
    }
}

static void apply_fx_locked(const nano_fx_state_t *fx)
{
    s_fx_ui_sync = true;

    if (s_gate_on.col || s_gate.col) {
        if (!s_ble_connected) {
            if (s_gate_on.slider) {
                lv_obj_add_state(s_gate_on.slider, LV_STATE_DISABLED);
            }
            if (s_gate.slider) {
                lv_obj_add_state(s_gate.slider, LV_STATE_DISABLED);
            }
        } else {
            if (s_gate_on.slider) {
                lv_obj_remove_state(s_gate_on.slider, LV_STATE_DISABLED);
            }
            if (s_gate.slider) {
                lv_obj_remove_state(s_gate.slider, LV_STATE_DISABLED);
            }
        }
    }

    if (s_gate_on.col || s_gate.col) {
        bool gate_on = fx && fx->gate.valid && fx->gate.on;
        if (!s_gate_style_valid || gate_on != s_gate_style_on) {
            s_gate_style_on = gate_on;
            s_gate_style_valid = true;
            lv_opa_t opa = gate_on ? LV_OPA_COVER : LV_OPA_40;
            lv_color_t name_color = gate_on ? lv_color_hex(0xB0B0B0) : lv_color_hex(0x606060);
            if (s_gate_on.col) {
                lv_obj_set_style_opa(s_gate_on.col, opa, 0);
            }
            if (s_gate.col) {
                lv_obj_set_style_opa(s_gate.col, opa, 0);
            }
            if (s_gate_on.name) {
                lv_obj_set_style_text_color(s_gate_on.name, name_color, 0);
            }
            if (s_gate.name) {
                lv_obj_set_style_text_color(s_gate.name, name_color, 0);
            }
        }
        if (s_gate_on.value && s_gate_on.slider) {
            if (!fx || !fx->gate.valid) {
                if (s_gate_on_drag == 0) {
                    lv_slider_set_value(s_gate_on.slider, 0, LV_ANIM_OFF);
                    lv_label_set_text(s_gate_on.value, "-");
                }
            } else if (s_gate_on_drag == 0) {
                int on = fx->gate.on ? 1 : 0;
                ui_set_slider_if_changed(s_gate_on.slider, on);
                ui_set_label_if_changed(s_gate_on.value, on ? "On" : "Off");
            }
        }
        if (s_gate.value && s_gate.slider) {
            if (!fx || !fx->gate.valid) {
                if (s_gate_drag == 0) {
                    lv_slider_set_value(s_gate.slider, 50, LV_ANIM_OFF);
                    lv_label_set_text(s_gate.value, "-");
                }
            } else {
                if (s_gate_drag == 0) {
                    ui_set_slider_if_changed(s_gate.slider, fx->gate.reduction_pct);
                    char buf[12];
                    snprintf(buf, sizeof(buf), "%u%%", fx->gate.reduction_pct);
                    ui_set_label_if_changed(s_gate.value, buf);
                }
            }
        }
    }

    update_chain_blocks_locked(nano_state_get_amp(), fx);

    if (!s_fx_model_btn) {
        s_fx_ui_sync = false;
        return;
    }

    const nano_fx_block_t *block = fx ? &fx->blocks[s_fx_selected] : NULL;

    if (!fx || !block || !block->valid) {
        for (int i = 0; i < NANO_FX_MAX_PARAMS; i++) {
            if (s_fx_params[i].col) {
                lv_obj_add_flag(s_fx_params[i].col, LV_OBJ_FLAG_HIDDEN);
            }
        }
        sync_header_model_locked(fx);
        s_fx_ui_sync = false;
        return;
    }

    if (!fx_panel_is_visible()) {
        sync_header_model_locked(fx);
        s_fx_ui_sync = false;
        return;
    }

    if (s_fx_params_row && s_fx_selected != s_fx_scroll_anchor) {
        lv_obj_scroll_to(s_fx_params_row, 0, 0, LV_ANIM_OFF);
        s_fx_scroll_anchor = s_fx_selected;
        s_pedal_params_scroll_inited = false;
    }

    ensure_fx_params_up_to((int)block->param_count);
    update_pedal_params_grid_scroll(s_fx_params_row, block->param_count);

    for (int i = 0; i < NANO_FX_MAX_PARAMS; i++) {
        if (i >= block->param_count) {
            if (s_fx_params[i].col) {
                lv_obj_add_flag(s_fx_params[i].col, LV_OBJ_FLAG_HIDDEN);
            }
            continue;
        }
        apply_fx_param_locked(i, block);
    }

    sync_header_model_locked(fx);
    s_fx_ui_sync = false;
}

static bool fx_panel_is_visible(void)
{
    return s_fx_panel && !lv_obj_has_flag(s_fx_panel, LV_OBJ_FLAG_HIDDEN);
}

static void build_fx_param_cell(int i)
{
    if (i < 0 || i >= NANO_FX_MAX_PARAMS || s_fx_params[i].col || !s_fx_params_row) {
        return;
    }

    fx_param_ui_t *param = &s_fx_params[i];
    const lv_coord_t slider_w = param_slider_width();

    param->col = lv_obj_create(s_fx_params_row);
    style_param_cell(param->col, s_param_cell_w, s_param_cell_h);
    lv_obj_set_flex_flow(param->col, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(param->col, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_add_flag(param->col, LV_OBJ_FLAG_HIDDEN);
    ui_param_cell_container(param->col);
    lv_obj_add_event_cb(param->col, param_cell_event_cb, LV_EVENT_ALL, (void *)(intptr_t)i);

    param->name = lv_label_create(param->col);
    lv_label_set_text(param->name, "Param");
    style_param_name_label(param->name);

    param->slider = lv_slider_create(param->col);
    style_compact_h_slider(param->slider, slider_w, 0, 100);
    lv_obj_add_state(param->slider, LV_STATE_DISABLED);

    param->value = lv_label_create(param->col);
    lv_label_set_text(param->value, "-");
    style_param_value_label(param->value);
}

static void fx_build_timer_cb(lv_timer_t *timer)
{
    const nano_fx_state_t *fx = nano_fx_get();
    if (!fx || !fx_panel_is_visible()) {
        lv_timer_delete(timer);
        s_fx_build_timer = NULL;
        return;
    }

    const nano_fx_block_t *block = &fx->blocks[s_fx_selected];
    int count = block->valid ? (int)block->param_count : 0;
    if (count > NANO_FX_MAX_PARAMS) {
        count = NANO_FX_MAX_PARAMS;
    }

    int built_idx = -1;
    for (int j = 0; j < count; j++) {
        if (!s_fx_params[j].col) {
            build_fx_param_cell(j);
            built_idx = j;
            break;
        }
    }

    if (built_idx >= 0) {
        apply_fx_param_locked(built_idx, block);
        return;
    }

    lv_timer_delete(timer);
    s_fx_build_timer = NULL;
    apply_fx_locked(fx);
}

static void ensure_fx_params_up_to(int count)
{
    if (count > NANO_FX_MAX_PARAMS) {
        count = NANO_FX_MAX_PARAMS;
    }

    bool need_more = false;
    for (int j = 0; j < count; j++) {
        if (!s_fx_params[j].col) {
            need_more = true;
            break;
        }
    }

    if (need_more && !s_fx_build_timer) {
        s_fx_build_timer = lv_timer_create(fx_build_timer_cb, 50, NULL);
    }
}

static void build_fx_panel(lv_obj_t *parent)
{
    s_fx_params_row = create_pedal_params_grid(parent);
}

static void build_tuner_panel(lv_obj_t *parent)
{
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_flex_flow(parent, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(parent, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_row(parent, 0, 0);
    lv_obj_set_style_pad_top(parent, 0, 0);
    lv_obj_set_style_pad_bottom(parent, 20, 0);
    lv_obj_set_size(parent, LV_PCT(100), LV_PCT(100));

    lv_obj_t *body = lv_obj_create(parent);
    lv_obj_set_width(body, LV_PCT(100));
    lv_obj_set_flex_grow(body, 1);
    lv_obj_set_style_bg_opa(body, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(body, 0, 0);
    lv_obj_set_style_pad_all(body, 0, 0);
    lv_obj_set_style_pad_row(body, 10, 0);
    lv_obj_set_flex_flow(body, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(body, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_clear_flag(body, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(body, LV_OBJ_FLAG_CLICKABLE);

    s_tuner_note = lv_label_create(body);
    lv_label_set_text(s_tuner_note, "-");
    lv_obj_set_style_text_color(s_tuner_note, lv_color_hex(0xFFFFFF), 0);
    lv_obj_set_style_text_font(s_tuner_note, UI_FONT_TUNER_NOTE, 0);

    s_tuner_cents = lv_label_create(body);
    lv_label_set_text(s_tuner_cents, "Tuner off");
    lv_obj_set_style_text_color(s_tuner_cents, lv_color_hex(0xC0C0C0), 0);
    lv_obj_set_style_text_font(s_tuner_cents, UI_FONT_LG, 0);
    lv_obj_set_style_pad_bottom(s_tuner_cents, 16, 0);

    s_tuner_meter = lv_obj_create(body);
    lv_obj_set_size(s_tuner_meter, TUNER_METER_W, TUNER_METER_H);
    lv_obj_set_style_bg_color(s_tuner_meter, lv_color_hex(0x1A1A1A), 0);
    lv_obj_set_style_bg_opa(s_tuner_meter, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(s_tuner_meter, 1, 0);
    lv_obj_set_style_border_color(s_tuner_meter, lv_color_hex(0x404040), 0);
    lv_obj_set_style_radius(s_tuner_meter, 8, 0);
    lv_obj_set_style_pad_all(s_tuner_meter, 0, 0);
    lv_obj_clear_flag(s_tuner_meter, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(s_tuner_meter, LV_OBJ_FLAG_CLICKABLE);

    s_tuner_meter_center = lv_obj_create(s_tuner_meter);
    lv_obj_set_size(s_tuner_meter_center, 48, TUNER_METER_H - 4);
    lv_obj_set_style_bg_color(s_tuner_meter_center, lv_color_hex(TUNER_COLOR_OK), 0);
    lv_obj_set_style_bg_opa(s_tuner_meter_center, LV_OPA_30, 0);
    lv_obj_set_style_border_width(s_tuner_meter_center, 0, 0);
    lv_obj_set_style_radius(s_tuner_meter_center, 4, 0);
    lv_obj_set_style_pad_all(s_tuner_meter_center, 0, 0);
    lv_obj_align(s_tuner_meter_center, LV_ALIGN_CENTER, 0, 0);
    lv_obj_clear_flag(s_tuner_meter_center, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(s_tuner_meter_center, LV_OBJ_FLAG_CLICKABLE);

    lv_obj_t *tick = lv_obj_create(s_tuner_meter);
    lv_obj_set_size(tick, 2, TUNER_METER_H);
    lv_obj_set_style_bg_color(tick, lv_color_hex(0xA0A0A0), 0);
    lv_obj_set_style_bg_opa(tick, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(tick, 0, 0);
    lv_obj_set_style_radius(tick, 0, 0);
    lv_obj_set_style_pad_all(tick, 0, 0);
    lv_obj_align(tick, LV_ALIGN_CENTER, 0, 0);
    lv_obj_clear_flag(tick, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(tick, LV_OBJ_FLAG_CLICKABLE);

    s_tuner_meter_needle = lv_obj_create(s_tuner_meter);
    lv_obj_set_size(s_tuner_meter_needle, TUNER_NEEDLE_W, TUNER_METER_H);
    lv_obj_set_style_bg_color(s_tuner_meter_needle, lv_color_hex(TUNER_COLOR_IDLE), 0);
    lv_obj_set_style_bg_opa(s_tuner_meter_needle, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(s_tuner_meter_needle, 0, 0);
    lv_obj_set_style_radius(s_tuner_meter_needle, 3, 0);
    lv_obj_set_style_pad_all(s_tuner_meter_needle, 0, 0);
    lv_obj_set_pos(s_tuner_meter_needle, (TUNER_METER_W - TUNER_NEEDLE_W) / 2, 0);
    lv_obj_clear_flag(s_tuner_meter_needle, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_remove_flag(s_tuner_meter_needle, LV_OBJ_FLAG_CLICKABLE);

    s_tuner_state = lv_label_create(body);
    lv_label_set_text(s_tuner_state, "");
    lv_obj_add_flag(s_tuner_state, LV_OBJ_FLAG_HIDDEN);

    s_tuner_btn = lv_button_create(parent);
    lv_obj_set_size(s_tuner_btn, 160, 48);
    lv_obj_add_event_cb(s_tuner_btn, tuner_btn_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_state(s_tuner_btn, LV_STATE_DISABLED);
    lv_obj_t *btn_label = lv_label_create(s_tuner_btn);
    lv_label_set_text(btn_label, "ON");
    lv_obj_center(btn_label);
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

static void style_screen(lv_obj_t *screen)
{
    lv_obj_set_style_bg_color(screen, lv_color_hex(0x121212), 0);
    lv_obj_set_style_bg_opa(screen, LV_OPA_COVER, 0);
    lv_obj_set_style_pad_all(screen, UI_EDGE_PAD, 0);
}

static lv_coord_t calc_param_cell_width(lv_coord_t area_w, int cols)
{
    if (cols < 1) {
        cols = 1;
    }
    lv_coord_t area = area_w - 2 * PARAM_GRID_PAD;
    lv_coord_t gaps = (lv_coord_t)(cols - 1) * PARAM_GRID_GAP;
    return (area - gaps) / cols;
}

static lv_coord_t calc_param_cell_height(lv_coord_t area_h, int rows)
{
    if (rows < 1) {
        rows = 1;
    }
    lv_coord_t area = area_h - 2 * PARAM_GRID_PAD;
    lv_coord_t gaps = (lv_coord_t)(rows - 1) * PARAM_GRID_GAP;
    return (area - gaps) / rows;
}

static lv_coord_t param_slider_width(void)
{
    lv_coord_t w = s_param_cell_w - 2 * PARAM_CELL_PAD - 2 * PARAM_SLIDER_INSET_H;
    if (w < 48) {
        w = 48;
    }
    return w;
}

static void style_param_grid_container(lv_obj_t *obj)
{
    lv_obj_set_style_pad_all(obj, PARAM_GRID_PAD, 0);
    lv_obj_set_style_pad_row(obj, PARAM_GRID_GAP, 0);
    lv_obj_set_style_pad_column(obj, PARAM_GRID_GAP, 0);
    lv_obj_set_flex_flow(obj, LV_FLEX_FLOW_ROW_WRAP);
    lv_obj_set_flex_align(obj, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_START);
}

static lv_obj_t *make_tuner_hdr_btn(lv_obj_t *parent)
{
    lv_obj_t *btn = lv_button_create(parent);
    lv_obj_set_size(btn, 36, 36);
    lv_obj_set_style_bg_opa(btn, LV_OPA_TRANSP, LV_PART_MAIN);
    lv_obj_set_style_bg_opa(btn, LV_OPA_TRANSP, LV_PART_MAIN | LV_STATE_CHECKED);
    lv_obj_set_style_border_width(btn, 1, LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, lv_color_hex(0x404040), LV_PART_MAIN);
    lv_obj_set_style_border_color(btn, lv_color_hex(0x8090A0), LV_PART_MAIN | LV_STATE_CHECKED);
    lv_obj_set_style_shadow_width(btn, 0, LV_PART_MAIN);
    lv_obj_set_style_radius(btn, 6, LV_PART_MAIN);
    lv_obj_add_flag(btn, LV_OBJ_FLAG_CHECKABLE);
    lv_obj_add_event_cb(btn, tuner_hdr_btn_event_cb, LV_EVENT_CLICKED, NULL);

    lv_obj_t *img = lv_image_create(btn);
    lv_image_set_src(img, &ui_icon_tuner_fork);
    lv_obj_set_style_image_recolor(img, lv_color_hex(0xB0B8C0), 0);
    lv_obj_set_style_image_recolor_opa(img, LV_OPA_COVER, 0);
    lv_obj_center(img);
    return btn;
}

static void init_fx_model_btn_style(void)
{
    if (s_fx_model_btn_style_inited) {
        return;
    }

    lv_style_init(&s_fx_model_btn_style);
    lv_style_set_bg_color(&s_fx_model_btn_style, lv_color_hex(0x080808));
    lv_style_set_bg_opa(&s_fx_model_btn_style, LV_OPA_COVER);
    lv_style_set_border_width(&s_fx_model_btn_style, 0);
    lv_style_set_shadow_width(&s_fx_model_btn_style, 0);
    lv_style_set_radius(&s_fx_model_btn_style, 4);
    lv_style_set_pad_top(&s_fx_model_btn_style, 8);
    lv_style_set_pad_bottom(&s_fx_model_btn_style, 8);
    lv_style_set_pad_left(&s_fx_model_btn_style, 12);
    lv_style_set_pad_right(&s_fx_model_btn_style, 12);

    s_fx_model_btn_style_inited = true;
}

static void style_fx_model_header_btn(lv_obj_t *btn)
{
    if (!btn) {
        return;
    }

    init_fx_model_btn_style();
    lv_obj_remove_style_all(btn);
    lv_obj_add_style(btn, &s_fx_model_btn_style, LV_PART_MAIN | LV_STATE_ANY);
    lv_obj_set_style_bg_opa(btn, LV_OPA_40, LV_PART_MAIN | LV_STATE_DISABLED);
    lv_obj_set_width(btn, LV_PCT(100));
    lv_obj_set_height(btn, HEADER_DD_H);
    lv_obj_set_flex_grow(btn, 1);
}

static void build_header_bar(lv_obj_t *screen)
{
    lv_obj_t *header = lv_obj_create(screen);
    s_header_bar = header;
    lv_obj_set_size(header, LV_PCT(100), HEADER_H);
    lv_obj_set_style_bg_opa(header, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(header, 0, 0);
    lv_obj_set_style_pad_all(header, 0, 0);
    lv_obj_set_flex_flow(header, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(header, LV_FLEX_ALIGN_SPACE_BETWEEN, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_align(header, LV_ALIGN_TOP_MID, 0, 0);
    ui_passive_container(header);

    lv_obj_t *left = lv_obj_create(header);
    lv_obj_set_size(left, 196, HEADER_H);
    lv_obj_set_style_bg_opa(left, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(left, 0, 0);
    lv_obj_set_style_pad_left(left, 8, 0);
    lv_obj_set_style_pad_right(left, 4, 0);
    lv_obj_set_flex_flow(left, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(left, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    ui_passive_container(left);

    s_preset_lbl = lv_label_create(left);
    lv_label_set_text(s_preset_lbl, "-");
    lv_obj_set_width(s_preset_lbl, 184);
    lv_label_set_long_mode(s_preset_lbl, LV_LABEL_LONG_DOT);
    lv_obj_set_style_text_color(s_preset_lbl, lv_color_hex(0x909090), 0);
    lv_obj_set_style_text_font(s_preset_lbl, UI_FONT_SM, 0);
    lv_obj_set_style_text_align(s_preset_lbl, LV_TEXT_ALIGN_LEFT, 0);

    lv_obj_t *center = lv_obj_create(header);
    lv_obj_set_height(center, HEADER_H);
    lv_obj_set_flex_grow(center, 1);
    lv_obj_set_style_bg_opa(center, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(center, 0, 0);
    lv_obj_set_style_pad_all(center, 0, 0);
    lv_obj_set_flex_flow(center, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(center, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    ui_passive_container(center);

    lv_obj_t *dd_host = lv_obj_create(center);
    lv_obj_set_size(dd_host, LV_PCT(100), HEADER_DD_H);
    lv_obj_set_flex_grow(dd_host, 1);
    lv_obj_set_style_bg_opa(dd_host, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(dd_host, 0, 0);
    lv_obj_set_style_pad_all(dd_host, 0, 0);
    ui_passive_container(dd_host);

    s_fx_model_btn = lv_button_create(dd_host);
    lv_obj_set_size(s_fx_model_btn, LV_PCT(100), LV_PCT(100));
    style_fx_model_header_btn(s_fx_model_btn);
    lv_obj_add_event_cb(s_fx_model_btn, fx_model_header_btn_event_cb, LV_EVENT_CLICKED, NULL);
    lv_obj_add_state(s_fx_model_btn, LV_STATE_DISABLED);

    s_fx_model_btn_lbl = lv_label_create(s_fx_model_btn);
    lv_label_set_text(s_fx_model_btn_lbl, "-");
    lv_obj_set_width(s_fx_model_btn_lbl, LV_PCT(100));
    lv_label_set_long_mode(s_fx_model_btn_lbl, LV_LABEL_LONG_DOT);
    lv_obj_set_style_text_font(s_fx_model_btn_lbl, UI_FONT_LG, 0);
    lv_obj_set_style_text_color(s_fx_model_btn_lbl, lv_color_hex(0x606060), 0);
    lv_obj_set_style_text_align(s_fx_model_btn_lbl, LV_TEXT_ALIGN_CENTER, 0);
    lv_obj_center(s_fx_model_btn_lbl);

    lv_obj_t *right = lv_obj_create(header);
    lv_obj_set_size(right, 200, HEADER_H);
    lv_obj_set_style_bg_opa(right, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(right, 0, 0);
    lv_obj_set_style_pad_all(right, 0, 0);
    lv_obj_set_style_pad_right(right, HEADER_RIGHT_PAD, 0);
    lv_obj_set_flex_flow(right, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(right, LV_FLEX_ALIGN_END, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(right, 14, 0);
    ui_passive_container(right);

    lv_obj_t *bpm_row = lv_obj_create(right);
    lv_obj_set_height(bpm_row, HEADER_H);
    lv_obj_set_width(bpm_row, LV_SIZE_CONTENT);
    lv_obj_set_style_bg_opa(bpm_row, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(bpm_row, 0, 0);
    lv_obj_set_style_pad_all(bpm_row, 0, 0);
    lv_obj_set_flex_flow(bpm_row, LV_FLEX_FLOW_ROW);
    lv_obj_set_flex_align(bpm_row, LV_FLEX_ALIGN_END, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    lv_obj_set_style_pad_column(bpm_row, BPM_DOT_GAP, 0);
    ui_passive_container(bpm_row);

    s_bpm_dot = lv_obj_create(bpm_row);
    lv_obj_set_size(s_bpm_dot, BPM_DOT_SIZE, BPM_DOT_SIZE);
    lv_obj_set_style_radius(s_bpm_dot, LV_RADIUS_CIRCLE, 0);
    lv_obj_set_style_bg_color(s_bpm_dot, lv_color_hex(BPM_DOT_COLOR), 0);
    lv_obj_set_style_bg_opa(s_bpm_dot, LV_OPA_20, 0);
    lv_obj_set_style_border_width(s_bpm_dot, 0, 0);
    lv_obj_set_style_pad_all(s_bpm_dot, 0, 0);
    lv_obj_clear_flag(s_bpm_dot, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_add_flag(s_bpm_dot, LV_OBJ_FLAG_HIDDEN);
    ui_passive_container(s_bpm_dot);

    s_bpm_lbl = lv_label_create(bpm_row);
    lv_label_set_text(s_bpm_lbl, "- BPM");
    lv_obj_set_style_text_color(s_bpm_lbl, lv_color_hex(0x909090), 0);
    lv_obj_set_style_text_font(s_bpm_lbl, UI_FONT_SM, 0);
    lv_obj_set_width(s_bpm_lbl, LV_SIZE_CONTENT);
    lv_label_set_long_mode(s_bpm_lbl, LV_LABEL_LONG_CLIP);
    lv_obj_set_style_text_align(s_bpm_lbl, LV_TEXT_ALIGN_LEFT, 0);

    s_ble_icon = lv_label_create(right);
    lv_label_set_text(s_ble_icon, LV_SYMBOL_BLUETOOTH);
    lv_obj_set_style_text_font(s_ble_icon, UI_FONT_SYMBOL, 0);
    lv_obj_set_style_text_color(s_ble_icon, lv_color_hex(0x505050), 0);
    lv_obj_set_style_opa(s_ble_icon, LV_OPA_30, 0);

    s_tuner_hdr_btn = make_tuner_hdr_btn(right);
}

static void make_knob_column(lv_obj_t *parent, amp_knob_ui_t *knob, const char *label, int idx)
{
    knob->label = label;
    const lv_coord_t slider_w = param_slider_width();

    lv_obj_t *col = lv_obj_create(parent);
    style_param_cell(col, s_param_cell_w, s_param_cell_h);
    lv_obj_set_flex_flow(col, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(col, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    ui_param_cell_container(col);
    lv_obj_add_event_cb(col, param_cell_event_cb, LV_EVENT_ALL, (void *)(intptr_t)(PARAM_CELL_AMP_TAG | idx));

    lv_obj_t *name = lv_label_create(col);
    lv_label_set_text(name, label);
    style_param_name_label(name);

    knob->slider = lv_slider_create(col);
    style_compact_h_slider(knob->slider, slider_w, 0, 255);
    lv_obj_add_state(knob->slider, LV_STATE_DISABLED);

    knob->value = lv_label_create(col);
    lv_label_set_text(knob->value, "-");
    style_param_value_label(knob->value);
}

static void make_gate_slider_cell(gate_cell_ui_t *cell, lv_obj_t *parent, const char *label,
                                  uintptr_t tag, int32_t smin, int32_t smax)
{
    const lv_coord_t slider_w = param_slider_width();

    cell->col = lv_obj_create(parent);
    style_param_cell(cell->col, s_param_cell_w, s_param_cell_h);
    lv_obj_set_flex_flow(cell->col, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_flex_align(cell->col, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    ui_param_cell_container(cell->col);
    lv_obj_add_event_cb(cell->col, param_cell_event_cb, LV_EVENT_ALL, (void *)(intptr_t)tag);

    cell->name = lv_label_create(cell->col);
    lv_label_set_text(cell->name, label);
    style_param_name_label(cell->name);

    cell->slider = lv_slider_create(cell->col);
    style_compact_h_slider(cell->slider, slider_w, smin, smax);
    lv_obj_add_state(cell->slider, LV_STATE_DISABLED);

    cell->value = lv_label_create(cell->col);
    lv_label_set_text(cell->value, "-");
    style_param_value_label(cell->value);
}

static void make_gate_on_cell(lv_obj_t *parent)
{
    make_gate_slider_cell(&s_gate_on, parent, "ON", PARAM_CELL_GATE_ON_TAG, 0, 1);
}

static void make_gate_cell(lv_obj_t *parent)
{
    make_gate_slider_cell(&s_gate, parent, "LEVEL", PARAM_CELL_GATE_TAG, 0, 100);
}

static void build_amp_panel(lv_obj_t *parent)
{
    s_amp_params_row = create_pedal_params_grid(parent);

    make_knob_column(s_amp_params_row, &s_knobs[0], "GAIN", 0);
    make_knob_column(s_amp_params_row, &s_knobs[1], "LEVEL", 1);
    make_knob_column(s_amp_params_row, &s_knobs[2], "BASS", 2);
    make_knob_column(s_amp_params_row, &s_knobs[3], "MID", 3);
    make_knob_column(s_amp_params_row, &s_knobs[4], "TREBLE", 4);
    make_gate_on_cell(s_amp_params_row);
    make_gate_cell(s_amp_params_row);
}

static void build_chain_bar(lv_obj_t *parent, bool vertical, lv_coord_t tab_h, lv_coord_t tab_extra)
{
    lv_obj_set_style_bg_opa(parent, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(parent, 0, 0);
    lv_obj_set_style_pad_all(parent, 0, 0);
    ui_passive_container(parent);

    const lv_coord_t use_tab_h = (vertical && tab_h > 0) ? tab_h : CHAIN_TAB_H;

    lv_obj_t *list = lv_obj_create(parent);
    lv_obj_set_size(list, LV_PCT(100), LV_PCT(100));
    lv_obj_set_style_bg_opa(list, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(list, 0, 0);
    if (vertical) {
        lv_obj_set_style_pad_left(list, CHAIN_RAIL_PAD, 0);
        lv_obj_set_style_pad_top(list, PARAM_GRID_PAD, 0);
        lv_obj_set_style_pad_bottom(list, CHAIN_RAIL_BOTTOM_PAD, 0);
        lv_obj_set_style_pad_right(list, 0, 0);
        lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
        lv_obj_set_flex_align(list, LV_FLEX_ALIGN_START, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
        lv_obj_set_style_pad_row(list, CHAIN_TAB_GAP, 0);
    } else {
        lv_obj_set_style_pad_all(list, 0, 0);
        lv_obj_set_flex_flow(list, LV_FLEX_FLOW_ROW);
        lv_obj_set_flex_align(list, LV_FLEX_ALIGN_SPACE_EVENLY, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
    }
    ui_passive_container(list);

    for (int i = 0; i < CHAIN_BLOCK_COUNT; i++) {
        chain_block_ui_t *cb = &s_chain[i];
        lv_coord_t slot_h = use_tab_h;
        if (vertical && tab_extra > 0 && (lv_coord_t)i < tab_extra) {
            slot_h++;
        }

        cb->col = lv_obj_create(list);
        lv_obj_set_size(cb->col, CHAIN_TAB_W, slot_h);
        lv_obj_set_style_bg_opa(cb->col, LV_OPA_TRANSP, 0);
        lv_obj_set_style_border_width(cb->col, 0, 0);
        lv_obj_set_style_pad_all(cb->col, 0, 0);
        lv_obj_set_style_radius(cb->col, 6, 0);
        lv_obj_set_flex_flow(cb->col, LV_FLEX_FLOW_COLUMN);
        lv_obj_set_flex_align(cb->col, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER, LV_FLEX_ALIGN_CENTER);
        ui_passive_container(cb->col);

        cb->btn = lv_obj_create(cb->col);
        lv_obj_set_size(cb->btn, LV_PCT(100), LV_PCT(100));
        chain_btn_prepare(cb->btn);
        lv_obj_add_event_cb(cb->btn, chain_block_btn_event_cb, LV_EVENT_ALL, (void *)(intptr_t)i);
        ui_no_scroll_only(cb->btn);

        cb->icon = lv_label_create(cb->btn);
        lv_obj_set_width(cb->icon, CHAIN_ICON_W);
        lv_label_set_long_mode(cb->icon, LV_LABEL_LONG_DOT);
        lv_obj_set_style_text_align(cb->icon, LV_TEXT_ALIGN_CENTER, 0);
        lv_obj_set_style_text_font(cb->icon, UI_FONT_CHAIN, 0);
        lv_obj_set_style_text_color(cb->icon, lv_color_hex(0x505050), 0);
        lv_obj_add_flag(cb->icon, LV_OBJ_FLAG_EVENT_BUBBLE);
        lv_obj_center(cb->icon);

        ui_icon_kind_t init_icon = (i == CHAIN_AMP) ? UI_ICON_AMP : UI_ICON_UTILITY;
        cb->icon_text[0] = '\0';
        cb->last_selected = false;
        cb->last_active = false;
        cb->last_accent = lv_color_hex(0x000000);
        chain_set_block_label(cb, icon_kind_glyph(init_icon), lv_color_hex(0x505050));
    }
}

static void build_ui(void)
{
    lv_obj_t *screen = lv_screen_active();
    style_screen(screen);

    build_header_bar(screen);
    build_fx_model_picker(screen);

    const lv_coord_t inner_h = LV_VER_RES - 2 * UI_EDGE_PAD;
    const lv_coord_t body_h = inner_h - HEADER_H;

#if NANO_UI_CHAIN_LEFT
    const lv_coord_t chain_vpad = PARAM_GRID_PAD + CHAIN_RAIL_BOTTOM_PAD;
    const lv_coord_t chain_gap_total = (CHAIN_BLOCK_COUNT - 1) * CHAIN_TAB_GAP;
    const lv_coord_t chain_avail = body_h - chain_vpad - chain_gap_total;
    const lv_coord_t chain_tab_h = chain_avail / CHAIN_BLOCK_COUNT;

    const lv_coord_t content_w = LV_HOR_RES - 2 * UI_EDGE_PAD - CHAIN_RAIL_W - CONTENT_CHAIN_GAP;
    const lv_coord_t content_x = CHAIN_RAIL_W + CONTENT_CHAIN_GAP;
    s_body_h = body_h;
    s_content_w_normal = content_w;
    s_content_x_normal = content_x;

    s_param_cell_w = calc_param_cell_width(content_w - PARAM_GRID_SCROLLBAR_W, PARAM_GRID_COLS);
    s_param_cell_h = calc_param_cell_height(body_h, PARAM_GRID_ROWS);

    s_chain_bar = lv_obj_create(screen);
    lv_obj_set_size(s_chain_bar, CHAIN_RAIL_W, body_h);
    lv_obj_align(s_chain_bar, LV_ALIGN_TOP_LEFT, 0, HEADER_H);
    build_chain_bar(s_chain_bar, true, chain_tab_h, 0);

    s_content_root = lv_obj_create(screen);
    lv_obj_set_size(s_content_root, content_w, body_h);
    lv_obj_align(s_content_root, LV_ALIGN_TOP_LEFT, content_x, HEADER_H);
#else
    const lv_coord_t content_h = body_h - CHAIN_BAR_H;

    s_param_cell_w = calc_param_cell_width(LV_HOR_RES - 2 * UI_EDGE_PAD - PARAM_GRID_SCROLLBAR_W,
                                           PARAM_GRID_COLS);
    s_param_cell_h = calc_param_cell_height(content_h, PARAM_GRID_ROWS);

    s_content_root = lv_obj_create(screen);
    lv_obj_set_size(s_content_root, LV_PCT(100), content_h);
    lv_obj_align(s_content_root, LV_ALIGN_TOP_MID, 0, HEADER_H);

    s_chain_bar = lv_obj_create(screen);
    lv_obj_set_size(s_chain_bar, LV_PCT(100), CHAIN_BAR_H);
    lv_obj_align(s_chain_bar, LV_ALIGN_BOTTOM_MID, 0, 0);
    build_chain_bar(s_chain_bar, false, 0, 0);
#endif
    lv_obj_set_style_bg_opa(s_content_root, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(s_content_root, 0, 0);
    lv_obj_set_style_pad_all(s_content_root, 0, 0);
    ui_passive_container(s_content_root);

    s_amp_panel = make_content_panel(s_content_root);
    build_amp_panel(s_amp_panel);

    s_fx_panel = make_content_panel(s_content_root);
    build_fx_panel(s_fx_panel);

    s_tuner_panel = make_content_panel(s_content_root);
    build_tuner_panel(s_tuner_panel);

    show_content_for_chain(CHAIN_AMP);
    ui_bring_chrome_to_front();

    ui_init_dispatch();
    if (!s_ble_icon_timer) {
        s_ble_icon_timer = lv_timer_create(ble_icon_timer_cb, 400, NULL);
    }
    refresh_ble_icon_locked();
    apply_tuner_locked(nano_tuner_get());
    apply_fx_locked(nano_fx_get());
    update_chain_blocks_locked(nano_state_get_amp(), nano_fx_get());
}

static void ui_build_timer_cb(lv_timer_t *timer)
{
    build_ui();
    if (s_ui_build_done) {
        xSemaphoreGive(s_ui_build_done);
    }
    lv_timer_delete(timer);
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
            lv_label_set_text(s_knobs[i].value, "-");
            s_last_sent[i] = 0;
        }
        set_slider_enabled_locked(false);
        update_chain_blocks_locked(NULL, nano_fx_get());
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
        ui_set_slider_if_changed(s_knobs[i].slider, raw[i]);
        snprintf(buf, sizeof(buf), "%u%%", pct(raw[i]));
        ui_set_label_if_changed(s_knobs[i].value, buf);
    }

    set_slider_enabled_locked(s_ble_connected);
    update_pedal_params_grid_scroll(s_amp_params_row, AMP_PEDAL_PARAM_COUNT);
    update_chain_blocks_locked(amp, nano_fx_get());
    refresh_preset_label_locked();
    if (s_chain_selected == CHAIN_AMP && !s_tuner_view) {
        sync_header_model_locked(nano_fx_get());
    }
    s_ui_sync = false;
}

esp_err_t nano_ui_start(void)
{
    esp_lcd_panel_handle_t panel = NULL;
    esp_lcd_touch_handle_t touch = NULL;

    ESP_ERROR_CHECK(waveshare_esp32_s3_rgb_lcd_init(&panel, &touch));
    ESP_ERROR_CHECK(lvgl_port_init(panel, touch));
    ESP_ERROR_CHECK(waveshare_rgb_lcd_bl_on());

    s_ui_build_done = xSemaphoreCreateBinary();
    if (!s_ui_build_done) {
        return ESP_ERR_NO_MEM;
    }

    if (!lvgl_port_lock(-1)) {
        vSemaphoreDelete(s_ui_build_done);
        s_ui_build_done = NULL;
        return ESP_FAIL;
    }

    lv_timer_t *build_timer = lv_timer_create(ui_build_timer_cb, 1, NULL);
    if (!build_timer) {
        lvgl_port_unlock();
        vSemaphoreDelete(s_ui_build_done);
        s_ui_build_done = NULL;
        return ESP_ERR_NO_MEM;
    }
    lv_timer_set_repeat_count(build_timer, 1);
    lvgl_port_unlock();

    if (xSemaphoreTake(s_ui_build_done, pdMS_TO_TICKS(60000)) != pdTRUE) {
        ESP_LOGE(TAG, "UI build timed out");
        vSemaphoreDelete(s_ui_build_done);
        s_ui_build_done = NULL;
        return ESP_FAIL;
    }

    vSemaphoreDelete(s_ui_build_done);
    s_ui_build_done = NULL;

#if NANO_UI_CHAIN_LEFT
    ESP_LOGI(TAG, "UI ready (800x480, left chain rail + panels)");
#else
    ESP_LOGI(TAG, "UI ready (800x480, signal chain + panels)");
#endif
    return ESP_OK;
}

void nano_ui_set_amp(const nano_amp_t *amp)
{
    if (amp && amp->valid) {
        if (s_ui_amp_cache_valid && memcmp(amp, &s_ui_amp_cache, sizeof(*amp)) == 0) {
            return;
        }
        s_ui_amp_cache = *amp;
        s_ui_amp_cache_valid = true;
        s_async_amp = *amp;
        s_async_amp_have = true;
    } else {
        if (!s_ui_amp_cache_valid) {
            return;
        }
        s_ui_amp_cache_valid = false;
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

void nano_ui_set_preset_name(const char *name)
{
    char incoming[NANO_PRESET_NAME_MAX];
    incoming[0] = '\0';
    if (name && name[0]) {
        strncpy(incoming, name, sizeof(incoming) - 1);
        incoming[sizeof(incoming) - 1] = '\0';
    }

    if (s_ui_preset_cache_valid && strcmp(incoming, s_ui_preset_cache) == 0) {
        return;
    }

    strncpy(s_ui_preset_cache, incoming, sizeof(s_ui_preset_cache) - 1);
    s_ui_preset_cache[sizeof(s_ui_preset_cache) - 1] = '\0';
    s_ui_preset_cache_valid = true;

    strncpy(s_async_preset, incoming, sizeof(s_async_preset) - 1);
    s_async_preset[sizeof(s_async_preset) - 1] = '\0';
    s_async_preset_pending = true;
    s_ui_dispatch_mask |= UI_DISP_PRESET;
    ui_request_dispatch();
}

void nano_ui_set_bpm(uint16_t bpm, bool valid)
{
    if (valid) {
        if (s_ui_bpm_cache_valid && s_ui_bpm_cache == bpm) {
            return;
        }
        s_ui_bpm_cache = bpm;
        s_ui_bpm_cache_valid = true;
    } else {
        if (!s_ui_bpm_cache_valid) {
            return;
        }
        s_ui_bpm_cache_valid = false;
    }

    s_async_bpm = bpm;
    s_async_bpm_valid = valid;
    s_async_bpm_pending = true;
    s_async_bpm_rephase = false;
    s_ui_dispatch_mask |= UI_DISP_BPM;
    ui_request_dispatch();
}

void nano_ui_bpm_tap(uint16_t bpm)
{
    if (bpm < 20 || bpm > 400) {
        return;
    }

    s_ui_bpm_cache = bpm;
    s_ui_bpm_cache_valid = true;
    s_async_bpm = bpm;
    s_async_bpm_valid = true;
    s_async_bpm_pending = true;
    s_async_bpm_rephase = true;
    s_ui_dispatch_mask |= UI_DISP_BPM;
    ui_request_dispatch();
}

void nano_ui_set_fx(const nano_fx_state_t *fx)
{
    if (fx) {
        if (s_ui_fx_cache_valid && memcmp(fx, &s_ui_fx_cache, sizeof(*fx)) == 0) {
            return;
        }
        s_ui_fx_cache = *fx;
        s_ui_fx_cache_valid = true;
        s_async_fx = *fx;
        s_async_fx_pending = true;
    } else {
        if (!s_ui_fx_cache_valid) {
            return;
        }
        s_ui_fx_cache_valid = false;
        s_async_fx_pending = false;
        memset(&s_async_fx, 0, sizeof(s_async_fx));
    }
    s_ui_dispatch_mask |= UI_DISP_FX;
    ui_request_dispatch();
}
