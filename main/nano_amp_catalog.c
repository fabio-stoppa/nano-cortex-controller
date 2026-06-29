#include "nano_amp.h"

#include <ctype.h>
#include <stdio.h>
#include <string.h>

const nano_amp_capture_t nano_amp_catalog[] = {
    { 1,  "CA John's Ch1",      NANO_AMP_BANK_CLEAN },
    { 2,  "Comet 60",           NANO_AMP_BANK_CLEAN },
    { 3,  "Bogna X100B Ch1",    NANO_AMP_BANK_CLEAN },
    { 4,  "Watt Custom Clean",  NANO_AMP_BANK_CLEAN },
    { 5,  "US HRDLX ChA",       NANO_AMP_BANK_CLEAN },
    { 6,  "Comet 60",           NANO_AMP_BANK_2 },
    { 7,  "NoMatch Chief",      NANO_AMP_BANK_2 },
    { 8,  "US Prince 65",       NANO_AMP_BANK_2 },
    { 9,  "Watt Custom VH",     NANO_AMP_BANK_2 },
    { 10, "D-Cell H4 Ch2",      NANO_AMP_BANK_2 },
    { 11, "Bogna Fish+290",     NANO_AMP_BANK_3 },
    { 12, "Brit 2555 Rhy",      NANO_AMP_BANK_3 },
    { 13, "Custom 3SE+290",     NANO_AMP_BANK_3 },
    { 14, "D-Cell H4 Ch3",      NANO_AMP_BANK_3 },
    { 15, "ENG Energy",         NANO_AMP_BANK_3 },
    { 16, "Bogna X100B Ch3",    NANO_AMP_BANK_HIGH_GAIN },
    { 17, "CA Studio+290",      NANO_AMP_BANK_HIGH_GAIN },
    { 18, "CA John's Ch3",      NANO_AMP_BANK_HIGH_GAIN },
    { 19, "PV 505Sig",          NANO_AMP_BANK_HIGH_GAIN },
    { 20, "Brit 2555 Lead",     NANO_AMP_BANK_HIGH_GAIN },
    { 21, "Aggi 751",           NANO_AMP_BANK_BASS },
    { 22, "Amped SV Classic",   NANO_AMP_BANK_BASS },
    { 23, "Rodent+SV",          NANO_AMP_BANK_BASS },
    { 24, "CA MixBass",         NANO_AMP_BANK_BASS },
    { 25, "Anima Fuzz",         NANO_AMP_BANK_BASS },
};

const size_t nano_amp_catalog_count = sizeof(nano_amp_catalog) / sizeof(nano_amp_catalog[0]);

static const char *bank_short_name(nano_amp_bank_t bank)
{
    switch (bank) {
    case NANO_AMP_BANK_CLEAN:
        return "Clean";
    case NANO_AMP_BANK_HIGH_GAIN:
        return "High Gain";
    case NANO_AMP_BANK_BASS:
        return "Bass";
    default:
        return NULL;
    }
}

nano_amp_bank_t nano_amp_bank_from_slot(uint8_t slot)
{
    static const nano_amp_bank_t banks[] = {
        NANO_AMP_BANK_CLEAN,
        NANO_AMP_BANK_2,
        NANO_AMP_BANK_3,
        NANO_AMP_BANK_HIGH_GAIN,
        NANO_AMP_BANK_BASS,
    };

    if (slot < 1 || slot > NANO_AMP_CAPTURE_COUNT) {
        return NANO_AMP_BANK_CLEAN;
    }
    return banks[(slot - 1) / 5];
}

void nano_amp_format_bank_line(uint8_t slot, char *out, size_t cap)
{
    if (!out || cap == 0) {
        return;
    }
    out[0] = '\0';
    if (slot < 1 || slot > NANO_AMP_CAPTURE_COUNT) {
        return;
    }

    nano_amp_bank_t bank = nano_amp_bank_from_slot(slot);
    unsigned bank_num = (unsigned)((slot - 1) / 5 + 1);
    const char *name = bank_short_name(bank);
    if (name && name[0]) {
        snprintf(out, cap, "Bank %u - %s", bank_num, name);
    } else {
        snprintf(out, cap, "Bank %u", bank_num);
    }
}

const char *nano_amp_bank_label(nano_amp_bank_t bank)
{
    switch (bank) {
    case NANO_AMP_BANK_CLEAN:
        return "Bank 1 - Clean";
    case NANO_AMP_BANK_2:
        return "Bank 2";
    case NANO_AMP_BANK_3:
        return "Bank 3";
    case NANO_AMP_BANK_HIGH_GAIN:
        return "Bank 4 - High Gain";
    case NANO_AMP_BANK_BASS:
        return "Bank 5 - Bass";
    default:
        return "Captures";
    }
}

const nano_amp_capture_t *nano_amp_capture_by_slot(uint8_t slot)
{
    for (size_t i = 0; i < nano_amp_catalog_count; i++) {
        if (nano_amp_catalog[i].slot == slot) {
            return &nano_amp_catalog[i];
        }
    }
    return NULL;
}

static bool is_generic_capture_name(const char *name)
{
    if (!name || !name[0]) {
        return true;
    }

    const char *p = name;
    while (*p && isspace((unsigned char)*p)) {
        p++;
    }
    if (strncasecmp(p, "capture", 7) != 0) {
        return false;
    }
    p += 7;
    while (*p && isspace((unsigned char)*p)) {
        p++;
    }
    if (!isdigit((unsigned char)*p)) {
        return false;
    }
    while (isdigit((unsigned char)*p)) {
        p++;
    }
    while (*p && isspace((unsigned char)*p)) {
        p++;
    }
    return *p == '\0';
}

void nano_amp_capture_header_text(const nano_amp_t *amp, char *out, size_t cap)
{
    if (!out || cap == 0) {
        return;
    }
    out[0] = '\0';
    if (!amp) {
        strncpy(out, "Amp", cap - 1);
        out[cap - 1] = '\0';
        return;
    }

    if (amp->have_capture_name && amp->capture_name[0] &&
        !is_generic_capture_name(amp->capture_name)) {
        strncpy(out, amp->capture_name, cap - 1);
        out[cap - 1] = '\0';
        return;
    }

    if (amp->have_capture_slot && amp->capture_slot > 0) {
        const nano_amp_capture_t *capture = nano_amp_capture_by_slot(amp->capture_slot);
        if (capture && capture->name) {
            strncpy(out, capture->name, cap - 1);
            out[cap - 1] = '\0';
            return;
        }
    }

    strncpy(out, "Amp", cap - 1);
    out[cap - 1] = '\0';
}
