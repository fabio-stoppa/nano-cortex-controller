#pragma once

#include "esp_bt.h"

#ifdef __cplusplus
extern "C" {
#endif

/** Fill `cfg` with `BT_CONTROLLER_INIT_CONFIG_DEFAULT()` (macro, not bindgen-able). */
void nano_bt_controller_config_default(esp_bt_controller_config_t *cfg);

#ifdef __cplusplus
}
#endif
