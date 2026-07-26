#include "nano_proto.h"

#include <math.h>
#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "nano_tuner.h"

static float read_f32_le(const uint8_t *p);

bool nano_read_varint(const uint8_t *data, size_t len, size_t offset, uint32_t *value, size_t *next)
{
    if (!data || !next || offset >= len) {
        return false;
    }

    uint32_t result = 0;
    unsigned shift = 0;

    for (size_t i = offset; i < len && i < offset + 10; i++) {
        uint8_t b = data[i];
        result |= (uint32_t)(b & 0x7F) << shift;
        if ((b & 0x80) == 0) {
            if (value) {
                *value = result;
            }
            *next = i + 1;
            return true;
        }
        shift += 7;
        if (shift >= 32) {
            return false;
        }
    }
    return false;
}

bool nano_parse_amp(const uint8_t *data, size_t len, nano_amp_t *out)
{
    if (!data || !out) {
        return false;
    }

    out->valid = false;
    out->gain = out->level = out->bass = out->mid = out->treble = 0;
    out->capture_slot = 0;
    out->capture_name[0] = '\0';
    out->have_capture_slot = false;
    out->have_capture_name = false;

    bool have[5] = {false, false, false, false, false};
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

        if (wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            if (field >= 3 && field <= 7) {
                uint8_t raw = (uint8_t)(val > 255 ? 255 : val);
                switch (field) {
                case 3:
                    if (!have[0]) {
                        out->gain = raw;
                        have[0] = true;
                    }
                    break;
                case 4:
                    if (!have[1]) {
                        out->level = raw;
                        have[1] = true;
                    }
                    break;
                case 5:
                    if (!have[2]) {
                        out->bass = raw;
                        have[2] = true;
                    }
                    break;
                case 6:
                    if (!have[3]) {
                        out->mid = raw;
                        have[3] = true;
                    }
                    break;
                case 7:
                    if (!have[4]) {
                        out->treble = raw;
                        have[4] = true;
                    }
                    break;
                default:
                    break;
                }
            } else if (field == 11 && !out->have_capture_slot) {
                out->capture_slot = (uint8_t)(val > 255 ? 255 : val);
                out->have_capture_slot = true;
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
            i = next;
            if (i + flen > len) {
                break;
            }
            if (field == 32 && !out->have_capture_name) {
                char name[NANO_AMP_CAPTURE_NAME_MAX];
                if (nano_proto_first_string(data + i, flen, 2, name, sizeof(name)) && name[0]) {
                    strncpy(out->capture_name, name, NANO_AMP_CAPTURE_NAME_MAX - 1);
                    out->capture_name[NANO_AMP_CAPTURE_NAME_MAX - 1] = '\0';
                    out->have_capture_name = true;
                }
            }
            i += flen;
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }

    out->valid = have[0] || have[1] || have[2] || have[3] || have[4];
    return out->valid;
}

bool nano_parse_bpm(const uint8_t *data, size_t len, uint16_t *out_bpm)
{
    if (!data || !len || !out_bpm) {
        return false;
    }

    /*
     * Field 56 fixed32 float BPM. Present top-level in some long dumps, and near
     * the end of FE metadata. Recurse into wire-type-2, then fall back to a raw
     * C5 03 tag scan — FE blobs are not always clean nested protobuf from offset 0.
     */
    const uint32_t field_want = 56;
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

        if (wire == 0) {
            if (!nano_read_varint(data, len, i, NULL, &next)) {
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
            i = next;
            if (i + flen > len) {
                break;
            }
            if (flen > 0 && nano_parse_bpm(data + i, flen, out_bpm)) {
                return true;
            }
            i += flen;
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            if (field == field_want) {
                float f = read_f32_le(data + i);
                if (isfinite((double)f) && f >= 20.f && f <= 400.f) {
                    *out_bpm = (uint16_t)lroundf(f);
                    return true;
                }
            }
            i += 4;
        } else {
            break;
        }
    }

    /* Raw tag scan: (56<<3)|5 = 453 = C5 03 */
    for (size_t off = 0; off + 6 <= len; off++) {
        if (data[off] != 0xC5 || data[off + 1] != 0x03) {
            continue;
        }
        float f = read_f32_le(data + off + 2);
        if (isfinite((double)f) && f >= 20.f && f <= 400.f) {
            *out_bpm = (uint16_t)lroundf(f);
            return true;
        }
    }

    return false;
}

void nano_log_bpm_hunt(const char *tag, const uint8_t *data, size_t len)
{
    if (!tag || !data || !len) {
        return;
    }

    uint32_t max_field = 0;
    unsigned entries = 0;
    unsigned fixed32_n = 0;
    bool saw_f56 = false;
    size_t i = 0;

    ESP_LOGI(tag, "BPM hunt: scan %u-byte state blob", (unsigned)len);

    while (i < len && entries < 160) {
        uint32_t tag_val = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag_val, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag_val >> 3;
        uint32_t wire = tag_val & 0x07;
        entries++;
        if (field > max_field) {
            max_field = field;
        }
        if (field == 56) {
            saw_f56 = true;
        }

        if (wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            i = next;
            if (field >= 40 && val >= 20 && val <= 400) {
                ESP_LOGI(tag, "BPM hunt: f%u varint=%u (tempo-ish?)", (unsigned)field, (unsigned)val);
            }
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
            uint32_t u = (uint32_t)data[i] | ((uint32_t)data[i + 1] << 8) |
                         ((uint32_t)data[i + 2] << 16) | ((uint32_t)data[i + 3] << 24);
            float f = 0.f;
            memcpy(&f, &u, sizeof(f));
            fixed32_n++;
            ESP_LOGI(tag,
                     "BPM hunt: f%u fixed32=%g raw=%08lX%s",
                     (unsigned)field,
                     (double)f,
                     (unsigned long)u,
                     field == 56 ? " <-- BPM candidate" : (field == 46 ? " (A440?)" : ""));
            i += 4;
        } else {
            break;
        }
    }

    ESP_LOGI(tag,
             "BPM hunt: done entries=%u max_field=%u fixed32_count=%u has_f56=%s",
             entries,
             (unsigned)max_field,
             fixed32_n,
             saw_f56 ? "yes" : "no");
}

void nano_log_bpm_raw_scan(const char *tag, const char *label, const uint8_t *data, size_t len)
{
    if (!tag || !data || !len) {
        return;
    }

    const char *lab = label ? label : "raw";
    unsigned hit_f56 = 0;
    unsigned hit_91 = 0;
    unsigned hit_float = 0;

    ESP_LOGI(tag, "BPM raw scan [%s]: %u bytes", lab, (unsigned)len);

    for (size_t i = 0; i + 6 <= len; i++) {
        /* Protobuf tag for field 56 wire5 = (56<<3)|5 = 453 = C5 03 */
        if (data[i] == 0xC5 && data[i + 1] == 0x03 && i + 6 <= len) {
            float f = read_f32_le(data + i + 2);
            hit_f56++;
            ESP_LOGI(tag, "BPM raw [%s]: f56 tag @%u = %g", lab, (unsigned)i, (double)f);
        }

        if (data[i] == 0x2D && data[i + 5] == 0x91) {
            float f = read_f32_le(data + i + 1);
            hit_91++;
            ESP_LOGI(tag, "BPM raw [%s]: 2D+91 @%u = %g", lab, (unsigned)i, (double)f);
        }
    }

    /* Sparse LE float scan (4-byte aligned) — log only tempo-ish values. */
    for (size_t i = 0; i + 4 <= len; i += 4) {
        float f = read_f32_le(data + i);
        if (!isfinite((double)f) || f < 40.f || f > 250.f) {
            continue;
        }
        /* Prefer near-integers (tap tempos are whole BPM). */
        float nearest = roundf(f);
        if (fabsf(f - nearest) > 0.05f) {
            continue;
        }
        hit_float++;
        if (hit_float <= 12) {
            ESP_LOGI(tag, "BPM raw [%s]: LE float @%u = %g", lab, (unsigned)i, (double)f);
        }
    }

    ESP_LOGI(tag,
             "BPM raw [%s]: done f56_tags=%u trailer91=%u tempo_floats=%u",
             lab, hit_f56, hit_91, hit_float);
}

static bool proto_string_is_printable(const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        return false;
    }
    for (size_t i = 0; i < len; i++) {
        if (data[i] < 0x20 || data[i] > 0x7E) {
            return false;
        }
    }
    return true;
}

bool nano_proto_first_string(const uint8_t *data, size_t len, uint32_t field_num, char *out, size_t cap)
{
    if (!data || !len || !out || cap == 0) {
        return false;
    }

    out[0] = '\0';
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
            i = next;
            if (i + flen > len) {
                break;
            }
            if (field == field_num && flen > 0 && proto_string_is_printable(data + i, flen)) {
                size_t n = flen < cap - 1 ? flen : cap - 1;
                memcpy(out, data + i, n);
                out[n] = '\0';
                return true;
            }
            i += flen;
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

bool nano_proto_field_varint(const uint8_t *data, size_t len, uint32_t field_num, uint32_t *out_val)
{
    if (!data || !len || !out_val) {
        return false;
    }

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

        if (wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            if (field == field_num) {
                *out_val = val;
                return true;
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

bool nano_parse_fe_preset_catalog(const uint8_t *data, size_t len, nano_preset_catalog_t *out)
{
    if (!data || !len || !out) {
        return false;
    }

    memset(out, 0, sizeof(*out));
    uint8_t idx = 0;
    size_t i = 0;

    while (i < len && idx < NANO_PRESET_COUNT) {
        uint32_t tag = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag, &next)) {
            break;
        }
        i = next;

        uint32_t field = tag >> 3;
        uint32_t wire = tag & 0x07;

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
            i = next;
            if (i + flen > len) {
                break;
            }
            if (field == 18) {
                char name[NANO_PRESET_NAME_MAX];
                if (nano_proto_first_string(data + i, flen, 1, name, sizeof(name)) && name[0]) {
                    strncpy(out->names[idx], name, NANO_PRESET_NAME_MAX - 1);
                    out->names[idx][NANO_PRESET_NAME_MAX - 1] = '\0';
                }
                idx++;
            }
            i += flen;
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            i += 4;
        } else {
            break;
        }
    }

    out->count = idx;
    out->valid = idx > 0;
    return out->valid;
}

bool nano_parse_tuner_state(const uint8_t *data, size_t len, bool *out_active,
                            nano_tuner_route_t *out_route)
{
    if (!data || !out_active || !out_route) {
        return false;
    }

    *out_active = false;
    *out_route = NANO_TUNER_ROUTE_UNKNOWN;

    bool f45_seen = false;
    uint32_t f45_val = 0;
    bool f51_seen = false;

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

        if (wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            if (field == 45) {
                f45_seen = true;
                f45_val = val;
            } else if (field == 51) {
                f51_seen = true;
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

    /* Field 45 = tuner on. Field 51 (~6010) = tuner off. Route comes from C305 only. */
    if (f45_seen && f45_val >= 1) {
        *out_active = true;
        return true;
    }

    if (f51_seen && !f45_seen) {
        *out_active = false;
        return true;
    }

    return false;
}

void nano_log_tuner_state_fields(const char *tag, const uint8_t *data, size_t len)
{
    if (!tag || !data || len == 0) {
        return;
    }

    bool have[5] = {false, false, false, false, false};
    uint32_t vals[5] = {0, 0, 0, 0, 0};
    static const uint32_t fields[5] = {45, 48, 49, 50, 51};

    size_t i = 0;
    while (i < len) {
        uint32_t tag_val = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag_val, &next)) {
            break;
        }

        uint32_t field = tag_val >> 3;
        uint32_t wire = tag_val & 0x07;
        i = next;

        if (wire != 0) {
            if (wire == 1) {
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
            continue;
        }

        uint32_t val = 0;
        if (!nano_read_varint(data, len, i, &val, &next)) {
            break;
        }
        i = next;

        for (unsigned f = 0; f < 5; f++) {
            if (field == fields[f]) {
                have[f] = true;
                vals[f] = val;
                break;
            }
        }
    }

    char f45[16];
    char f48[16];
    char f49[16];
    char f50[16];
    char f51[16];

    snprintf(f45, sizeof(f45), have[0] ? "%u" : "-", (unsigned)vals[0]);
    snprintf(f48, sizeof(f48), have[1] ? "%u" : "-", (unsigned)vals[1]);
    snprintf(f49, sizeof(f49), have[2] ? "%u" : "-", (unsigned)vals[2]);
    snprintf(f50, sizeof(f50), have[3] ? "%u" : "-", (unsigned)vals[3]);
    snprintf(f51, sizeof(f51), have[4] ? "%u" : "-", (unsigned)vals[4]);

    ESP_LOGI(tag, "Tuner preset: f45=%s f48=%s f49=%s f50=%s f51=%s", f45, f48, f49, f50, f51);
}

bool nano_parse_tuner_active(const uint8_t *data, size_t len, bool *out_active)
{
    nano_tuner_route_t route = NANO_TUNER_ROUTE_UNKNOWN;
    return nano_parse_tuner_state(data, len, out_active, &route);
}

static float read_f32_le(const uint8_t *p)
{
    uint32_t u = (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
    float f;
    memcpy(&f, &u, sizeof(f));
    return f;
}

static bool map_pedal_knob_id(uint8_t hw_id, uint8_t *knob_id)
{
    if (hw_id <= 4) {
        *knob_id = hw_id;
        return true;
    }
    if (hw_id >= 0x05 && hw_id <= 0x09) {
        *knob_id = (uint8_t)(hw_id - 0x05);
        return true;
    }
    return false;
}

static bool try_parse_amp_gain_2d_notify(const uint8_t *data, size_t len, uint8_t *raw)
{
    if (!data || !raw || len < 12) {
        return false;
    }

    for (size_t i = 0; i + 11 <= len; i++) {
        if (data[i] != 0x2D) {
            continue;
        }

        float value = read_f32_le(data + i + 1);
        if (fabsf(value - 440.f) <= 1.f) {
            continue;
        }

        if (i + 6 > len || data[i + 5] != 0x63) {
            continue;
        }

        bool gain_prefix = false;
        for (size_t back = (i >= 8 ? i - 8 : 0); back + 5 <= i; back++) {
            if (data[back] != 0xC0 || data[back + 1] != 0x08 || data[back + 2] != 0x01 ||
                data[back + 3] != 0x20) {
                continue;
            }

            uint32_t prefix = 0;
            size_t next = 0;
            if (nano_read_varint(data, len, back + 4, &prefix, &next) && next == i) {
                gain_prefix = true;
                break;
            }
        }
        if (!gain_prefix) {
            continue;
        }

        if (value < 0.f) {
            value = 0.f;
        } else if (value > 255.f) {
            value = 255.f;
        }
        *raw = (uint8_t)(value + 0.5f);
        return true;
    }

    return false;
}

bool nano_try_parse_amp_knob_notify(const uint8_t *data, size_t len, uint8_t *knob_id, uint8_t *raw)
{
    if (!data || !knob_id || !raw || len < 5) {
        return false;
    }

    for (size_t i = 0; i + 4 < len; i++) {
        if (data[i] != 0xC0 || data[i + 1] != 0x18 || data[i + 2] > 4 || data[i + 3] != 0x20) {
            continue;
        }

        uint32_t val = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i + 4, &val, &next)) {
            continue;
        }

        *knob_id = data[i + 2];
        *raw = (uint8_t)(val > 255 ? 255 : val);
        return true;
    }

    return false;
}

bool nano_try_parse_amp_hw_notify(const uint8_t *data, size_t len, uint8_t *knob_id, uint8_t *raw)
{
    if (!data || !knob_id || !raw || len < 7) {
        return false;
    }

    uint8_t gain_raw = 0;
    if (try_parse_amp_gain_2d_notify(data, len, &gain_raw)) {
        *knob_id = 0;
        *raw = gain_raw;
        return true;
    }

    for (size_t i = 0; i + 6 < len; i++) {
        if (data[i] != 0xC0 || data[i + 1] != 0x08 || data[i + 2] != 0x01) {
            continue;
        }

        size_t p = i + 3;
        if (p < len && data[p] == 0x18) {
            if (p + 3 >= len || data[p + 2] != 0x20) {
                continue;
            }
            uint8_t hw_id = data[p + 1];
            uint8_t mapped = 0;
            if (!map_pedal_knob_id(hw_id, &mapped)) {
                continue;
            }
            uint32_t val = 0;
            size_t next = 0;
            if (!nano_read_varint(data, len, p + 3, &val, &next)) {
                continue;
            }
            /* Tap-tempo uses 18 01 20 01 2D <f32> 91 — not an amp varint. */
            if (next < len && data[next] == 0x2D) {
                continue;
            }
            *knob_id = mapped;
            *raw = (uint8_t)(val > 255 ? 255 : val);
            return true;
        }

        if (p < len && data[p] == 0x20) {
            uint32_t val = 0;
            size_t next = 0;
            if (!nano_read_varint(data, len, p + 1, &val, &next)) {
                continue;
            }
            if (next < len && data[next] == 0x2D) {
                continue;
            }
            *knob_id = 0;
            *raw = (uint8_t)(val > 255 ? 255 : val);
            return true;
        }
    }

    return false;
}

bool nano_try_parse_bpm_notify(const uint8_t *data, size_t len, uint16_t *out_bpm)
{
    if (!data || !out_bpm || len < 12) {
        return false;
    }

    for (size_t i = 0; i + 6 <= len; i++) {
        if (data[i] != 0x2D) {
            continue;
        }
        /* Amp gain float uses trailer 0x63; tap-tempo BPM uses 0x91. */
        if (data[i + 5] != 0x91) {
            continue;
        }

        float value = read_f32_le(data + i + 1);
        if (!isfinite((double)value) || value < 20.f || value > 400.f) {
            continue;
        }

        bool prefix = false;
        size_t back_start = (i >= 12) ? (i - 12) : 0;
        for (size_t back = back_start; back + 3 <= i; back++) {
            if (data[back] == 0xC0 && data[back + 1] == 0x08 && data[back + 2] == 0x01) {
                prefix = true;
                break;
            }
        }
        if (!prefix) {
            continue;
        }

        *out_bpm = (uint16_t)lroundf(value);
        return true;
    }

    return false;
}

static unsigned pct(uint8_t raw)
{
    return (unsigned)((raw * 100U + 127U) / 255U);
}

void nano_log_amp(const char *tag, const nano_amp_t *amp)
{
    if (!amp || !amp->valid) {
        ESP_LOGI(tag, "Amp: (not parsed yet)");
        return;
    }
    ESP_LOGI(tag,
             "Amp — Gain: %u%% Level: %u%% Bass: %u%% Mid: %u%% Treble: %u%%  "
             "(raw %u/%u/%u/%u/%u)",
             pct(amp->gain), pct(amp->level), pct(amp->bass), pct(amp->mid), pct(amp->treble),
             amp->gain, amp->level, amp->bass, amp->mid, amp->treble);
}

static bool field_is_known(uint32_t field)
{
    switch (field) {
    case 3:
    case 4:
    case 5:
    case 6:
    case 7:
    case 8:
    case 9:
    case 11:
    case 12:
    case 13:
    case 14:
    case 15:
    case 18:
    case 31:
    case 32:
    case 33:
    case 44:
    case 45:
    case 46:
    case 48:
    case 49:
    case 50:
    case 51:
    case 52:
    case 54:
        return true;
    default:
        return false;
    }
}

static void log_field_line(const char *tag, const char *label, uint32_t field, uint32_t wire,
                           const char *detail)
{
    const char *mark = field_is_known(field) ? " " : "*";
    ESP_LOGI(tag, "STATE %s %c f%-3u w%-1u %s", label, *mark, (unsigned)field, (unsigned)wire, detail);
}

void nano_log_proto_fields(const char *tag, const char *label, const uint8_t *data, size_t len)
{
    if (!tag || !label || !data || len == 0) {
        return;
    }

    ESP_LOGI(tag, "STATE %s — protobuf scan (%u bytes, *=unknown field)", label, (unsigned)len);

    size_t i = 0;
    unsigned entries = 0;

    while (i < len && entries < 128) {
        uint32_t tag_val = 0;
        size_t next = 0;
        if (!nano_read_varint(data, len, i, &tag_val, &next)) {
            break;
        }

        uint32_t field = tag_val >> 3;
        uint32_t wire = tag_val & 0x07;
        i = next;
        entries++;

        char detail[96];
        detail[0] = '\0';

        if (wire == 0) {
            uint32_t val = 0;
            if (!nano_read_varint(data, len, i, &val, &next)) {
                break;
            }
            snprintf(detail, sizeof(detail), "varint %u", (unsigned)val);
            log_field_line(tag, label, field, wire, detail);
            i = next;
        } else if (wire == 1) {
            if (i + 8 > len) {
                break;
            }
            snprintf(detail,
                     sizeof(detail),
                     "fixed64 %02X%02X%02X%02X%02X%02X%02X%02X",
                     data[i],
                     data[i + 1],
                     data[i + 2],
                     data[i + 3],
                     data[i + 4],
                     data[i + 5],
                     data[i + 6],
                     data[i + 7]);
            log_field_line(tag, label, field, wire, detail);
            i += 8;
        } else if (wire == 2) {
            uint32_t flen = 0;
            if (!nano_read_varint(data, len, i, &flen, &next)) {
                break;
            }
            size_t start = next;
            size_t end = start + flen;
            if (end > len) {
                break;
            }

            size_t show = flen > 12 ? 12 : flen;
            int pos = snprintf(detail, sizeof(detail), "bytes len=%u hex=", (unsigned)flen);
            for (size_t b = 0; b < show && pos + 3 < (int)sizeof(detail); b++) {
                pos += snprintf(detail + pos, sizeof(detail) - (size_t)pos, "%02X", data[start + b]);
            }
            if (flen > show) {
                snprintf(detail + pos, sizeof(detail) - (size_t)pos, "...");
            }
            log_field_line(tag, label, field, wire, detail);
            i = end;
        } else if (wire == 5) {
            if (i + 4 > len) {
                break;
            }
            uint32_t u = (uint32_t)data[i] | ((uint32_t)data[i + 1] << 8) |
                         ((uint32_t)data[i + 2] << 16) | ((uint32_t)data[i + 3] << 24);
            float f = 0.f;
            memcpy(&f, &u, sizeof(f));
            snprintf(detail,
                     sizeof(detail),
                     "fixed32 raw=%08lX float=%g",
                     (unsigned long)u,
                     (double)f);
            log_field_line(tag, label, field, wire, detail);
            i += 4;
        } else {
            snprintf(detail, sizeof(detail), "unsupported wire type");
            log_field_line(tag, label, field, wire, detail);
            break;
        }
    }

    ESP_LOGI(tag, "STATE %s — scan done (%u entries)", label, entries);
}
