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
mod midi;
mod usb_storage;
mod panel;
mod screen;
mod settings;
mod state;
mod syncutil;
mod tuner;
mod ui;

use embassy_executor::Spawner;
use embassy_futures::select::select;
use esp_backtrace as _;

esp_bootloader_esp_idf::esp_app_desc!();

static mut APP_CORE_STACK: esp_hal::system::Stack<4096> = esp_hal::system::Stack::new();

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
    // The second core only services the scan interrupts; the radio stays on
    // the first core.
    esp_rtos::start_second_core(
        rest.cpu_ctrl,
        rest.cpu_intr1,
        unsafe { &mut *core::ptr::addr_of_mut!(APP_CORE_STACK) },
        bounce::claim_interrupts_on_this_core,
    );
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
            select(ble::touch_only(&mut board), usb_storage::requested()).await;
            usb_storage::run(rest.usb, &mut board).await;
            return;
        }
    };
    log::info!("ble controller up");
    let controller = bt_hci::controller::ExternalController::<_, 20>::new(connector);
    select(ble::run(controller, &mut board), usb_storage::requested()).await;
    usb_storage::run(rest.usb, &mut board).await;
}
