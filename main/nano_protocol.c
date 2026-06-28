#include "nano_protocol.h"

#include <stdio.h>

#include "esp_log.h"

static const char *TAG = "nano_proto";

static size_t encode_varint(uint32_t value, uint8_t *out, size_t cap)
{
    size_t n = 0;
    while (value >= 0x80 && n + 1 < cap) {
        out[n++] = (uint8_t)((value & 0x7F) | 0x80);
        value >>= 7;
    }
    if (n >= cap) {
        return 0;
    }
    out[n++] = (uint8_t)value;
    return n;
}

static size_t copy_fixed(const uint8_t *src, size_t len, uint8_t *buf, size_t cap)
{
    if (len > cap) {
        return 0;
    }
    for (size_t i = 0; i < len; i++) {
        buf[i] = src[i];
    }
    return len;
}

size_t nano_fe_metadata_request(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {0x06, 0xC0, 0x08, 0x03, 0x01, 0x00, 0x00, 0x00};
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_request_current_state(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x0C, 0xC0, 0x08, 0x03, 0x18, 0x01, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00,
    };
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_preset_change_ack(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {0x06, 0xC0, 0x20, 0x01, 0x1E, 0x00, 0x00, 0x00};
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_post_save_refresh(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x0C, 0xC0, 0x08, 0x03, 0x18, 0x00, 0x20, 0x01, 0x28, 0x01, 0x01, 0x00, 0x00, 0x00,
    };
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_amp_knob_payload(nano_knob_t knob, uint8_t raw_value, uint8_t *buf, size_t cap)
{
    if (cap < 12) {
        return 0;
    }

    uint8_t value_bytes[5];
    size_t vb_len = encode_varint(raw_value, value_bytes, sizeof(value_bytes));
    if (vb_len == 0) {
        return 0;
    }

    size_t total = 1 + 3 + vb_len + 6;
    if (total > cap) {
        return 0;
    }

    size_t i = 0;
    buf[i++] = 0x00; /* length placeholder */
    buf[i++] = 0xC0;
    buf[i++] = 0x18;
    buf[i++] = (uint8_t)knob;
    buf[i++] = 0x20;
    for (size_t v = 0; v < vb_len; v++) {
        buf[i++] = value_bytes[v];
    }
    buf[i++] = 0x28;
    buf[i++] = 0x00;
    buf[i++] = 0x1A;
    buf[i++] = 0x00;
    buf[i++] = 0x00;
    buf[i++] = 0x00;
    buf[0] = (uint8_t)(i - 2);
    return i;
}

size_t nano_global_toggle_payload(uint8_t id, uint8_t value, uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x0A, 0xC0, 0x08, 0x01, 0x18, 0x00, 0x20, 0x00, 0x1F, 0x00, 0x00, 0x00,
    };
    if (cap < sizeof(pkt)) {
        return 0;
    }
    for (size_t i = 0; i < sizeof(pkt); i++) {
        buf[i] = pkt[i];
    }
    buf[5] = id;
    buf[7] = value;
    return sizeof(pkt);
}

size_t nano_tuner_enable_payload(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x0D, 0xC0, 0x08, 0x01, 0x2D, 0x00, 0x00, 0xDC, 0x43, 0x38, 0x01, 0x7F, 0x00, 0x00, 0x00,
    };
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_tuner_enable_footswitch_payload(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x12, 0xC0, 0x08, 0x01, 0x22, 0x01, 0x44, 0x2D, 0x56, 0xC5, 0xD7, 0x3F, 0x30, 0x01, 0x38, 0x01, 0x80, 0x00,
    };
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

size_t nano_tuner_disable_payload(uint8_t *buf, size_t cap)
{
    static const uint8_t pkt[] = {
        0x0B, 0xC0, 0x08, 0x01, 0x2D, 0x00, 0x00, 0xDC, 0x43, 0x7F, 0x00, 0x00, 0x00,
    };
    return copy_fixed(pkt, sizeof(pkt), buf, cap);
}

void nano_log_bytes(const char *tag, const char *label, const uint8_t *data, size_t len)
{
    char line[96];
    size_t pos = 0;

    pos += (size_t)snprintf(line + pos, sizeof(line) - pos, "%s: ", label);
    for (size_t i = 0; i < len && pos + 4 < sizeof(line); i++) {
        pos += (size_t)snprintf(line + pos, sizeof(line) - pos, "%02X ", data[i]);
    }
    ESP_LOGI(tag, "%s", line);
}

void nano_log_bytes_full(const char *tag, const char *label, const uint8_t *data, size_t len)
{
    if (!data || len == 0) {
        ESP_LOGI(tag, "%s: (empty)", label);
        return;
    }

    char line[128];
    size_t pos = 0;
    pos += (size_t)snprintf(line + pos, sizeof(line) - pos, "%s: ", label);

    for (size_t i = 0; i < len; i++) {
        if (pos + 4 >= sizeof(line)) {
            ESP_LOGI(tag, "%s", line);
            pos = 0;
            pos += (size_t)snprintf(line + pos, sizeof(line) - pos, "  ... ");
        }
        pos += (size_t)snprintf(line + pos, sizeof(line) - pos, "%02X ", data[i]);
    }

    if (pos > 0) {
        ESP_LOGI(tag, "%s", line);
    }
}
