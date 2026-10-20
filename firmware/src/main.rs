//! Bare-metal firmware for the Waveshare ESP32-S3-Touch-LCD-4.3B.
//!
//! Draws the Slint controller UI into the panel and runs a TrouBLE central
//! for the Nano Cortex and an M-Vave footswitch.

#![no_std]
#![no_main]
#![feature(asm_experimental_arch)]

extern crate alloc;

mod amp;
mod ble;
mod bounce;
mod clock;
mod deadline;
mod float;
mod fx;
mod link;
mod memsafe;
mod panel;
mod screen;
mod state;
mod syncutil;
mod tuner;
mod ui;

use embassy_executor::Spawner;
use esp_backtrace as _;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_rtos::main]
async fn main(_spawner: Spawner) {
    let peripherals =
        esp_hal::init(esp_hal::Config::default().with_cpu_clock(esp_hal::clock::CpuClock::max()));
    esp_println::logger::init_logger(log::LevelFilter::Info);

    let (mut board, rest) = panel::init(peripherals).unwrap_or_else(|_| loop {
        core::hint::spin_loop();
    });
    // Registered after the PSRAM heap, so ordinary allocations stay out of it.
    // The controller task stack and its exchange memory come from here.
    esp_alloc::heap_allocator!(size: 72 * 1024);
    log::info!("panel up (800x480, GT911)");
    esp_hal::delay::Delay::new().delay_millis(100);
    log::info!("bounce refills={}", bounce::eof_count());
    ui::frame(&mut board.framebuffer);
    log::info!("ui up");
    log::info!(
        "internal heap free {}",
        esp_alloc::HEAP.free_caps(esp_alloc::MemoryCapability::Internal.into())
    );
    // 8 KB sits on the controller's exchange memory and has aborted init before.
    let ble_config = esp_radio::ble::Config::default().with_task_stack_size(16 * 1024);
    log::info!("starting ble controller");
    let connector = match esp_radio::ble::controller::BleConnector::new(rest.bt, ble_config) {
        Ok(connector) => connector,
        Err(err) => {
            log::error!("ble controller init failed: {err:?}");
            ble::touch_only(&mut board).await;
            return;
        }
    };
    log::info!("ble controller up");
    let controller = bt_hci::controller::ExternalController::<_, 20>::new(connector);
    ble::run(controller, &mut board).await;
}
