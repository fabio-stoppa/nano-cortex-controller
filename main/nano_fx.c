#include "nano_fx.h"

#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "esp_log.h"
#include "esp_timer.h"
#include "nano_ble.h"
#include "nano_protocol.h"
#include "nano_proto.h"
#include "nano_state.h"
#include "nano_ui.h"

static const char *TAG = "nano_fx";

#define FX_REFRESH_TIMEOUT_US   (900 * 1000)
#define FX_REFRESH_GAP_US       (80 * 1000)
#define FX_ENABLE_ASSERT_US     (3000 * 1000)
#define FX_MODEL_WRITE_ASSERT_US (30 * 1000 * 1000)

static void fx_param_range_impl(const char *name, float *min_out, float *max_out, float *step_out,
                                bool *is_pct);

typedef struct {
    uint8_t model_slot;
    uint8_t refresh_slot;
    uint8_t enable_slot;
    uint32_t model_field;
} nano_fx_slot_cfg_t;

static const nano_fx_slot_cfg_t s_slot_cfg[NANO_FX_BLOCK_COUNT] = {
    {0x00, 0x00, 0x04, 48},
    {0x01, 0x01, 0x05, 49},
    {0x02, 0x02, 0x06, 50},
    {0x03, 0x03, 0x07, 51},
    {0x04, 0x04, 0x08, 52},
};

/** Device firmware also exposes 1-byte model IDs in these varint fields. */
static const uint32_t s_model_field_legacy[NANO_FX_BLOCK_COUNT] = {26, 9, 13, 14, 15};
/** Alternate field numbers seen on some firmware builds. */
static const uint32_t s_model_field_legacy_alt[NANO_FX_BLOCK_COUNT] = {8, 10, 0, 0, 0};

static nano_fx_state_t s_fx;
static nano_fx_block_id_t s_refresh_queue[NANO_FX_BLOCK_COUNT];
static size_t s_refresh_q_head;
static size_t s_refresh_q_len;
static nano_fx_block_id_t s_pending_block;
static bool s_refresh_active;
static esp_timer_handle_t s_refresh_timeout_timer;
static esp_timer_handle_t s_refresh_gap_timer;
static esp_timer_handle_t s_begin_refresh_timer;
static int64_t s_block_on_asserted_us[NANO_FX_BLOCK_COUNT];
static int64_t s_block_model_write_us[NANO_FX_BLOCK_COUNT];
/** Protects local Gate ON/OFF against stale live-poll until assert window ends. */
static int64_t s_gate_assert_us;
static int s_gate_assert_on; /* -1 idle, 0 off, 1 on */
/** Protects local Gate amount against stale field-53 until assert window ends. */
static int64_t s_gate_amount_assert_us;
static int s_gate_amount_assert_pct; /* -1 idle, else 0..100 */
/** Model last selected via BLE write; preset/inference must not override. */
static const nano_fx_model_def_t *s_block_trusted_model[NANO_FX_BLOCK_COUNT];

static void refresh_timeout_cb(void *arg);
static void refresh_gap_cb(void *arg);
static void begin_refresh_cb(void *arg);
static void send_next_refresh(void);
static void enqueue_param_refresh(nano_fx_block_id_t block);
static void run_begin_param_refresh(void);
static bool block_in_refresh_queue(nano_fx_block_id_t block);
static void apply_model(nano_fx_block_t *block, const nano_fx_model_def_t *model);

static const char *s_block_labels[NANO_FX_BLOCK_COUNT] = {
    "Pre FX 1", "Pre FX 2", "Post FX 1", "Post FX 2", "Post FX 3",
};

const nano_fx_model_def_t *nano_fx_find_model(const uint8_t *id, uint8_t id_len);

static nano_fx_model_def_t s_generic_model;
static const char *s_generic_param_ptrs[NANO_FX_MAX_PARAMS];
static char s_generic_param_names[NANO_FX_MAX_PARAMS][12];

static const nano_fx_model_def_t *find_model_in_blob(const uint8_t *raw, size_t raw_len)
{
    if (!raw || raw_len == 0) {
        return NULL;
    }

    const nano_fx_model_def_t *direct = nano_fx_find_model(raw, (uint8_t)raw_len);
    if (direct) {
        return direct;
    }

    for (size_t c = 0; c < nano_fx_catalog_count; c++) {
        const nano_fx_model_def_t *entry = &nano_fx_catalog[c];
        if (entry->id_len == 0 || raw_len < entry->id_len) {
            continue;
        }
        for (size_t off = 0; off + entry->id_len <= raw_len; off++) {
            if (memcmp(raw + off, entry->id, entry->id_len) == 0) {
                return entry;
            }
        }
    }

    return NULL;
}

static void set_generic_model(nano_fx_block_t *block, uint8_t count)
{
    if (count == 0) {
        return;
    }
    if (count > NANO_FX_MAX_PARAMS) {
        count = NANO_FX_MAX_PARAMS;
    }

    for (uint8_t i = 0; i < count; i++) {
        snprintf(s_generic_param_names[i], sizeof(s_generic_param_names[i]), "Param %u", (unsigned)(i + 1));
        s_generic_param_ptrs[i] = s_generic_param_names[i];
        block->params[i].display = 50.0f;
        block->params[i].normalized = 0.5f;
        block->params[i].valid = false;
    }

    s_generic_model.name = "FX";
    s_generic_model.id_len = 0;
    s_generic_model.param_count = count;
    s_generic_model.param_names = s_generic_param_ptrs;
    block->model = &s_generic_model;
    block->param_count = count;
}

static void log_model_id_hex(const char *label, const uint8_t *raw, uint8_t len)
{
    char hex[24];
    size_t pos = 0;
    for (uint8_t i = 0; i < len && pos + 3 < sizeof(hex); i++) {
        pos += (size_t)snprintf(hex + pos, sizeof(hex) - pos, "%02X", raw[i]);
    }
    ESP_LOGI(TAG, "%s model field raw: %s", label, hex);
}

static float clampf(float v, float lo, float hi)
{
    if (v < lo) {
        return lo;
    }
    if (v > hi) {
        return hi;
    }
    return v;
}

static bool id_match(const uint8_t *a, uint8_t alen, const uint8_t *b, uint8_t blen)
{
    return alen == blen && alen > 0 && memcmp(a, b, alen) == 0;
}

const nano_fx_model_def_t *nano_fx_find_model(const uint8_t *id, uint8_t id_len)
{
    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (id_match(id, id_len, nano_fx_catalog[i].id, nano_fx_catalog[i].id_len)) {
            return &nano_fx_catalog[i];
        }
    }
    return NULL;
}

static const nano_fx_model_def_t *find_model_by_name(const char *name)
{
    if (!name) {
        return NULL;
    }
    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (strcmp(nano_fx_catalog[i].name, name) == 0) {
            return &nano_fx_catalog[i];
        }
    }
    return NULL;
}

static void store_catalog_model_id(nano_fx_block_t *block, const nano_fx_model_def_t *model)
{
    if (!block || !model || model->id_len == 0) {
        return;
    }
    uint8_t copy_len = model->id_len > sizeof(block->model_id_raw) ? (uint8_t)sizeof(block->model_id_raw)
                                                                   : model->id_len;
    memcpy(block->model_id_raw, model->id, copy_len);
    block->model_id_len = copy_len;
    block->has_model_id = true;
}

/** Map firmware preset varints that are not catalog wire IDs (e.g. 25 = Transpose). */
static const nano_fx_model_def_t *resolve_legacy_model(nano_fx_block_id_t block, uint8_t val)
{
    const nano_fx_model_def_t *direct = nano_fx_find_model(&val, 1);
    if (direct) {
        return direct;
    }

    static const struct {
        nano_fx_block_id_t block;
        uint8_t val;
        const char *name;
    } firmware_enum[] = {
        { NANO_FX_PRE1, 25, "Transpose" },
        { NANO_FX_POST1, 15, "Doubler" },
    };

    for (size_t i = 0; i < sizeof(firmware_enum) / sizeof(firmware_enum[0]); i++) {
        if (firmware_enum[i].block == block && firmware_enum[i].val == val) {
            return find_model_by_name(firmware_enum[i].name);
        }
    }

    return NULL;
}

static const nano_fx_model_def_t *find_model_id_in_preset(const uint8_t *data, size_t len,
                                                           const uint8_t *id, uint8_t id_len)
{
    if (!data || !id || id_len == 0 || len < id_len) {
        return NULL;
    }

    for (size_t i = 0; i + id_len <= len; i++) {
        if (memcmp(data + i, id, id_len) == 0) {
            return nano_fx_find_model(id, id_len);
        }
    }

    return NULL;
}

static bool try_apply_slot_exclusive_model(const uint8_t *data, size_t len, int block_idx,
                                           nano_fx_block_t *block)
{
    static const struct {
        nano_fx_block_id_t block;
        const uint8_t id[3];
        uint8_t id_len;
        const char *name;
    } exclusive[] = {
        { NANO_FX_PRE1, { 0xD1, 0x8C, 0x01 }, 3, "Transpose" },
        { NANO_FX_POST1, { 0x8B, 0x7D }, 2, "Doubler" },
    };

    for (size_t i = 0; i < sizeof(exclusive) / sizeof(exclusive[0]); i++) {
        if ((nano_fx_block_id_t)block_idx != exclusive[i].block) {
            continue;
        }

        const nano_fx_model_def_t *model =
            find_model_id_in_preset(data, len, exclusive[i].id, exclusive[i].id_len);
        if (!model) {
            model = find_model_by_name(exclusive[i].name);
        }
        if (!model) {
            continue;
        }

        block->id = (nano_fx_block_id_t)block_idx;
        block->valid = true;
        store_catalog_model_id(block, model);
        apply_model(block, model);
        ESP_LOGI(TAG, "%s model: %s (slot ID)", s_block_labels[block_idx], model->name);
        return true;
    }

    return false;
}

const char *nano_fx_block_label(nano_fx_block_id_t block)
{
    if (block >= NANO_FX_BLOCK_COUNT) {
        return "?";
    }
    return s_block_labels[block];
}

static nano_fx_category_t category_from_catalog_index(size_t idx)
{
    if (idx <= 10) {
        return (idx == 3 || idx == 7) ? NANO_FX_CAT_BASS_OD : NANO_FX_CAT_GUITAR_OD;
    }
    if (idx == 11 || idx == 15 || idx == 16) {
        return NANO_FX_CAT_UTILITY;
    }
    if ((idx >= 12 && idx <= 14) || (idx >= 21 && idx <= 22)) {
        return NANO_FX_CAT_FILTER;
    }
    if (idx >= 17 && idx <= 20) {
        return NANO_FX_CAT_WAH;
    }
    if ((idx >= 23 && idx <= 26) || (idx >= 29 && idx <= 32)) {
        return NANO_FX_CAT_COMPRESSOR;
    }
    if (idx >= 27 && idx <= 28) {
        return NANO_FX_CAT_PITCH;
    }
    if (idx >= 33 && idx <= 40) {
        return NANO_FX_CAT_MODULATION;
    }
    if (idx >= 41 && idx <= 46) {
        return NANO_FX_CAT_DELAY;
    }
    if (idx >= 47 && idx <= 52) {
        return NANO_FX_CAT_REVERB;
    }
    return NANO_FX_CAT_NONE;
}

nano_fx_category_t nano_fx_model_category(const nano_fx_model_def_t *model)
{
    if (!model) {
        return NANO_FX_CAT_NONE;
    }

    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (&nano_fx_catalog[i] == model) {
            return category_from_catalog_index(i);
        }
    }

    if (model->name) {
        if (strstr(model->name, "Wah") != NULL) {
            return NANO_FX_CAT_WAH;
        }
        if (strstr(model->name, "Comp") != NULL) {
            return NANO_FX_CAT_COMPRESSOR;
        }
        if (strstr(model->name, "Filter") != NULL || strstr(model->name, "EQ") != NULL ||
            strstr(model->name, "Graphic") != NULL || strstr(model->name, "Parametric") != NULL) {
            return NANO_FX_CAT_FILTER;
        }
        if (strstr(model->name, "Transpose") != NULL || strstr(model->name, "Doubler") != NULL ||
            strstr(model->name, "Pitch") != NULL) {
            return NANO_FX_CAT_PITCH;
        }
        if (strstr(model->name, "Bass") != NULL) {
            return NANO_FX_CAT_BASS_OD;
        }
        if (strstr(model->name, "Volume") != NULL || strstr(model->name, "Gate") != NULL) {
            return NANO_FX_CAT_UTILITY;
        }
        if (strstr(model->name, "Delay") != NULL) {
            return NANO_FX_CAT_DELAY;
        }
        if (strstr(model->name, "Reverb") != NULL || strstr(model->name, "Hall") != NULL ||
            strstr(model->name, "Room") != NULL || strstr(model->name, "Cave") != NULL ||
            strstr(model->name, "Ambience") != NULL) {
            return NANO_FX_CAT_REVERB;
        }
        if (strstr(model->name, "Chorus") != NULL || strstr(model->name, "Flanger") != NULL ||
            strstr(model->name, "Phase") != NULL || strstr(model->name, "Tremolo") != NULL ||
            strstr(model->name, "Vibe") != NULL) {
            return NANO_FX_CAT_MODULATION;
        }
    }

    return NANO_FX_CAT_GUITAR_OD;
}

const char *nano_fx_category_label(nano_fx_category_t cat)
{
    switch (cat) {
    case NANO_FX_CAT_GUITAR_OD:
        return "Guitar OD";
    case NANO_FX_CAT_BASS_OD:
        return "Bass OD";
    case NANO_FX_CAT_COMPRESSOR:
        return "Compressor";
    case NANO_FX_CAT_PITCH:
        return "Pitch";
    case NANO_FX_CAT_FILTER:
        return "Filter";
    case NANO_FX_CAT_WAH:
        return "Wah";
    case NANO_FX_CAT_UTILITY:
        return "Utility";
    case NANO_FX_CAT_MODULATION:
        return "Modulation";
    case NANO_FX_CAT_DELAY:
        return "Delay";
    case NANO_FX_CAT_REVERB:
        return "Reverb";
    default:
        return "-";
    }
}

void nano_fx_block_model_text(const nano_fx_block_t *block, char *buf, size_t cap)
{
    if (!buf || cap == 0) {
        return;
    }

    if (block && block->has_model_id && block->model_id_len >= 2) {
        const nano_fx_model_def_t *from_id =
            find_model_in_blob(block->model_id_raw, block->model_id_len);
        if (from_id && from_id->name) {
            snprintf(buf, cap, "%s", from_id->name);
            return;
        }
    }

    if (block && block->model && block->model->name) {
        snprintf(buf, cap, "%s", block->model->name);
        return;
    }

    if (block && block->has_model_id && block->model_id_len > 0) {
        char hex[24];
        size_t pos = 0;
        for (uint8_t i = 0; i < block->model_id_len && pos + 3 < sizeof(hex); i++) {
            pos += (size_t)snprintf(hex + pos, sizeof(hex) - pos, "%02X", block->model_id_raw[i]);
        }
        snprintf(buf, cap, "ID %s", hex);
        return;
    }

    snprintf(buf, cap, "Empty slot");
}

static const struct {
    const char *name;
    const char *abbr;
} s_chain_abbr[] = {
    { "Chief BD2", "BD2" },
    { "Chief OD1", "OD1" },
    { "Exotic", "EXOTIC" },
    { "Exotic Bass Z Boost", "BASS-Z" },
    { "Exotic Z Boost", "ZBOOST" },
    { "Facial Fuzz", "FUZZ" },
    { "Green 808", "GRN808" },
    { "Microtubes B3K", "B3K" },
    { "OD250", "OD250" },
    { "Obsessive Drive", "OBSESS" },
    { "Rodent Drive", "RODENT" },
    { "Adaptive Gate", "A.GATE" },
    { "Graphic 9", "GRAPH9" },
    { "Low-High Cut", "LO-HI" },
    { "Parametric 3", "PARAM3" },
    { "Utility Gate", "U.GATE" },
    { "Volume", "VOLUME" },
    { "Bass Wah", "BASSW" },
    { "Bubba Wah", "BUBBA" },
    { "Crying Clyde Wah", "CLYDE" },
    { "Crying Wah", "C.WAH" },
    { "Envelope Filter", "ENVELOP" },
    { "Love Meat", "LOVE" },
    { "Legendary 87 (M)", "LEG87M" },
    { "Opto Comp (M)", "OPTO-M" },
    { "Solid State Comp (M)", "SSD-M" },
    { "VCA Comp (M)", "VCA-M" },
    { "Transpose", "TRANS" },
    { "Doubler", "DOUBLE" },
    { "Legendary 87 (ST)", "LEG87" },
    { "Opto Comp (ST)", "OPTO" },
    { "Solid State Comp (ST)", "SSD" },
    { "VCA Comp (ST)", "VCA" },
    { "Chief CE2W (ST)", "CE2W" },
    { "Chief DC2W (ST)", "DC2W" },
    { "Chorus 229T", "CHR229" },
    { "Dream Chorus", "DREAM" },
    { "MX Flanger", "FLANG" },
    { "MX Phase 95", "PHASE95" },
    { "MX Vibe", "VIBE" },
    { "Tremolo", "TREMO" },
    { "Analog Delay", "ANALOG" },
    { "Circular Delay", "CIRC" },
    { "Digital Delay (ST)", "DIGDLY" },
    { "Dual Delay", "DUAL" },
    { "Dual Reverse Delay", "DREV" },
    { "Tape Delay", "TAPE" },
    { "Ambience", "AMBIEN" },
    { "Cave", "CAVE" },
    { "Hall", "HALL" },
    { "Mind Hall", "MINDH" },
    { "Modulated", "MODREV" },
    { "Room", "ROOM" },
};

static bool is_skip_chain_word(const char *word)
{
    static const char *const skip[] = {
        "Chief", "MX", "Exotic", "Microtubes", "Solid", "State", "Legendary", "Digital", "Dual",
        "Utility", "Adaptive", "Crying",
    };
    for (size_t i = 0; i < sizeof(skip) / sizeof(skip[0]); i++) {
        if (strcmp(word, skip[i]) == 0) {
            return true;
        }
    }
    return false;
}

static bool token_has_digit(const char *s)
{
    if (!s) {
        return false;
    }
    for (; *s; s++) {
        if (isdigit((unsigned char)*s)) {
            return true;
        }
    }
    return false;
}

static void strip_model_parens(const char *in, char *out, size_t cap)
{
    if (!in || !out || cap == 0) {
        return;
    }

    size_t n = 0;
    for (size_t i = 0; in[i] && n + 1 < cap; i++) {
        if (in[i] == '(') {
            while (in[i] && in[i] != ')') {
                i++;
            }
            continue;
        }
        out[n++] = in[i];
    }
    while (n > 0 && out[n - 1] == ' ') {
        n--;
    }
    out[n] = '\0';
}

static int split_chain_tokens(const char *text, char tokens[][16], int max_tokens)
{
    int count = 0;
    const char *p = text;

    while (*p && count < max_tokens) {
        while (*p == ' ' || *p == '-') {
            p++;
        }
        if (!*p) {
            break;
        }

        size_t n = 0;
        while (*p && *p != ' ' && n + 1 < sizeof(tokens[0])) {
            tokens[count][n++] = *p++;
        }
        tokens[count][n] = '\0';
        count++;
    }

    return count;
}

static void uppercase_chain_abbr(char *s)
{
    if (!s) {
        return;
    }
    for (; *s; s++) {
        *s = (char)toupper((unsigned char)*s);
    }
}

static bool chain_abbr_from_name(const char *name, char *out, size_t cap)
{
    if (!name || !out || cap < 2) {
        return false;
    }

    for (size_t i = 0; i < sizeof(s_chain_abbr) / sizeof(s_chain_abbr[0]); i++) {
        if (strcmp(name, s_chain_abbr[i].name) == 0) {
            snprintf(out, cap, "%s", s_chain_abbr[i].abbr);
            return true;
        }
    }

    char clean[56];
    strip_model_parens(name, clean, sizeof(clean));

    char tokens[8][16];
    int nt = split_chain_tokens(clean, tokens, 8);
    if (nt <= 0) {
        return false;
    }

    int start = 0;
    while (start < nt && is_skip_chain_word(tokens[start])) {
        start++;
    }
    if (start >= nt) {
        start = 0;
    }

    if (nt - start == 1) {
        const char *t = tokens[start];
        size_t n = 0;
        for (; t[n] && n + 1 < cap && n < 10; n++) {
            out[n] = (char)toupper((unsigned char)t[n]);
        }
        out[n] = '\0';
        return n > 0;
    }

    int digit_idx = -1;
    for (int i = nt - 1; i >= start; i--) {
        if (token_has_digit(tokens[i])) {
            digit_idx = i;
            break;
        }
    }

    if (digit_idx > start) {
        const char *prev = tokens[digit_idx - 1];
        const char *num = tokens[digit_idx];
        if (!token_has_digit(prev)) {
            out[0] = (char)toupper((unsigned char)prev[0]);
            snprintf(out + 1, cap - 1, "%s", num);
            uppercase_chain_abbr(out);
            return true;
        }
    }

    if (digit_idx >= start) {
        snprintf(out, cap, "%.10s", tokens[digit_idx]);
        uppercase_chain_abbr(out);
        return true;
    }

    if (nt - start == 2) {
        const char *w1 = tokens[start];
        const char *w2 = tokens[start + 1];
        size_t w2len = strlen(w2);
        out[0] = (char)toupper((unsigned char)w1[0]);
        out[1] = (char)toupper((unsigned char)w2[0]);
        out[2] = (char)toupper((unsigned char)w2[w2len > 0 ? w2len - 1 : 0]);
        out[3] = '\0';
        return true;
    }

    size_t n = 0;
    for (int i = start; i < nt && n + 1 < cap && n < 8; i++) {
        if (tokens[i][0]) {
            out[n++] = (char)toupper((unsigned char)tokens[i][0]);
        }
    }
    out[n] = '\0';
    return n > 0;
}

void nano_fx_model_chain_acronym(const nano_fx_block_t *block, char *buf, size_t cap)
{
    if (!buf || cap == 0) {
        return;
    }

    buf[0] = '\0';
    if (!block || !block->valid) {
        snprintf(buf, cap, "-");
        return;
    }

    char name[56];
    nano_fx_block_model_text(block, name, sizeof(name));
    if (strcmp(name, "Empty slot") == 0 || strncmp(name, "ID ", 3) == 0) {
        snprintf(buf, cap, "-");
        return;
    }

    if (!chain_abbr_from_name(name, buf, cap)) {
        snprintf(buf, cap, "FX");
    }
}

static bool name_has_enum_hint(const char *name)
{
    return name && (strstr(name, "(On/Off)") || strstr(name, "/") || strstr(name, "(HB/Single)"));
}

bool nano_fx_param_is_enum(const char *param_name)
{
    return name_has_enum_hint(param_name);
}

int nano_fx_param_enum_count(const char *param_name)
{
    if (!param_name) {
        return 2;
    }
    if (strstr(param_name, "Sync Note")) {
        return 21;
    }
    if (strstr(param_name, "Dyn Mode")) {
        return 3;
    }
    if (strstr(param_name, "Type (Peak")) {
        return 5;
    }
    if (strstr(param_name, "Waveform")) {
        return 4;
    }
    if (strstr(param_name, "Tap Preset")) {
        return 4;
    }
    if (strstr(param_name, "Curve (Linear/Log)")) {
        return 2;
    }
    if (strstr(param_name, "Peak (LP/HP)")) {
        return 2;
    }
    if (strstr(param_name, "Pickup (HB/Single)")) {
        return 2;
    }
    if (strstr(param_name, "Feedback Mode")) {
        return 2;
    }
    return 2;
}

const char *nano_fx_param_enum_label(const char *param_name, int index)
{
    if (!param_name) {
        return "?";
    }

    if (strstr(param_name, "Dyn Mode")) {
        static const char *labels[] = {"Off", "Duck", "Gate"};
        if (index >= 0 && index < 3) {
            return labels[index];
        }
        return "?";
    }

    if (strstr(param_name, "Curve (Linear/Log)")) {
        return index == 0 ? "Linear" : "Log";
    }

    if (strstr(param_name, "Peak (LP/HP)")) {
        return index == 0 ? "LP" : "HP";
    }

    if (strstr(param_name, "Pickup (HB/Single)")) {
        return index == 0 ? "HB" : "Single";
    }

    if (strstr(param_name, "On/Off") || strstr(param_name, "Sync (") || strstr(param_name, "Sync L") ||
        strstr(param_name, "Sync R") || strstr(param_name, "Ping Pong") ||
        strstr(param_name, "Vintage Mode") || strstr(param_name, "Link FBack") ||
        strstr(param_name, "Growl") || strstr(param_name, "Midboost") ||
        strstr(param_name, "Active (On/Off)")) {
        return index == 0 ? "Off" : "On";
    }

    static char buf[8];
    snprintf(buf, sizeof(buf), "%d", index);
    return buf;
}

void nano_fx_param_range(const char *param_name, float *min_out, float *max_out, float *step_out)
{
    bool is_pct;
    fx_param_range_impl(param_name, min_out, max_out, step_out, &is_pct);
}

int nano_fx_param_scale(const char *param_name)
{
    if (nano_fx_param_is_enum(param_name)) {
        return 1;
    }

    float min_v, max_v, step;
    bool is_pct;
    fx_param_range_impl(param_name, &min_v, &max_v, &step, &is_pct);

    if (step >= 1.0f) {
        return 1;
    }
    if (step >= 0.1f) {
        return 10;
    }
    return 100;
}

int nano_fx_display_to_slider(const char *param_name, float display)
{
    float min_v, max_v, step;
    nano_fx_param_range(param_name, &min_v, &max_v, &step);
    int scale = nano_fx_param_scale(param_name);
    int raw = (int)((display - min_v) * (float)scale + 0.5f);
    int max_slider = (int)((max_v - min_v) * (float)scale + 0.5f);
    if (raw < 0) {
        return 0;
    }
    if (raw > max_slider) {
        return max_slider;
    }
    return raw;
}

float nano_fx_slider_to_display(const char *param_name, int slider_value)
{
    float min_v, max_v, step;
    nano_fx_param_range(param_name, &min_v, &max_v, &step);
    int scale = nano_fx_param_scale(param_name);
    return min_v + (float)slider_value / (float)scale;
}

int nano_fx_slider_min(const char *param_name)
{
    return 0;
}

int nano_fx_slider_max(const char *param_name)
{
    float min_v, max_v, step;
    nano_fx_param_range(param_name, &min_v, &max_v, &step);
    return nano_fx_display_to_slider(param_name, max_v);
}

static void fx_param_range_impl(const char *name, float *min_out, float *max_out, float *step_out,
                                bool *is_pct)
{
    *min_out = 0.0f;
    *max_out = 100.0f;
    *step_out = 1.0f;
    *is_pct = true;

    if (!name) {
        return;
    }

    if (nano_fx_param_is_enum(name)) {
        *max_out = (float)(nano_fx_param_enum_count(name) - 1);
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Threshold") || (strstr(name, "Range") && !strstr(name, "Freq"))) {
        *min_out = -90.0f;
        *max_out = 0.0f;
        *step_out = 0.1f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Dry Level") || strstr(name, "FX Level")) {
        *min_out = -40.0f;
        *max_out = 12.0f;
        *step_out = 0.1f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Spread")) {
        *min_out = -100.0f;
        *max_out = 100.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Semitones")) {
        *min_out = -12.0f;
        *max_out = 12.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Delay Time") || strstr(name, "Time L") || strstr(name, "Time R")) {
        *min_out = 50.0f;
        *max_out = 2000.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Attack") || strstr(name, "Release") || strstr(name, "Hold")) {
        *min_out = 1.0f;
        *max_out = 2000.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Pre Delay")) {
        *min_out = 0.0f;
        *max_out = 500.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Freq") && !strstr(name, "High Pass") && !strstr(name, "Low Pass")) {
        *min_out = 20.0f;
        *max_out = 20000.0f;
        *step_out = 1.0f;
        *is_pct = false;
        return;
    }

    if (strstr(name, "Hz")) {
        *min_out = -12.0f;
        *max_out = 12.0f;
        *step_out = 0.5f;
        *is_pct = false;
    }
}

static const char *semitone_interval_abbr(int semitones)
{
    switch (abs(semitones)) {
    case 0:
        return "Unison";
    case 1:
        return "m2nd";
    case 2:
        return "M2nd";
    case 3:
        return "m3rd";
    case 4:
        return "M3rd";
    case 5:
        return "P4th";
    case 6:
        return "TT";
    case 7:
        return "P5th";
    case 8:
        return "m6th";
    case 9:
        return "M6th";
    case 10:
        return "m7th";
    case 11:
        return "M7th";
    case 12:
        return "Oct";
    default:
        return NULL;
    }
}

static void format_semitone_param(float display, char *buf, size_t cap)
{
    int semi = (int)(display + (display >= 0.f ? 0.5f : -0.5f));
    const char *interval = semitone_interval_abbr(semi);

    if (interval) {
        if (semi > 0) {
            snprintf(buf, cap, "+%d (%s)", semi, interval);
        } else if (semi < 0) {
            snprintf(buf, cap, "%d (%s)", semi, interval);
        } else {
            snprintf(buf, cap, "0 (%s)", interval);
        }
        return;
    }

    snprintf(buf, cap, "%d", semi);
}

float nano_fx_display_to_normalized(const char *param_name, float display)
{
    float min_v, max_v, step;
    bool is_pct;
    fx_param_range_impl(param_name, &min_v, &max_v, &step, &is_pct);

    if (max_v <= min_v) {
        return 0.0f;
    }

    return clampf((display - min_v) / (max_v - min_v), 0.0f, 1.0f);
}

float nano_fx_normalized_to_display(const char *param_name, float normalized)
{
    float min_v, max_v, step;
    bool is_pct;
    fx_param_range_impl(param_name, &min_v, &max_v, &step, &is_pct);
    return min_v + clampf(normalized, 0.0f, 1.0f) * (max_v - min_v);
}

void nano_fx_format_param(const nano_fx_block_t *block, uint8_t param_index, char *buf, size_t cap)
{
    if (!buf || cap == 0) {
        return;
    }

    if (!block || !block->model || param_index >= block->param_count) {
        snprintf(buf, cap, "-");
        return;
    }

    const nano_fx_param_t *p = &block->params[param_index];
    if (!p->valid) {
        snprintf(buf, cap, "-");
        return;
    }

    const char *name = block->model->param_names[param_index];
    if (strstr(name, "Semitones")) {
        format_semitone_param(p->display, buf, cap);
        return;
    }

    if (nano_fx_param_is_enum(name)) {
        int idx = (int)(p->display + 0.5f);
        int count = nano_fx_param_enum_count(name);
        if (idx < 0) {
            idx = 0;
        }
        if (idx >= count) {
            idx = count - 1;
        }
        snprintf(buf, cap, "%s", nano_fx_param_enum_label(name, idx));
        return;
    }

    float min_v, max_v, step;
    bool is_pct;
    fx_param_range_impl(name, &min_v, &max_v, &step, &is_pct);

    int decimals = (step >= 1.0f) ? 0 : 1;
    if (is_pct && min_v == 0.0f && max_v == 100.0f) {
        snprintf(buf, cap, "%.*f%%", decimals, p->display);
    } else {
        snprintf(buf, cap, "%.*f", decimals, p->display);
    }
}

static void reset_block_params(nano_fx_block_t *block)
{
    block->params_loaded = false;
    block->param_count = 0;
    memset(block->params, 0, sizeof(block->params));
}

static void apply_model(nano_fx_block_t *block, const nano_fx_model_def_t *model)
{
    if (block->model == model && block->model != NULL && block->param_count > 0) {
        return;
    }

    block->model = model;
    reset_block_params(block);

    if (model) {
        block->param_count = model->param_count;
        for (uint8_t i = 0; i < model->param_count && i < NANO_FX_MAX_PARAMS; i++) {
            block->params[i].display = nano_fx_normalized_to_display(model->param_names[i], 0.5f);
            block->params[i].normalized = 0.5f;
            block->params[i].valid = false;
        }
    }
}

static bool block_needs_refresh(const nano_fx_block_t *block)
{
    return block && block->on && !block->params_loaded;
}

static bool block_model_write_recent(nano_fx_block_id_t block)
{
    if (block >= NANO_FX_BLOCK_COUNT || s_block_model_write_us[block] == 0) {
        return false;
    }
    return esp_timer_get_time() - s_block_model_write_us[block] < FX_MODEL_WRITE_ASSERT_US;
}

/** Preset 1-byte firmware enums often lag behind a BLE model select. */
static bool preset_model_stale_legacy(const nano_fx_block_t *local, const nano_fx_block_t *preset)
{
    if (!local || !preset || !local->model || local->model_id_len < 2 || preset->model_id_len != 1) {
        return false;
    }
    return local->params_loaded || block_model_write_recent(local->id);
}

/** Extract FX model ID from preset field (wire-2 bytes and 1–3 byte varint raw). */
static bool extract_model_field(const uint8_t *data, size_t len, uint32_t field_num,
                                const uint8_t **out_raw, size_t *out_len)
{
    const uint8_t *first_raw = NULL;
    size_t first_len = 0;
    const uint8_t *best_wire2 = NULL;
    size_t best_wire2_len = 0;
    const uint8_t *catalog_raw = NULL;
    size_t catalog_len = 0;

    size_t i = 0;
    while (i < len) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag >> 3;
        uint32_t wire = tag & 0x07;

        if (field == field_num) {
            if (wire == 0) {
                size_t raw_start = i;
                uint32_t val = 0;
                if (!nano_read_varint(data, len, i, &val, &next)) {
                    break;
                }
                size_t raw_len = next - raw_start;
                const uint8_t *raw = data + raw_start;

                if (!first_raw && raw_len > 0) {
                    first_raw = raw;
                    first_len = raw_len;
                }

                /* Field 51 wire-0 is the tuner-off marker, not a model ID. */
                if (field_num != 51 && raw_len > 0 && raw_len <= 3 && find_model_in_blob(raw, raw_len)) {
                    catalog_raw = raw;
                    catalog_len = raw_len;
                }
                i = next;
                continue;
            }

            if (wire == 2) {
                uint32_t flen = 0;
                if (!nano_read_varint(data, len, i, &flen, &next)) {
                    break;
                }
                if (next + flen > len) {
                    break;
                }
                const uint8_t *raw = data + next;

                if (!first_raw && flen > 0) {
                    first_raw = raw;
                    first_len = flen;
                }
                if (find_model_in_blob(raw, flen)) {
                    catalog_raw = raw;
                    catalog_len = flen;
                }
                if (flen > 0) {
                    best_wire2 = raw;
                    best_wire2_len = flen;
                }
                i = next + flen;
                continue;
            }
        }

        if (wire == 0) {
            uint32_t skip = 0;
            if (!nano_read_varint(data, len, i, &skip, &next)) {
                break;
            }
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                break;
            }
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            i = next + flen;
            if (i > len) {
                break;
            }
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }

    if (catalog_raw && catalog_len > 0) {
        *out_raw = catalog_raw;
        *out_len = catalog_len;
        return true;
    }
    if (best_wire2 && best_wire2_len > 0) {
        *out_raw = best_wire2;
        *out_len = best_wire2_len;
        return true;
    }
    if (first_raw && first_len > 0 && field_num != 51) {
        *out_raw = first_raw;
        *out_len = first_len;
        return true;
    }
    return false;
}

static void try_apply_catalog_model(nano_fx_block_t *block);

static bool extract_field_varint(const uint8_t *data, size_t len, uint32_t field_num, uint32_t *out_val);
static bool extract_field_fixed32_float(const uint8_t *data, size_t len, uint32_t field_num, float *out_val);

/** Scan raw dump for gate amount (field 53 fixed32) even if earlier fields are truncated. */
static bool scan_gate_amount_field53(const uint8_t *data, size_t len, float *out_val)
{
    /* Tag for field 53 wire5 = (53<<3)|5 = 429 = AD 03 */
    if (!data || !out_val || len < 6) {
        return false;
    }
    for (size_t i = 0; i + 6 <= len; i++) {
        if (data[i] != 0xAD || data[i + 1] != 0x03) {
            continue;
        }
        union {
            uint32_t u;
            float f;
        } u;
        u.u = (uint32_t)data[i + 2] | ((uint32_t)data[i + 3] << 8) | ((uint32_t)data[i + 4] << 16) |
              ((uint32_t)data[i + 5] << 24);
        if (!isfinite(u.f) || u.f < 0.0f || u.f > 1.0f) {
            continue;
        }
        *out_val = u.f;
        return true;
    }
    return false;
}

/** Scan raw dump for gate bypass (field 54 varint 0/1) even if earlier fields are truncated. */
static bool scan_gate_bypass_field54(const uint8_t *data, size_t len, uint32_t *out_val)
{
    /* Tag for field 54 wire0 = (54<<3)|0 = 432 = B0 03 */
    if (!data || !out_val || len < 3) {
        return false;
    }
    for (size_t i = 0; i + 3 <= len; i++) {
        if (data[i] != 0xB0 || data[i + 1] != 0x03) {
            continue;
        }
        uint32_t val = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i + 2, &val, &next)) {
            continue;
        }
        if (val > 1U) {
            continue;
        }
        *out_val = val;
        return true;
    }
    return false;
}

static bool is_reverb_model(const nano_fx_model_def_t *m)
{
    if (!m || !m->name) {
        return false;
    }
    return strstr(m->name, "Hall") != NULL || strstr(m->name, "Room") != NULL ||
           strstr(m->name, "Cave") != NULL || strstr(m->name, "Ambience") != NULL ||
           strstr(m->name, "Modulated") != NULL;
}

static bool is_delay_model(const nano_fx_model_def_t *m)
{
    return m && m->name && strstr(m->name, "Delay") != NULL;
}

static bool is_modulation_model(const nano_fx_model_def_t *m)
{
    if (!m || !m->name) {
        return false;
    }
    return strstr(m->name, "Chorus") != NULL || strstr(m->name, "Phaser") != NULL ||
           strstr(m->name, "Flanger") != NULL || strstr(m->name, "Tremolo") != NULL ||
           strstr(m->name, "Vibe") != NULL || strstr(m->name, "CE2") != NULL ||
           strstr(m->name, "DC2") != NULL;
}

/** Post FX 2 is delay-only (NanOS 2.0); scan preset for delay wire IDs. */
static const nano_fx_model_def_t *find_post2_delay_in_preset(const uint8_t *data, size_t len)
{
    static const struct {
        uint8_t id[2];
        const char *name;
    } delays[] = {
        { { 0xFA, 0x2E }, "Analog Delay" },
        { { 0xFF, 0x2E }, "Circular Delay" },
        { { 0xF4, 0x2E }, "Tape Delay" },
        { { 0xFB, 0x2E }, "Digital Delay (ST)" },
        { { 0xFC, 0x2E }, "Dual Delay" },
        { { 0xFE, 0x2E }, "Dual Reverse Delay" },
    };

    for (size_t d = 0; d < sizeof(delays) / sizeof(delays[0]); d++) {
        if (find_model_id_in_preset(data, len, delays[d].id, 2)) {
            return find_model_by_name(delays[d].name);
        }
    }

    return NULL;
}

static const nano_fx_model_def_t *infer_post2_delay(uint8_t param_count)
{
    switch (param_count) {
    case 11:
        return find_model_by_name("Tape Delay");
    case 12:
        return find_model_by_name("Analog Delay");
    case 18:
        return find_model_by_name("Digital Delay (ST)");
    case 22:
        return find_model_by_name("Dual Delay");
    default:
        return NULL;
    }
}

static const nano_fx_model_def_t *infer_model_by_params(nano_fx_block_id_t block, uint8_t param_count)
{
    if (block == NANO_FX_PRE1 && param_count == 5) {
        return find_model_by_name("Transpose");
    }

    if (block == NANO_FX_POST1) {
        if (param_count == 3 || param_count == 4) {
            return find_model_by_name("Doubler");
        }
        if (param_count == 5) {
            return find_model_by_name("Chief DC2W (ST)");
        }
    }

    if (block == NANO_FX_POST2) {
        return infer_post2_delay(param_count);
    }

    const nano_fx_model_def_t *found = NULL;
    size_t matches = 0;

    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        const nano_fx_model_def_t *m = &nano_fx_catalog[i];
        if (m->param_count != param_count) {
            continue;
        }

        bool ok = false;
        switch (block) {
        case NANO_FX_POST3:
            ok = is_reverb_model(m);
            break;
        case NANO_FX_POST2:
            ok = is_delay_model(m);
            break;
        case NANO_FX_POST1:
            ok = is_modulation_model(m) || param_count <= 8;
            break;
        case NANO_FX_PRE1:
        case NANO_FX_PRE2:
            ok = !is_reverb_model(m) && !is_delay_model(m) && param_count <= 12;
            break;
        default:
            break;
        }

        if (!ok) {
            continue;
        }

        found = m;
        matches++;
    }

    if (matches == 1) {
        return found;
    }

    if (block == NANO_FX_POST3 && is_reverb_model(found)) {
        return found;
    }

    return NULL;
}

static void upgrade_block_model(nano_fx_block_t *block, const nano_fx_model_def_t *model,
                                const float *floats, size_t float_count)
{
    if (!block || !model || block->model == model) {
        return;
    }

    block->model = model;
    block->param_count = model->param_count;

    uint8_t n = model->param_count;
    if ((uint8_t)float_count < n) {
        n = (uint8_t)float_count;
    }

    for (uint8_t i = 0; i < n && i < NANO_FX_MAX_PARAMS; i++) {
        block->params[i].normalized = clampf(floats[i], 0.0f, 1.0f);
        block->params[i].display =
            nano_fx_normalized_to_display(model->param_names[i], block->params[i].normalized);
        block->params[i].valid = true;
    }

    for (uint8_t i = n; i < model->param_count && i < NANO_FX_MAX_PARAMS; i++) {
        block->params[i].display = nano_fx_normalized_to_display(model->param_names[i], 0.5f);
        block->params[i].normalized = 0.5f;
        block->params[i].valid = false;
    }
}

static bool try_parse_legacy_model_field(const uint8_t *data, size_t len, int block_idx,
                                         nano_fx_block_t *block)
{
    if (block_idx < 0 || block_idx >= NANO_FX_BLOCK_COUNT) {
        return false;
    }

    const uint32_t fields[] = {
        s_model_field_legacy[block_idx],
        s_model_field_legacy_alt[block_idx],
    };
    const size_t field_count = s_model_field_legacy_alt[block_idx] ? 2 : 1;

    for (size_t fi = 0; fi < field_count; fi++) {
        uint32_t field = fields[fi];
        if (field == 0) {
            continue;
        }

        uint32_t val = 0;
        if (!extract_field_varint(data, len, field, &val) || val == 0 || val > 0xFF) {
            continue;
        }

        block->model_id_raw[0] = (uint8_t)val;
        block->model_id_len = 1;
        block->has_model_id = true;

        log_model_id_hex(s_block_labels[block_idx], block->model_id_raw, block->model_id_len);
        ESP_LOGI(TAG, "%s legacy model field %lu = %u", s_block_labels[block_idx], (unsigned long)field,
                 (unsigned)val);

        const nano_fx_model_def_t *model =
            resolve_legacy_model((nano_fx_block_id_t)block_idx, (uint8_t)val);
        if (model) {
            store_catalog_model_id(block, model);
            apply_model(block, model);
            ESP_LOGI(TAG, "%s model: %s", s_block_labels[block_idx], model->name);
        } else {
            ESP_LOGW(TAG, "%s legacy model ID %u not in catalog", s_block_labels[block_idx], (unsigned)val);
            block->model = NULL;
            block->param_count = 0;
            block->params_loaded = false;
        }

        return true;
    }

    return false;
}

static void try_apply_catalog_model(nano_fx_block_t *block)
{
    if (!block || !block->has_model_id || block->model_id_len == 0) {
        return;
    }

    const nano_fx_model_def_t *model = find_model_in_blob(block->model_id_raw, block->model_id_len);
    if (!model || block->model == model) {
        return;
    }

    apply_model(block, model);
}

static bool extract_field_bytes(const uint8_t *data, size_t len, uint32_t field_num,
                                const uint8_t **out_raw, size_t *out_len)
{
    const uint8_t *best_raw = NULL;
    size_t best_len = 0;

    size_t i = 0;
    while (i < len) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag >> 3;
        uint32_t wire = tag & 0x07;

        if (field == field_num && wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            if (next + flen > len) {
                break;
            }
            const uint8_t *raw = data + next;
            if (find_model_in_blob(raw, flen)) {
                *out_raw = raw;
                *out_len = flen;
                return true;
            }
            if (flen > 0) {
                best_raw = raw;
                best_len = flen;
            }
            i = next + flen;
            continue;
        }

        if (wire == 0) {
            uint32_t skip = 0;
            if (!nano_read_varint(data, len, i, &skip, &next)) {
                break;
            }
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                break;
            }
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            i = next + flen;
            if (i > len) {
                break;
            }
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }

    if (best_raw && best_len > 0) {
        *out_raw = best_raw;
        *out_len = best_len;
        return true;
    }
    return false;
}

static bool extract_field_varint(const uint8_t *data, size_t len, uint32_t field_num, uint32_t *out_val)
{
    size_t i = 0;
    while (i < len) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag >> 3;
        uint32_t wire = tag & 0x07;

        if (field == field_num && wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            *out_val = val;
            return true;
        }

        if (wire == 0) {
            uint32_t skip = 0;
            if (!nano_read_varint(data, len, i, &skip, &next)) {
                break;
            }
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                break;
            }
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            i = next + flen;
            if (i > len) {
                break;
            }
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }
    return false;
}

static bool extract_field_fixed32_float(const uint8_t *data, size_t len, uint32_t field_num, float *out_val)
{
    size_t i = 0;
    bool found = false;
    float last = 0.0f;
    while (i < len) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag >> 3;
        uint32_t wire = tag & 0x07;

        if (field == field_num && wire == 5) {
            if (i + 4 > len) {
                break;
            }
            union {
                uint32_t u;
                float f;
            } u;
            u.u = (uint32_t)data[i] | ((uint32_t)data[i + 1] << 8) | ((uint32_t)data[i + 2] << 16) |
                  ((uint32_t)data[i + 3] << 24);
            last = u.f;
            found = true;
            i += 4;
            continue;
        }

        if (wire == 0) {
            uint32_t skip = 0;
            if (!nano_read_varint(data, len, i, &skip, &next)) {
                break;
            }
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                break;
            }
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            i = next + flen;
            if (i > len) {
                break;
            }
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }
    if (found && out_val) {
        *out_val = last;
    }
    return found;
}

static void ensure_refresh_timers(void)
{
    if (!s_refresh_timeout_timer) {
        const esp_timer_create_args_t timeout_args = {
            .callback = refresh_timeout_cb,
            .name = "nano_fx_to",
        };
        ESP_ERROR_CHECK(esp_timer_create(&timeout_args, &s_refresh_timeout_timer));
    }
    if (!s_refresh_gap_timer) {
        const esp_timer_create_args_t gap_args = {
            .callback = refresh_gap_cb,
            .name = "nano_fx_gap",
        };
        ESP_ERROR_CHECK(esp_timer_create(&gap_args, &s_refresh_gap_timer));
    }
    if (!s_begin_refresh_timer) {
        const esp_timer_create_args_t begin_args = {
            .callback = begin_refresh_cb,
            .name = "nano_fx_begin",
        };
        ESP_ERROR_CHECK(esp_timer_create(&begin_args, &s_begin_refresh_timer));
    }
}

static void refresh_timeout_cb(void *arg)
{
    (void)arg;
    ESP_LOGW(TAG, "FX param refresh timed out for %s", nano_fx_block_label(s_pending_block));
    s_refresh_active = false;
    send_next_refresh();
}

static void refresh_gap_cb(void *arg)
{
    (void)arg;
    send_next_refresh();
}

static void begin_refresh_cb(void *arg)
{
    (void)arg;
    run_begin_param_refresh();
}

static bool block_in_refresh_queue(nano_fx_block_id_t block)
{
    for (size_t q = s_refresh_q_head; q < s_refresh_q_len; q++) {
        if (s_refresh_queue[q] == block) {
            return true;
        }
    }
    return false;
}

static void merge_refresh_queue_from_blocks(void)
{
    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        nano_fx_block_t *block = &s_fx.blocks[i];
        if (!block_needs_refresh(block)) {
            continue;
        }
        if (block_in_refresh_queue((nano_fx_block_id_t)i)) {
            continue;
        }
        if (s_refresh_q_len < NANO_FX_BLOCK_COUNT) {
            s_refresh_queue[s_refresh_q_len++] = (nano_fx_block_id_t)i;
        }
    }
}

static void enqueue_param_refresh(nano_fx_block_id_t block)
{
    if (block >= NANO_FX_BLOCK_COUNT) {
        return;
    }

    ensure_refresh_timers();
    s_fx.blocks[block].params_loaded = false;

    for (size_t q = s_refresh_q_head; q < s_refresh_q_len; q++) {
        if (s_refresh_queue[q] == block) {
            if (!s_refresh_active) {
                send_next_refresh();
            }
            return;
        }
    }

    if (s_refresh_q_len < NANO_FX_BLOCK_COUNT) {
        s_refresh_queue[s_refresh_q_len++] = block;
    }

    if (!s_refresh_active) {
        send_next_refresh();
    }
}

static void send_next_refresh(void)
{
    ensure_refresh_timers();

    while (s_refresh_q_head < s_refresh_q_len) {
        nano_fx_block_id_t block_id = s_refresh_queue[s_refresh_q_head++];
        nano_fx_block_t *block = &s_fx.blocks[block_id];

        if (!block->on || !block_needs_refresh(block)) {
            continue;
        }

        s_pending_block = block_id;
        s_refresh_active = true;

        uint8_t buf[16];
        size_t n = nano_fx_param_refresh_payload(s_slot_cfg[block_id].refresh_slot, buf, sizeof(buf));
        if (n == 0 || !nano_ble_connected()) {
            s_refresh_active = false;
            continue;
        }

        char label[32];
        char model_text[32];
        nano_fx_block_model_text(block, model_text, sizeof(model_text));
        snprintf(label, sizeof(label), "%s param refresh", s_block_labels[block_id]);
        ESP_LOGI(TAG, "Refreshing params: %s (%s)", s_block_labels[block_id], model_text);
        nano_ble_write_c304(label, buf, n);

        esp_timer_stop(s_refresh_timeout_timer);
        esp_timer_start_once(s_refresh_timeout_timer, FX_REFRESH_TIMEOUT_US);
        return;
    }

    s_refresh_active = false;
    s_refresh_q_len = 0;
    s_refresh_q_head = 0;
    nano_ui_set_fx(&s_fx);
}

void nano_fx_reset(void)
{
    memset(&s_fx, 0, sizeof(s_fx));
    memset(s_block_on_asserted_us, 0, sizeof(s_block_on_asserted_us));
    memset(s_block_model_write_us, 0, sizeof(s_block_model_write_us));
    memset(s_block_trusted_model, 0, sizeof(s_block_trusted_model));
    s_gate_assert_us = 0;
    s_gate_assert_on = -1;
    s_gate_amount_assert_us = 0;
    s_gate_amount_assert_pct = -1;
    s_refresh_q_len = 0;
    s_refresh_q_head = 0;
    s_refresh_active = false;
    if (s_refresh_timeout_timer) {
        esp_timer_stop(s_refresh_timeout_timer);
    }
    if (s_refresh_gap_timer) {
        esp_timer_stop(s_refresh_gap_timer);
    }
    if (s_begin_refresh_timer) {
        esp_timer_stop(s_begin_refresh_timer);
    }
}

const nano_fx_state_t *nano_fx_get(void)
{
    return s_fx.valid ? &s_fx : NULL;
}

bool nano_fx_parse_preset(const uint8_t *data, size_t len, nano_fx_state_t *out)
{
    if (!data || !len || !out) {
        return false;
    }

    bool any = false;

    const uint8_t *bypass_raw = NULL;
    size_t bypass_len = 0;
    if (extract_field_bytes(data, len, 31, &bypass_raw, &bypass_len) && bypass_len >= NANO_FX_BLOCK_COUNT) {
        for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
            out->blocks[i].id = (nano_fx_block_id_t)i;
            out->blocks[i].on = bypass_raw[i] == 0;
            out->blocks[i].valid = true;
            any = true;
        }
    }

    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        const uint8_t *raw = NULL;
        size_t raw_len = 0;
        bool got_model = extract_model_field(data, len, s_slot_cfg[i].model_field, &raw, &raw_len) &&
                         raw_len > 0;

        if (got_model) {
            nano_fx_block_t *block = &out->blocks[i];
            block->id = (nano_fx_block_id_t)i;
            block->valid = true;
            any = true;

            uint8_t copy_len = raw_len > sizeof(block->model_id_raw) ? (uint8_t)sizeof(block->model_id_raw)
                                                                     : (uint8_t)raw_len;
            memcpy(block->model_id_raw, raw, copy_len);
            block->model_id_len = copy_len;
            block->has_model_id = true;

            log_model_id_hex(s_block_labels[i], block->model_id_raw, block->model_id_len);

            const nano_fx_model_def_t *model = find_model_in_blob(block->model_id_raw, block->model_id_len);
            if (model) {
                apply_model(block, model);
                ESP_LOGI(TAG, "%s model: %s", s_block_labels[i], model->name);
            } else if (i == NANO_FX_POST2) {
                model = find_post2_delay_in_preset(data, len);
                if (!model && block->model_id_len == 1) {
                    model = resolve_legacy_model(NANO_FX_POST2, block->model_id_raw[0]);
                }
                if (model) {
                    store_catalog_model_id(block, model);
                    apply_model(block, model);
                    ESP_LOGI(TAG, "%s model: %s", s_block_labels[i], model->name);
                } else {
                    ESP_LOGW(TAG, "%s model ID not in catalog — will refresh params anyway",
                             s_block_labels[i]);
                    block->model = NULL;
                    block->param_count = 0;
                    block->params_loaded = false;
                }
            } else {
                ESP_LOGW(TAG, "%s model ID not in catalog — will refresh params anyway", s_block_labels[i]);
                block->model = NULL;
                block->param_count = 0;
                block->params_loaded = false;
            }
        } else if (try_parse_legacy_model_field(data, len, i, &out->blocks[i])) {
            any = true;
        } else if (try_apply_slot_exclusive_model(data, len, i, &out->blocks[i])) {
            any = true;
        }
    }

    {
        nano_fx_block_t *p2 = &out->blocks[NANO_FX_POST2];
        const nano_fx_model_def_t *delay = find_post2_delay_in_preset(data, len);
        if (delay && (!p2->model || !is_delay_model(p2->model))) {
            p2->valid = true;
            store_catalog_model_id(p2, delay);
            apply_model(p2, delay);
            ESP_LOGI(TAG, "%s model: %s (preset scan)", s_block_labels[NANO_FX_POST2], delay->name);
            any = true;
        }
    }

    /*
     * Gate always salvage-scanned — not gated on amp/FX models. Truncated dumps
     * often stop mid-f34 before a sequential walk reaches f53/f54; short C305
     * tails after a mid-dump gap may contain only those fields.
     * Field 54 = inverted bypass (0=ON, 1=OFF). Proto3 omits default 0 (ON): if
     * we found amount (f53) in the gate region but no f54, treat as ON.
     */
    {
        uint32_t gate_field = 0;
        bool has_f54 = extract_field_varint(data, len, 54, &gate_field);
        const char *f54_src = has_f54 ? "walk" : "none";
        if (!has_f54 && scan_gate_bypass_field54(data, len, &gate_field)) {
            has_f54 = true;
            f54_src = "scan";
        }

        /* Amount: field 53 fixed32 in [0,1] → raw 0..255 → percent. */
        float gate_norm = 0.0f;
        bool have_amt = false;
        uint32_t amt_raw = 0;
        const char *amt_src = "none";

        if (extract_field_fixed32_float(data, len, 53, &gate_norm) && isfinite(gate_norm) &&
            gate_norm >= 0.0f && gate_norm <= 1.0f) {
            amt_src = "walk";
        } else if (scan_gate_amount_field53(data, len, &gate_norm)) {
            amt_src = "scan";
        } else {
            gate_norm = -1.0f;
        }
        if (gate_norm >= 0.0f && gate_norm <= 1.0f) {
            int raw = (int)lroundf(gate_norm * 255.0f);
            if (raw < 0) {
                raw = 0;
            }
            if (raw > 255) {
                raw = 255;
            }
            /* Placeholder ~0.1 in some dumps is not a real amount. */
            if (raw > 32 || gate_norm >= 0.15f) {
                amt_raw = (uint32_t)raw;
                out->gate.reduction_pct = (uint8_t)((amt_raw * 100U + 127U) / 255U);
                have_amt = true;
            } else {
                amt_src = "none";
            }
        }
        if (have_amt) {
            if (out->gate.reduction_pct > 100) {
                out->gate.reduction_pct = 100;
            }
            out->gate.reduction_valid = true;
            out->gate.valid = true;
            any = true;
        }

        if (has_f54) {
            out->gate.on = gate_field == 0;
            out->gate.on_known = true;
            out->gate.valid = true;
            any = true;
        } else if (have_amt) {
            /* Reached gate region (f53 present); omitted f54 = enabled. */
            out->gate.on = true;
            out->gate.on_known = true;
            out->gate.valid = true;
            f54_src = "omit";
            any = true;
        }

        ESP_LOGI(TAG, "Gate parse: dump=%u f54=%s/%u on_known=%d amt=%s/%u%% (src=%s raw=%u)",
                 (unsigned)len, (has_f54 || have_amt) ? f54_src : "no",
                 has_f54 ? (unsigned)gate_field : 0u, (int)out->gate.on_known,
                 out->gate.reduction_valid ? "yes" : "no",
                 out->gate.reduction_valid ? (unsigned)out->gate.reduction_pct : 0u, amt_src,
                 have_amt ? (unsigned)amt_raw : 0u);
    }

    out->valid = any;
    return any;
}

bool nano_fx_dump_has_gate_fields(const uint8_t *data, size_t len)
{
    float amt = 0.0f;
    uint32_t bypass = 0;
    return scan_gate_amount_field53(data, len, &amt) || scan_gate_bypass_field54(data, len, &bypass);
}

static void apply_gate_from_parsed(const nano_fx_state_t *parsed)
{
    if (!parsed || !parsed->gate.valid) {
        return;
    }

    if (parsed->gate.on_known) {
        bool remote_on = parsed->gate.on;
        bool accept_on = true;
        if (s_gate_assert_on >= 0 && s_gate_assert_us != 0) {
            int64_t now = esp_timer_get_time();
            if (now - s_gate_assert_us <= FX_ENABLE_ASSERT_US) {
                bool want_on = s_gate_assert_on != 0;
                if (remote_on != want_on) {
                    accept_on = false;
                } else {
                    s_gate_assert_on = -1;
                    s_gate_assert_us = 0;
                }
            } else {
                s_gate_assert_on = -1;
                s_gate_assert_us = 0;
            }
        }
        if (accept_on) {
            s_fx.gate.on = remote_on;
            s_fx.gate.on_known = true;
        }
    }

    if (parsed->gate.reduction_valid) {
        bool accept_amt = true;
        if (s_gate_amount_assert_pct >= 0 && s_gate_amount_assert_us != 0) {
            int64_t now = esp_timer_get_time();
            if (now - s_gate_amount_assert_us <= FX_ENABLE_ASSERT_US) {
                if ((int)parsed->gate.reduction_pct != s_gate_amount_assert_pct) {
                    accept_amt = false;
                } else {
                    s_gate_amount_assert_pct = -1;
                    s_gate_amount_assert_us = 0;
                }
            } else {
                s_gate_amount_assert_pct = -1;
                s_gate_amount_assert_us = 0;
            }
        }
        if (accept_amt) {
            s_fx.gate.reduction_pct = parsed->gate.reduction_pct;
            s_fx.gate.reduction_valid = true;
        }
    } else if (!s_fx.gate.reduction_valid) {
        if (s_fx.gate.reduction_pct == 0) {
            s_fx.gate.reduction_pct = 50;
        }
    }

    s_fx.gate.valid = true;
    s_fx.valid = true;
    ESP_LOGI(TAG, "Gate sync: %s%s amount=%u%%%s",
             s_fx.gate.on_known ? (s_fx.gate.on ? "ON" : "OFF") : "?",
             s_fx.gate.on_known ? "" : " (bypass unknown)",
             (unsigned)s_fx.gate.reduction_pct,
             parsed->gate.reduction_valid ? " (from dump)" : "");
}

bool nano_fx_try_apply_gate_bytes(const uint8_t *data, size_t len)
{
    if (!data || len < 6 || !nano_fx_dump_has_gate_fields(data, len)) {
        return false;
    }

    nano_fx_state_t parsed;
    memset(&parsed, 0, sizeof(parsed));
    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        parsed.blocks[i].id = (nano_fx_block_id_t)i;
    }

    if (!nano_fx_parse_preset(data, len, &parsed) || !parsed.gate.valid) {
        return false;
    }

    apply_gate_from_parsed(&parsed);
    nano_ui_set_fx(&s_fx);
    return true;
}

void nano_fx_begin_param_refresh(void)
{
    ensure_refresh_timers();
    esp_timer_stop(s_begin_refresh_timer);
    esp_timer_start_once(s_begin_refresh_timer, 1);
}

static void run_begin_param_refresh(void)
{
    if (s_refresh_active) {
        merge_refresh_queue_from_blocks();
        return;
    }

    s_refresh_q_len = 0;
    s_refresh_q_head = 0;

    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        nano_fx_block_t *block = &s_fx.blocks[i];
        if (block_needs_refresh(block)) {
            s_refresh_queue[s_refresh_q_len++] = (nano_fx_block_id_t)i;
        }
    }

    if (s_refresh_q_len == 0) {
        ESP_LOGI(TAG, "No active FX blocks to refresh");
        nano_ui_set_fx(&s_fx);
        return;
    }

    ESP_LOGI(TAG, "Queueing FX param refresh for %u block(s)", (unsigned)s_refresh_q_len);
    send_next_refresh();
}

void nano_fx_sync_from_preset(const uint8_t *data, size_t len)
{
    nano_fx_state_t parsed;
    memset(&parsed, 0, sizeof(parsed));

    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        parsed.blocks[i].id = (nano_fx_block_id_t)i;
    }

    if (!nano_fx_parse_preset(data, len, &parsed)) {
        return;
    }

    if (parsed.gate.valid) {
        apply_gate_from_parsed(&parsed);
    }

    for (int i = 0; i < NANO_FX_BLOCK_COUNT; i++) {
        nano_fx_block_t *dst = &s_fx.blocks[i];
        nano_fx_block_t *src = &parsed.blocks[i];

        if (src->valid) {
            if (src->on) {
                dst->on = true;
                s_block_on_asserted_us[i] = 0;
            } else if (!dst->on) {
                dst->on = false;
            } else {
                int64_t now = esp_timer_get_time();
                if (s_block_on_asserted_us[i] != 0 &&
                    now - s_block_on_asserted_us[i] > FX_ENABLE_ASSERT_US) {
                    dst->on = false;
                    s_block_on_asserted_us[i] = 0;
                }
            }
            dst->valid = true;
        }

        if (src->model_id_len > 0) {
            if (s_block_trusted_model[i] || block_model_write_recent((nano_fx_block_id_t)i) ||
                preset_model_stale_legacy(dst, src)) {
                continue;
            }

            dst->has_model_id = true;

            bool id_changed = dst->model_id_len != src->model_id_len ||
                              !id_match(dst->model_id_raw, dst->model_id_len, src->model_id_raw,
                                        src->model_id_len);

            if (id_changed) {
                dst->model_id_len = src->model_id_len;
                memcpy(dst->model_id_raw, src->model_id_raw, src->model_id_len);

                const nano_fx_model_def_t *model = src->model;
                if (!model) {
                    model = find_model_in_blob(dst->model_id_raw, dst->model_id_len);
                }
                if (model) {
                    apply_model(dst, model);
                } else {
                    dst->model = NULL;
                    dst->param_count = 0;
                    dst->params_loaded = false;
                }
            } else if (!dst->model) {
                const nano_fx_model_def_t *model = src->model;
                if (!model) {
                    model = find_model_in_blob(dst->model_id_raw, dst->model_id_len);
                }
                if (model) {
                    apply_model(dst, model);
                }
            }
        }
    }

    s_fx.valid = true;
    nano_ui_set_fx(&s_fx);
    nano_fx_begin_param_refresh();
}

static bool decode_float32_array(const uint8_t *data, size_t len, float *out, size_t out_cap,
                                 size_t *out_count)
{
    size_t count = 0;
    for (size_t i = 0; i + 3 < len && count < out_cap; i += 4) {
        union {
            float f;
            uint8_t b[4];
        } u;
        u.b[0] = data[i];
        u.b[1] = data[i + 1];
        u.b[2] = data[i + 2];
        u.b[3] = data[i + 3];
        out[count++] = u.f;
    }
    *out_count = count;
    return count > 0;
}

static size_t find_fx_param_response(const uint8_t *data, size_t len)
{
    if (!data || len < 6) {
        return len;
    }

    for (size_t i = 0; i + 5 <= len; i++) {
        if (data[i] == 0xC0 && data[i + 1] == 0x08 && data[i + 2] == 0x06 && data[i + 3] == 0x22) {
            return i;
        }
    }

    return len;
}

bool nano_fx_feed_c305(const uint8_t *data, size_t len)
{
    if (!data || len < 10) {
        return false;
    }

    size_t hdr = find_fx_param_response(data, len);
    if (hdr >= len || hdr + 6 > len) {
        return false;
    }

    if (!s_refresh_active) {
        ESP_LOGD(TAG, "FX param response with no pending refresh");
        return true;
    }

    nano_fx_block_t *block = &s_fx.blocks[s_pending_block];
    uint8_t float_len = data[hdr + 4];
    size_t start = hdr + 5;
    size_t end = start + float_len;

    if (end > len || float_len == 0) {
        ESP_LOGW(TAG, "Invalid FX param response for %s", nano_fx_block_label(s_pending_block));
        s_refresh_active = false;
        esp_timer_stop(s_refresh_timeout_timer);
        esp_timer_start_once(s_refresh_gap_timer, FX_REFRESH_GAP_US);
        return true;
    }

    float floats[NANO_FX_MAX_PARAMS];
    size_t float_count = 0;
    decode_float32_array(data + start, end - start, floats, NANO_FX_MAX_PARAMS, &float_count);

    try_apply_catalog_model(block);

    const nano_fx_model_def_t *trusted = s_block_trusted_model[s_pending_block];
    if (trusted) {
        if (block->model != trusted) {
            store_catalog_model_id(block, trusted);
            apply_model(block, trusted);
        }
    } else if (!block->model && block->has_model_id && block->model_id_len == 1) {
        const nano_fx_model_def_t *legacy =
            resolve_legacy_model(s_pending_block, block->model_id_raw[0]);
        if (legacy) {
            store_catalog_model_id(block, legacy);
            apply_model(block, legacy);
        }
    }

    if (!trusted) {
        const nano_fx_model_def_t *inferred =
            infer_model_by_params(s_pending_block, (uint8_t)float_count);
        if (block->model && block->model->param_count == (uint8_t)float_count) {
            inferred = NULL;
        } else if (inferred && block_model_write_recent(s_pending_block)) {
            inferred = NULL;
        } else if (inferred && block->has_model_id && block->model_id_len >= 2) {
            inferred = NULL;
        }
        if (inferred) {
            upgrade_block_model(block, inferred, floats, float_count);
            ESP_LOGI(TAG, "%s inferred model: %s", s_block_labels[s_pending_block], inferred->name);
        } else if (!block->model || block->param_count == 0) {
            set_generic_model(block, (uint8_t)float_count);
        }
    } else if (!block->model || block->param_count == 0) {
        set_generic_model(block, (uint8_t)float_count);
    }

    uint8_t n = block->param_count;
    if (block->model && block->model->param_count < n) {
        n = block->model->param_count;
    }
    if ((uint8_t)float_count < n) {
        n = (uint8_t)float_count;
    }

    for (uint8_t i = 0; i < n && i < float_count; i++) {
        block->params[i].normalized = clampf(floats[i], 0.0f, 1.0f);
        if (block->model && block->model->param_names[i]) {
            block->params[i].display =
                nano_fx_normalized_to_display(block->model->param_names[i], block->params[i].normalized);
        } else {
            block->params[i].display = block->params[i].normalized * 100.0f;
        }
        block->params[i].valid = true;
    }

    block->params_loaded = true;
    ESP_LOGI(TAG, "%s params loaded (%u values)", s_block_labels[s_pending_block], (unsigned)float_count);

    if (block->on && !block->has_model_id) {
        nano_state_request_preset_refresh("FX model sync");
    }

    s_refresh_active = false;
    esp_timer_stop(s_refresh_timeout_timer);
    esp_timer_start_once(s_refresh_gap_timer, FX_REFRESH_GAP_US);
    nano_ui_set_fx(&s_fx);
    return true;
}

bool nano_fx_write_gate_enabled(bool enabled)
{
    s_fx.gate.on = enabled;
    s_fx.gate.on_known = true;
    s_fx.gate.valid = true;
    s_fx.valid = true;

    uint8_t buf[16];
    size_t n = nano_global_toggle_payload(0x09, enabled ? 0x00 : 0x01, buf, sizeof(buf));
    if (n == 0 || !nano_ble_connected()) {
        return false;
    }
    ESP_LOGI(TAG, "Gate %s", enabled ? "ON" : "OFF");
    nano_ui_set_fx(&s_fx);
    if (nano_ble_write_c304(enabled ? "Gate ON" : "Gate OFF", buf, n) != ESP_OK) {
        return false;
    }
    s_gate_assert_us = esp_timer_get_time();
    s_gate_assert_on = enabled ? 1 : 0;
    return true;
}

bool nano_fx_write_gate_reduction(uint8_t pct)
{
    if (pct > 100) {
        pct = 100;
    }
    s_fx.gate.reduction_pct = pct;
    s_fx.gate.reduction_valid = true;
    s_fx.valid = true;

    uint8_t buf[20];
    size_t n = nano_gate_reduction_payload(pct, buf, sizeof(buf));
    if (n == 0 || !nano_ble_connected()) {
        return false;
    }

    char label[24];
    snprintf(label, sizeof(label), "Gate amount %u%%", pct);
    nano_ui_set_fx(&s_fx);
    if (nano_ble_write_c304(label, buf, n) != ESP_OK) {
        return false;
    }
    s_gate_amount_assert_us = esp_timer_get_time();
    s_gate_amount_assert_pct = (int)pct;
    /* Amount writes must not let a stale poll flip bypass while the user is editing. */
    if (s_gate_assert_on < 0) {
        s_gate_assert_us = s_gate_amount_assert_us;
        s_gate_assert_on = s_fx.gate.on ? 1 : 0;
    }
    return true;
}

static bool catalog_index_allowed(nano_fx_block_id_t block, size_t idx)
{
    if (idx >= nano_fx_catalog_count) {
        return false;
    }

    /* EQ utilities: any pre/post slot. */
    if (idx == 12 || idx == 13 || idx == 14) {
        return true;
    }

    if (idx <= 10) {
        return block <= NANO_FX_PRE2;
    }
    if (idx == 11 || idx == 15 || idx == 16) {
        return block <= NANO_FX_PRE2;
    }
    if (idx >= 17 && idx <= 22) {
        return block <= NANO_FX_PRE2;
    }
    if (idx >= 23 && idx <= 26) {
        return block <= NANO_FX_PRE2;
    }
    if (idx == 27) {
        return block == NANO_FX_PRE1;
    }
    if (idx == 28) {
        return block == NANO_FX_POST1;
    }
    if (idx >= 29 && idx <= 32) {
        return block >= NANO_FX_POST1;
    }
    if (idx >= 33 && idx <= 40) {
        return block == NANO_FX_POST1;
    }
    if (idx >= 41 && idx <= 46) {
        return block == NANO_FX_POST2;
    }
    if (idx >= 47 && idx <= 52) {
        return block == NANO_FX_POST3;
    }

    return false;
}

bool nano_fx_is_model_allowed(nano_fx_block_id_t block, const nano_fx_model_def_t *model)
{
    if (block >= NANO_FX_BLOCK_COUNT || !model || !model->name || model->id_len == 0) {
        return false;
    }

    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (&nano_fx_catalog[i] == model) {
            return catalog_index_allowed(block, i);
        }
    }

    return false;
}

size_t nano_fx_allowed_model_count(nano_fx_block_id_t block)
{
    size_t count = 0;
    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (catalog_index_allowed(block, i)) {
            count++;
        }
    }
    return count;
}

const nano_fx_model_def_t *nano_fx_get_allowed_model(nano_fx_block_id_t block, size_t index)
{
    size_t seen = 0;
    for (size_t i = 0; i < nano_fx_catalog_count; i++) {
        if (!catalog_index_allowed(block, i)) {
            continue;
        }
        if (seen == index) {
            return &nano_fx_catalog[i];
        }
        seen++;
    }
    return NULL;
}

bool nano_fx_write_block_model(nano_fx_block_id_t block, const nano_fx_model_def_t *model)
{
    if (block >= NANO_FX_BLOCK_COUNT || !model || !nano_fx_is_model_allowed(block, model)) {
        return false;
    }

    if (!nano_ble_connected()) {
        return false;
    }

    uint8_t buf[16];
    size_t n = nano_fx_model_select_payload(s_slot_cfg[block].model_slot, model->id, model->id_len,
                                             buf, sizeof(buf));
    if (n == 0) {
        return false;
    }

    char label[56];
    snprintf(label, sizeof(label), "%s model %s", s_block_labels[block], model->name);
    ESP_LOGI(TAG, "%s", label);
    if (nano_ble_write_c304(label, buf, n) != ESP_OK) {
        return false;
    }

    nano_fx_block_t *b = &s_fx.blocks[block];
    store_catalog_model_id(b, model);
    apply_model(b, model);
    b->valid = true;
    b->params_loaded = false;
    s_block_model_write_us[block] = esp_timer_get_time();
    s_block_trusted_model[block] = model;
    s_fx.valid = true;

    if (!b->on) {
        uint8_t en_buf[16];
        size_t en_n = nano_global_toggle_payload(s_slot_cfg[block].enable_slot, 0x00, en_buf,
                                                 sizeof(en_buf));
        if (en_n > 0) {
            char en_label[32];
            snprintf(en_label, sizeof(en_label), "%s ON", s_block_labels[block]);
            b->on = true;
            s_block_on_asserted_us[block] = esp_timer_get_time();
            nano_ble_write_c304(en_label, en_buf, en_n);
        }
    }

    enqueue_param_refresh(block);
    nano_ui_set_fx(&s_fx);
    return true;
}

bool nano_fx_write_block_enabled(nano_fx_block_id_t block, bool enabled)
{
    if (block >= NANO_FX_BLOCK_COUNT) {
        return false;
    }

    s_fx.blocks[block].on = enabled;
    s_fx.blocks[block].valid = true;
    s_fx.valid = true;

    uint8_t buf[16];
    size_t n = nano_global_toggle_payload(s_slot_cfg[block].enable_slot, enabled ? 0x00 : 0x01, buf,
                                          sizeof(buf));
    if (n == 0 || !nano_ble_connected()) {
        return false;
    }

    char label[32];
    snprintf(label, sizeof(label), "%s %s", s_block_labels[block], enabled ? "ON" : "OFF");
    ESP_LOGI(TAG, "%s", label);
    nano_ui_set_fx(&s_fx);

    if (nano_ble_write_c304(label, buf, n) != ESP_OK) {
        return false;
    }

    if (enabled) {
        s_block_on_asserted_us[block] = esp_timer_get_time();
        enqueue_param_refresh(block);
    } else {
        s_block_on_asserted_us[block] = 0;
        s_fx.blocks[block].params_loaded = false;
    }
    return true;
}

bool nano_fx_write_param_normalized(nano_fx_block_id_t block, uint8_t param_index, float normalized)
{
    if (block >= NANO_FX_BLOCK_COUNT) {
        return false;
    }

    nano_fx_block_t *b = &s_fx.blocks[block];
    if (!b->model || param_index >= b->param_count) {
        return false;
    }

    normalized = clampf(normalized, 0.0f, 1.0f);
    b->params[param_index].normalized = normalized;
    b->params[param_index].display =
        nano_fx_normalized_to_display(b->model->param_names[param_index], normalized);
    b->params[param_index].valid = true;
    s_fx.valid = true;

    uint8_t buf[24];
    size_t n = nano_fx_param_payload(s_slot_cfg[block].model_slot, param_index, normalized, buf,
                                     sizeof(buf));
    if (n == 0 || !nano_ble_connected()) {
        return false;
    }

    char label[48];
    char val[16];
    nano_fx_format_param(b, param_index, val, sizeof(val));
    snprintf(label, sizeof(label), "%s %s=%s", s_block_labels[block], b->model->param_names[param_index],
             val);
    return nano_ble_write_c304(label, buf, n) == ESP_OK;
}

bool nano_fx_write_param_display(nano_fx_block_id_t block, uint8_t param_index, float display_value)
{
    if (block >= NANO_FX_BLOCK_COUNT) {
        return false;
    }

    nano_fx_block_t *b = &s_fx.blocks[block];
    if (!b->model || param_index >= b->param_count) {
        return false;
    }

    float normalized =
        nano_fx_display_to_normalized(b->model->param_names[param_index], display_value);
    return nano_fx_write_param_normalized(block, param_index, normalized);
}
