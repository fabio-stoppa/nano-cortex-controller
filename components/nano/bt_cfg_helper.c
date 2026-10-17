/*
 * BT_CONTROLLER_INIT_CONFIG_DEFAULT() is a macro, so the Rust BLE client cannot
 * reach it through bindgen. Expose it as a plain function instead.
 */

#include "bt_cfg_helper.h"

void nano_bt_controller_config_default(esp_bt_controller_config_t *cfg)
{
    if (!cfg) {
        return;
    }
    esp_bt_controller_config_t defaults = BT_CONTROLLER_INIT_CONFIG_DEFAULT();
    *cfg = defaults;
}
