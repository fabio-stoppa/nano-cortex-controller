/*
 * SPDX-FileCopyrightText: 2022 Espressif Systems (Shanghai) CO LTD
 *
 * SPDX-License-Identifier: CC0-1.0
 */

#include "bsp/board.h"
#include "bsp/lvgl_port.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

static const char *TAG = "DISPLAY";

static i2c_master_bus_handle_t i2c_bus_handle = NULL;
static i2c_master_dev_handle_t ch422g_ctrl_handle = NULL; // 0x24
static i2c_master_dev_handle_t ch422g_out_handle = NULL;  // 0x38


#if CONFIG_LCD_TOUCH_CONTROLLER_GT911
/**
 * @brief I2C master initialization
 */
    static esp_err_t i2c_master_init(void)
    {
        // Prevent double initialization
        if (i2c_bus_handle != NULL) {
            return ESP_OK;
        }

        // Configure the physical I2C bus
        i2c_master_bus_config_t i2c_bus_conf = {
            .clk_source = I2C_CLK_SRC_DEFAULT,
            .i2c_port = I2C_MASTER_NUM,
            .sda_io_num = I2C_MASTER_SDA_IO,
            .scl_io_num = I2C_MASTER_SCL_IO,
            .flags.enable_internal_pullup = true,
        };
        esp_err_t ret = i2c_new_master_bus(&i2c_bus_conf, &i2c_bus_handle);
        if (ret != ESP_OK) {
            return ret;
        }

        // Register CH422G Control interface (Address 0x24)
        i2c_device_config_t ctrl_dev_cfg = {
            .dev_addr_length = I2C_ADDR_BIT_LEN_7,
            .device_address = 0x24,
            .scl_speed_hz = I2C_MASTER_FREQ_HZ,
        };
        ret = i2c_master_bus_add_device(i2c_bus_handle, &ctrl_dev_cfg, &ch422g_ctrl_handle);
        if (ret != ESP_OK) {
            return ret;
        }

        // Register CH422G Output interface (Address 0x38)
        i2c_device_config_t out_dev_cfg = {
            .dev_addr_length = I2C_ADDR_BIT_LEN_7,
            .device_address = 0x38,
            .scl_speed_hz = I2C_MASTER_FREQ_HZ,
        };
        return i2c_master_bus_add_device(i2c_bus_handle, &out_dev_cfg, &ch422g_out_handle);
    }



// GPIO initialization
static esp_err_t gpio_init(void)
{
    // Zero-initialize the config structure
    gpio_config_t io_conf = {};
    // Disable interrupt
    io_conf.intr_type = GPIO_INTR_DISABLE;
    // Bit mask of the pins, use GPIO4 here
    io_conf.pin_bit_mask = GPIO_INPUT_PIN_SEL;
    // Set as input mode
    io_conf.mode = GPIO_MODE_OUTPUT;

    return gpio_config(&io_conf);
}

// Reset the touch screen
static esp_err_t waveshare_esp32_s3_touch_reset()
{
    uint8_t write_buf = 0x01;
    esp_err_t ret = i2c_master_transmit(ch422g_ctrl_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
    if ( ret != ESP_OK ) return ret;

    // Reset the touch screen. It is recommended to reset the touch screen before using it.
    write_buf = 0x2C;
    ret = i2c_master_transmit(ch422g_out_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
    if (ret != ESP_OK) return ret;

    vTaskDelay(pdMS_TO_TICKS(100));
    ret = gpio_set_level(GPIO_INPUT_IO_4, 0);
    if (ret != ESP_OK) return ret;
    vTaskDelay(pdMS_TO_TICKS(100));
    write_buf = 0x2E;
    ret = i2c_master_transmit(ch422g_out_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
    if (ret != ESP_OK) return ret;
    vTaskDelay(pdMS_TO_TICKS(200));
    return ESP_OK;
}

#endif

// Initialize RGB LCD
esp_err_t waveshare_esp32_s3_rgb_lcd_init(esp_lcd_panel_handle_t *ret_panel, esp_lcd_touch_handle_t *ret_touch)
    {
        ESP_LOGI(TAG, "Install RGB LCD panel driver"); // Log the start of the RGB LCD panel driver installation
        esp_lcd_panel_handle_t panel_handle = NULL; // Declare a handle for the LCD panel
        esp_lcd_rgb_panel_config_t panel_config = {
            .clk_src = LCD_CLK_SRC_DEFAULT, // Set the clock source for the panel
            .timings =  {
                .pclk_hz = LCD_PIXEL_CLOCK_HZ, // Pixel clock frequency
                .h_res = LCD_H_RES, // Horizontal resolution
                .v_res = LCD_V_RES, // Vertical resolution
                .hsync_pulse_width = 4, // Horizontal sync pulse width
                .hsync_back_porch = 8, // Horizontal back porch
                .hsync_front_porch = 8, // Horizontal front porch
                .vsync_pulse_width = 4, // Vertical sync pulse width
                .vsync_back_porch = 8, // Vertical back porch
                .vsync_front_porch = 8, // Vertical front porch
                .flags = {
                    .pclk_active_neg = 1, // Active low pixel clock
                },
            },
            .data_width = RGB_DATA_WIDTH, // Data width for RGB
            .num_fbs = LVGL_PORT_LCD_RGB_BUFFER_NUMS, // Number of frame buffers
            .bounce_buffer_size_px = RGB_BOUNCE_BUFFER_SIZE, // Bounce buffer size in pixels
            .dma_burst_size = 64,
            .hsync_gpio_num = LCD_IO_RGB_HSYNC, // GPIO number for horizontal sync
            .vsync_gpio_num = LCD_IO_RGB_VSYNC, // GPIO number for vertical sync
            .de_gpio_num = LCD_IO_RGB_DE, // GPIO number for data enable
            .pclk_gpio_num = LCD_IO_RGB_PCLK, // GPIO number for pixel clock
            .disp_gpio_num = LCD_IO_RGB_DISP, // GPIO number for display
            .data_gpio_nums = {
                LCD_IO_RGB_DATA0,
                LCD_IO_RGB_DATA1,
                LCD_IO_RGB_DATA2,
                LCD_IO_RGB_DATA3,
                LCD_IO_RGB_DATA4,
                LCD_IO_RGB_DATA5,
                LCD_IO_RGB_DATA6,
                LCD_IO_RGB_DATA7,
                LCD_IO_RGB_DATA8,
                LCD_IO_RGB_DATA9,
                LCD_IO_RGB_DATA10,
                LCD_IO_RGB_DATA11,
                LCD_IO_RGB_DATA12,
                LCD_IO_RGB_DATA13,
                LCD_IO_RGB_DATA14,
                LCD_IO_RGB_DATA15,
            },
            .flags = {
                .fb_in_psram = 1, // Use PSRAM for framebuffer
            },
        };

        // Create a new RGB panel with the specified configuration
        ESP_ERROR_CHECK(esp_lcd_new_rgb_panel(&panel_config, &panel_handle));

        ESP_LOGI(TAG, "Initialize RGB LCD panel"); // Log the initialization of the RGB LCD panel
        ESP_ERROR_CHECK(esp_lcd_panel_init(panel_handle)); // Initialize the LCD panel

        esp_lcd_touch_handle_t tp_handle = NULL; // Declare a handle for the touch panel
    #if CONFIG_LCD_TOUCH_CONTROLLER_GT911
        ESP_LOGI(TAG, "Initialize I2C bus"); // Log the initialization of the I2C bus
        esp_err_t ret = i2c_master_init(); // Initialize the I2C master
        if (ret != ESP_OK) return ret;
        ESP_LOGI(TAG, "Initialize GPIO"); // Log GPIO initialization
        ret = gpio_init(); // Initialize GPIO pins
        if ( ret != ESP_OK) return ret; 
        ESP_LOGI(TAG, "Initialize Touch LCD"); // Log touch LCD initialization
        ret = waveshare_esp32_s3_touch_reset(); // Reset the touch panel
        if (ret != ESP_OK) return ret;

        esp_lcd_panel_io_handle_t tp_io_handle = NULL; // Declare a handle for touch panel I/O
        esp_lcd_panel_io_i2c_config_t tp_io_config = ESP_LCD_TOUCH_IO_I2C_GT911_CONFIG();
        tp_io_config.scl_speed_hz = I2C_MASTER_FREQ_HZ;

        // Configure I2C for GT911 touch controller
        ESP_LOGI(TAG, "Initialize I2C panel IO"); // Log I2C panel I/O initialization
        esp_err_t io_ret = esp_lcd_new_panel_io_i2c(i2c_bus_handle, &tp_io_config, &tp_io_handle);
        if (io_ret != ESP_OK) {
            ESP_LOGE(TAG, "esp_lcd_new_panel_io_i2c failed: %s", esp_err_to_name(io_ret));
        }

        ESP_LOGI(TAG, "Initialize touch controller GT911"); // Log touch controller initialization
        const esp_lcd_touch_config_t tp_cfg = {
            .x_max = LCD_H_RES, // Set maximum X coordinate
            .y_max = LCD_V_RES, // Set maximum Y coordinate
            .rst_gpio_num = PIN_NUM_TOUCH_RST, // GPIO number for reset
            .int_gpio_num = PIN_NUM_TOUCH_INT, // GPIO number for interrupt
            .levels = {
                .reset = 0, // Reset level
                .interrupt = 0, // Interrupt level
            },
            .flags = {
                .swap_xy = 0, // No swap of X and Y
                .mirror_x = 0, // No mirroring of X
                .mirror_y = 0, // No mirroring of Y
            },
        };
        esp_err_t touch_ret = esp_lcd_touch_new_i2c_gt911(tp_io_handle, &tp_cfg, &tp_handle);
        if (touch_ret != ESP_OK) {
            ESP_LOGE(TAG, "esp_lcd_touch_new_i2c_gt911 failed: %s", esp_err_to_name(touch_ret));
        }
    #endif // CONFIG_LCD_TOUCH_CONTROLLER_GT911

        if (ret_panel) {
            *ret_panel = panel_handle;
        }
        if (ret_touch) {
            *ret_touch = tp_handle;
        }
        return ESP_OK; // Return success
    }


/******************************* Turn on the screen backlight **************************************/
esp_err_t waveshare_rgb_lcd_bl_on()
{
    //Configure CH422G to output mode 
    uint8_t write_buf = 0x01;
    esp_err_t ret = i2c_master_transmit(ch422g_ctrl_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
    if (ret != ESP_OK) return ret;
    //Pull the backlight pin high to light the screen backlight 
    write_buf = 0x1E;
    return i2c_master_transmit(ch422g_out_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
}

/******************************* Turn off the screen backlight **************************************/
esp_err_t waveshare_rgb_lcd_bl_off()
{
    //Configure CH422G to output mode 
    uint8_t write_buf = 0x01;
    esp_err_t ret = i2c_master_transmit(ch422g_ctrl_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
    if (ret != ESP_OK) return ret;

    //Turn off the screen backlight by pulling the backlight pin low 
    write_buf = 0x1A;
    return i2c_master_transmit(ch422g_out_handle, &write_buf, 1, I2C_MASTER_TIMEOUT_MS);
}



