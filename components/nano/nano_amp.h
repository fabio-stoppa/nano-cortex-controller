#pragma once

#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>

#include "nano_proto.h"

#ifdef __cplusplus
extern "C" {
#endif

#define NANO_AMP_CAPTURE_COUNT     25
#define NANO_AMP_CAPTURE_NAME_MAX  40

typedef enum {
    NANO_AMP_BANK_CLEAN = 0,
    NANO_AMP_BANK_2,
    NANO_AMP_BANK_3,
    NANO_AMP_BANK_HIGH_GAIN,
    NANO_AMP_BANK_BASS,
} nano_amp_bank_t;

typedef struct {
    uint8_t slot;
    const char *name;
    nano_amp_bank_t bank;
} nano_amp_capture_t;

extern const nano_amp_capture_t nano_amp_catalog[];
extern const size_t nano_amp_catalog_count;

const char *nano_amp_bank_label(nano_amp_bank_t bank);

nano_amp_bank_t nano_amp_bank_from_slot(uint8_t slot);

/** e.g. "Bank 1 - Clean" or "Bank 2" (ASCII hyphen only). */
void nano_amp_format_bank_line(uint8_t slot, char *out, size_t cap);

const nano_amp_capture_t *nano_amp_capture_by_slot(uint8_t slot);

/** Prefer live state name, then factory catalog for slot, else "Amp". */
void nano_amp_capture_header_text(const nano_amp_t *amp, char *out, size_t cap);

#ifdef __cplusplus
}
#endif
