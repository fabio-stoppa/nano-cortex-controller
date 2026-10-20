//! 800×480 RGB panel, CH422G backlight / touch reset, and GT911.
//!
//! Timings and pins for the Waveshare ESP32-S3-Touch-LCD-4.3B.

use core::mem::forget;

use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::timer::timg::TimerGroup;
use esp_hal::lcd_cam::{
    lcd::{
        dpi::{Config as DpiConfig, Dpi, Format, FrameTiming},
        ClockMode, Phase, Polarity,
    },
    LcdCam,
};
use esp_hal::peripherals::Peripherals;
use esp_hal::psram::{FlashFreq, Psram, PsramConfig, PsramMode, SpiRamFreq};
use esp_hal::time::Rate;
use esp_hal::Blocking;
use gt911::Gt911Blocking;

use crate::bounce::{self, Framebuffer};

const WIDTH: usize = 800;
const HEIGHT: usize = 480;

const CH422G_CTRL: u8 = 0x24;
const CH422G_OUT: u8 = 0x38;

pub struct Board<'d> {
    /// PSRAM frame the DPI bounce buffer copies from. Later screens draw here.
    #[allow(dead_code)]
    pub framebuffer: Framebuffer,
    pub i2c: I2c<'d, Blocking>,
    pub touch: Gt911Blocking<I2c<'d, Blocking>>,
}

/// Peripherals the panel does not use. The radio takes the Bluetooth one.
pub struct Rest {
    pub bt: esp_hal::peripherals::BT<'static>,
}

pub fn init(p: Peripherals) -> Result<(Board<'static>, Rest), &'static str> {
    let rest = Rest { bt: p.BT };
    let timg0 = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0);
    log::info!("scheduler up");

    let delay = Delay::new();

    let mut i2c = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .map_err(|_| "i2c")?
    .with_sda(p.GPIO8)
    .with_scl(p.GPIO9);

    let mut touch_gpio = Output::new(p.GPIO4, Level::High, OutputConfig::default());
    touch_reset(&mut i2c, &mut touch_gpio, &delay)?;
    forget(touch_gpio);
    backlight_on(&mut i2c)?;

    let touch = Gt911Blocking::default();
    touch.init(&mut i2c).map_err(|_| "gt911")?;

    let psram = Psram::new(
        p.PSRAM,
        PsramConfig {
            mode: PsramMode::OctalSpi,
            flash_frequency: FlashFreq::FlashFreq80m,
            ram_frequency: SpiRamFreq::Freq80m,
            ..Default::default()
        },
    );
    let (psram_ptr, psram_size) = psram.raw_parts();
    forget(psram);
    let mut framebuffer = Framebuffer::from_psram(psram_ptr, psram_size).ok_or("psram")?;
    framebuffer.fill_bands();
    // Two framebuffers own the first slice. Everything after them is the general
    // heap, registered before the internal heap so the UI does not consume the
    // RAM the controller allocates with malloc_internal.
    let fb_bytes = crate::bounce::BYTES;
    let heap_size = psram_size.saturating_sub(fb_bytes);
    if heap_size > 0 {
        unsafe {
            esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
                psram_ptr.add(fb_bytes),
                heap_size,
                esp_alloc::MemoryCapability::External.into(),
            ));
        }
        log::info!("psram heap {heap_size} bytes");
    }

    let lcd_cam = LcdCam::new(p.LCD_CAM);
    // The 4.3-inch panel locks at 14 MHz with this porch. A slower clock or a
    // longer line makes it flash solid colors.
    let dpi_config = DpiConfig::default()
        .with_frequency(Rate::from_mhz(14))
        .with_clock_mode(ClockMode {
            // `pclk_active_neg = 1`, idle level left at the IDF default (low).
            polarity: Polarity::IdleLow,
            phase: Phase::ShiftHigh,
        })
        .with_format(Format {
            enable_2byte_mode: true,
            ..Default::default()
        })
        .with_timing(FrameTiming {
            horizontal_active_width: WIDTH,
            horizontal_total_width: 820,
            horizontal_blank_front_porch: 12,
            vertical_active_height: HEIGHT,
            vertical_total_height: 500,
            vertical_blank_front_porch: 12,
            hsync_width: 4,
            vsync_width: 4,
            hsync_position: 0,
        })
        .with_vsync_idle_level(Level::High)
        .with_hsync_idle_level(Level::High)
        .with_de_idle_level(Level::Low);

    let dpi = Dpi::new(lcd_cam.lcd, p.DMA_CH0, dpi_config)
        .map_err(|_| "dpi")?
        .with_vsync(p.GPIO3)
        .with_hsync(p.GPIO46)
        .with_de(p.GPIO5)
        .with_pclk(p.GPIO7)
        .with_data0(p.GPIO14)
        .with_data1(p.GPIO38)
        .with_data2(p.GPIO18)
        .with_data3(p.GPIO17)
        .with_data4(p.GPIO10)
        .with_data5(p.GPIO39)
        .with_data6(p.GPIO0)
        .with_data7(p.GPIO45)
        .with_data8(p.GPIO48)
        .with_data9(p.GPIO47)
        .with_data10(p.GPIO21)
        .with_data11(p.GPIO1)
        .with_data12(p.GPIO2)
        .with_data13(p.GPIO42)
        .with_data14(p.GPIO41)
        .with_data15(p.GPIO40);

    // The RGB driver keeps the data pins driven for the whole frame. Without
    // that, the panel samples the line late and the picture sits to the right.
    esp_hal::peripherals::LCD_CAM::regs().lcd_user().modify(|_, w| {
        w.lcd_always_out_en().set_bit();
        w.lcd_update().set_bit()
    });

    let bounce = unsafe { bounce::BounceRing::new(framebuffer.as_ptr()) };
    unsafe { bounce::enable_eof_interrupt() };
    let transfer = dpi.send(true, bounce).map_err(|_| "dpi start")?;
    forget(transfer);
    // `send` leaves the async FIFO reset bit set, which holds the first
    // pixels of every line. Drop it so the scan can run.
    esp_hal::peripherals::LCD_CAM::regs()
        .lcd_misc()
        .modify(|_, w| w.lcd_afifo_reset().clear_bit());

    Ok((
        Board {
            framebuffer,
            i2c,
            touch,
        },
        rest,
    ))
}

fn ch422g_write(i2c: &mut I2c<'_, Blocking>, addr: u8, value: u8) -> Result<(), &'static str> {
    i2c.write(addr, &[value]).map_err(|_| "ch422g")
}

/// Same sequence as `waveshare_esp32_s3_touch_reset` in the vendored BSP.
fn touch_reset(
    i2c: &mut I2c<'_, Blocking>,
    gpio4: &mut Output<'_>,
    delay: &Delay,
) -> Result<(), &'static str> {
    ch422g_write(i2c, CH422G_CTRL, 0x01)?;
    ch422g_write(i2c, CH422G_OUT, 0x2C)?;
    delay.delay_millis(100);
    gpio4.set_low();
    delay.delay_millis(100);
    ch422g_write(i2c, CH422G_OUT, 0x2E)?;
    delay.delay_millis(200);
    Ok(())
}

fn backlight_on(i2c: &mut I2c<'_, Blocking>) -> Result<(), &'static str> {
    ch422g_write(i2c, CH422G_CTRL, 0x01)?;
    ch422g_write(i2c, CH422G_OUT, 0x1E)
}
