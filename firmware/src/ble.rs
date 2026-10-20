//! Dual BLE central: Neural DSP Nano Cortex and an M-Vave FootCtrl.
//!
//! Scans for the same names as the ESP-IDF client, connects to both, raises
//! the ATT MTU with the 512-byte packet pool, writes the settings read on
//! C304, and subscribes to C305 notify. C306 is the indicate mirror of C305
//! and is left unsubscribed. FootCtrl / FootCtrlPlus use the BLE-MIDI service.
//! The notify queue is 16 deep: each slot is 512 bytes and sits on the main
//! stack beside the controller's static buffers.

use bt_hci::cmd::le::{LeReadLocalSupportedFeatures, LeSetDataLength, LeSetScanParams};
use bt_hci::controller::ControllerCmdSync;
use embassy_futures::join::join3;
use embassy_futures::select::{select, Either};
use embassy_time::with_timeout;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::signal::Signal;

#[derive(Clone, Copy)]
pub struct SliderWrite {
    pub id: u8,
    pub raw: u8,
}

pub static SLIDER: Signal<CriticalSectionRawMutex, SliderWrite> = Signal::new();

/// The old host dropped queued notifies here. The link task reads them directly.
pub fn nano_ble_flush_c305_backlog() {}
use embassy_time::{Duration, Timer};
use trouble_host::prelude::*;

use crate::panel::Board;

const NANO_PREFIX: &[u8] = b"Neural DSP Nano Cortex";
const FOOT: &[u8] = b"FootCtrl";
const FOOT_PLUS: &[u8] = b"FootCtrlPlus";

const UUID_A002: u16 = 0xA002;
const UUID_A003: u16 = 0xA003;
const UUID_C304: u16 = 0xC304;
const UUID_C305: u16 = 0xC305;
const UUID_C306: u16 = 0xC306;

/// BLE-MIDI service and IO characteristic, little-endian.
const MIDI_SVC: [u8; 16] = [
    0x00, 0xC7, 0xC4, 0x4E, 0xE3, 0x6C, 0x51, 0xA7, 0x33, 0x4B, 0xE8, 0xED, 0x5A, 0x0E, 0xB8, 0x03,
];
const MIDI_IO: [u8; 16] = [
    0xF3, 0x6B, 0x10, 0x9D, 0x66, 0xF2, 0xA9, 0xA1, 0x12, 0x41, 0x68, 0x38, 0xDB, 0xE5, 0x72, 0x77,
];

const CONNECTIONS_MAX: usize = 2;
const L2CAP_CHANNELS_MAX: usize = 6;

struct Hunt {
    nano: Signal<NoopRawMutex, Address>,
    foot: Signal<NoopRawMutex, Address>,
    nano_armed: core::sync::atomic::AtomicBool,
    foot_armed: core::sync::atomic::AtomicBool,
}

impl Hunt {
    fn new() -> Self {
        Self {
            nano: Signal::new(),
            foot: Signal::new(),
            nano_armed: core::sync::atomic::AtomicBool::new(true),
            foot_armed: core::sync::atomic::AtomicBool::new(true),
        }
    }

    fn arm(&self) {
        use core::sync::atomic::Ordering;
        self.nano_armed.store(true, Ordering::Relaxed);
        self.foot_armed.store(true, Ordering::Relaxed);
    }
}

impl EventHandler for Hunt {
    fn on_adv_reports(&self, mut reports: LeAdvReportsIter<'_>) {
        use core::sync::atomic::Ordering;
        while let Some(Ok(report)) = reports.next() {
            let Some(name) = local_name(report.data) else {
                continue;
            };
            let addr = Address::new(report.addr_kind, report.addr);
            if name.starts_with(NANO_PREFIX) && self.nano_armed.swap(false, Ordering::Relaxed) {
                log::info!("found Nano Cortex");
                self.nano.signal(addr);
            } else if (eq_ignore_ascii_case(name, FOOT_PLUS) || eq_ignore_ascii_case(name, FOOT))
                && self.foot_armed.swap(false, Ordering::Relaxed)
            {
                log::info!("found footswitch ({} bytes)", name.len());
                self.foot.signal(addr);
            }
        }
    }
}

pub async fn run<C>(controller: C, board: &mut Board<'_>)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    // Top two bits set: static random address.
    let address = Address::random([0xff, 0x8f, 0x1b, 0x05, 0xe4, 0x42]);
    log::info!("central address {address}");

    let mut resources: HostResources<C, DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> =
        HostResources::new();
    let stack = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .build();
    let mut runner = stack.runner();
    let hunt = Hunt::new();

    let _ = join3(
        async {
            if let Err(_) = runner.run_with_handler(&hunt).await {
                log::error!("ble runner stopped");
            }
        },
        links(&stack, &hunt),
        touch_loop(board),
    )
    .await;
}

pub async fn touch_only(board: &mut Board<'_>) {
    touch_loop(board).await;
}

async fn links<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    loop {
        hunt.arm();
        session(stack, hunt).await;
        crate::link::set_up(false);
        crate::screen::nano_ui_set_ble_connected(false);
        log::warn!("link ended, scanning again");
        Timer::after(Duration::from_secs(2)).await;
    }
}

async fn session<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    let (nano, foot) = {
        let central = stack.central();
        let mut scanner = Scanner::new(central);
        let config = listen(&[]);
        let scan = match scanner.scan(&config).await {
            Ok(scan) => scan,
            Err(_) => {
                log::warn!("scan start failed");
                return;
            }
        };
        log::info!("scanning");
        let nano = hunt.nano.wait().await;
        let foot = match select(hunt.foot.wait(), Timer::after(Duration::from_secs(8))).await {
            Either::First(addr) => Some(addr),
            Either::Second(_) => None,
        };
        // Dropping the session asks the runner to disable scanning.
        drop(scan);
        drop(scanner);
        Timer::after(Duration::from_millis(200)).await;
        (nano, foot)
    };

    let mut central = stack.central();
    let nano_filter = [nano];
    let connect = ConnectConfig {
        connect_params: peer_params(),
        scan_config: listen(&nano_filter),
    };
    log::info!("connecting to nano");
    let conn = match central.connect(&connect).await {
        Ok(conn) => conn,
        Err(_) => {
            log::warn!("nano connect failed");
            return;
        }
    };
    let interval_us = conn.params().conn_interval.as_micros();
    log::info!("nano connected, interval {interval_us} us");
    crate::link::set_up(true);
    crate::screen::nano_ui_set_ble_connected(true);
    // One 251-byte PDU is about 2.1 ms. Without this, the link stays at
    // 27-byte PDUs and one event still carries more packets than the
    // controller can buffer.
    match conn.update_data_length(stack, 251, 2120).await {
        Ok(()) => log::info!("data length 251 requested"),
        Err(_) => log::warn!("data length update failed"),
    };

    let client = match GattClient::<C, DefaultPacketPool, 8>::new(stack, &conn).await {
        Ok(client) => client,
        Err(_) => {
            log::warn!("nano gatt client failed");
            return;
        }
    };
    log::info!("nano ATT MTU {}", conn.att_mtu());

    let _ = join3(
        async {
            if let Err(_) = client.task().await {
                log::warn!("nano gatt task ended");
            }
        },
        nano_link(&client),
        foot_link(stack, foot),
    )
    .await;
}

async fn nano_link<C: Controller, P: PacketPool>(client: &GattClient<'_, C, P, 8>) {
    let Some(c304) = find_char(client, UUID_C304).await else {
        log::warn!("C304 missing");
        return;
    };
    let Some(c305) = find_char(client, UUID_C305).await else {
        log::warn!("C305 missing");
        return;
    };
    if let Some(c306) = find_char(client, UUID_C306).await {
        log::info!("C306 handle {} left unsubscribed", c306.handle);
    }
    log::info!("C304 handle {} C305 handle {}", c304.handle, c305.handle);

    let mut listener = match client.subscribe(&c305, false).await {
        Ok(listener) => listener,
        Err(_) => {
            log::warn!("C305 subscribe failed");
            return;
        }
    };

    let mut payload = [0u8; 16];
    let n = nano_wire::editor::read_settings_payload(&mut payload);
    if n > 0 {
        match client.write_handle(c304.handle, &payload[..n]).await {
            Ok(()) => log::info!("C304 read-settings write ({n} bytes)"),
            Err(_) => log::warn!("C304 write failed"),
        }
    }
    crate::state::nano_state_reset();
    crate::state::nano_state_begin_sync();
    flush_writes(client, c304.handle).await;

    let mut seen = 0u32;
    loop {
        match with_timeout(
            Duration::from_millis(10),
            select(listener.next(), SLIDER.wait()),
        )
        .await
        {
            Ok(Either::First(note)) => {
                seen = seen.wrapping_add(1);
                if seen <= 4 || seen % 32 == 0 {
                    log_pkt("C305", note.as_ref(), seen);
                }
                crate::state::feed_c305_bytes(note.as_ref());
            }
            Ok(Either::Second(write)) => {
                let mut buf = [0u8; 24];
                let n = slider_payload(write, &mut buf);
                if n > 0 {
                    match client.write_handle(c304.handle, &buf[..n]).await {
                        Ok(()) => log::info!("slider {} = {}", write.id, write.raw),
                        Err(_) => log::warn!("slider write failed"),
                    }
                }
            }
            Err(_) => {}
        }
        while let Some(cmd) = crate::link::pop_cmd() {
            apply_cmd(cmd);
        }
        if let Some(param) = crate::link::take_param() {
            let _ = crate::fx::nano_fx_write_param_normalized(
                u32::from(param.block),
                param.wire,
                param.norm,
            );
        }
        crate::state::poll();
        flush_writes(client, c304.handle).await;
    }
}

fn apply_cmd(cmd: crate::link::Cmd) {
    match cmd {
        crate::link::Cmd::Gate(on) => {
            let _ = crate::fx::nano_fx_write_gate_enabled(on);
        }
        crate::link::Cmd::Tuner(on) => {
            let _ = crate::tuner::nano_tuner_set_active(on);
        }
        crate::link::Cmd::Tempo => {
            let _ = crate::state::nano_state_tempo_tap();
        }
        crate::link::Cmd::TempoEnter => {
            let _ = crate::state::nano_state_tempo_enter();
        }
        crate::link::Cmd::TempoExit => {
            let _ = crate::state::nano_state_tempo_exit();
        }
        crate::link::Cmd::Focus(slot) => crate::fx::nano_fx_focus_from_ui(slot),
        crate::link::Cmd::Block { slot, on } => {
            let _ = crate::fx::nano_fx_write_block_enabled(u32::from(slot), on);
        }
        crate::link::Cmd::Model { block, delta } => step_model(block, delta),
        crate::link::Cmd::Save => {
            let _ = crate::state::nano_state_save_preset();
        }
        crate::link::Cmd::Copy => {
            let _ = crate::state::nano_state_copy_current();
        }
        crate::link::Cmd::Library => {
            let _ = crate::state::nano_state_request_library();
        }
        crate::link::Cmd::Preset(delta) => {
            let _ = crate::state::nano_state_step_preset(delta);
        }
        crate::link::Cmd::Capture(delta) => {
            let _ = crate::state::nano_state_step_capture(delta);
        }
    }
}

fn step_model(block: u8, delta: i8) {
    if delta == 0 || block as usize >= crate::fx::types::NANO_FX_BLOCK_COUNT {
        return;
    }
    let mut fx = crate::fx::types::NanoFxState::zeroed();
    if !crate::fx::nano_fx_snapshot(&mut fx) {
        return;
    }
    let count = crate::fx::nano_fx_allowed_model_count(u32::from(block));
    if count == 0 {
        return;
    }
    let current = fx.blocks[block as usize].model;
    let found = (0..count).find(|&index| {
        crate::fx::nano_fx_get_allowed_model(u32::from(block), index)
            .is_some_and(|model| core::ptr::eq(model as *const _, current))
    });
    let next = match found {
        Some(index) if delta < 0 => {
            if index == 0 {
                count - 1
            } else {
                index - 1
            }
        }
        Some(index) => (index + 1) % count,
        None if delta < 0 => count - 1,
        None => 0,
    };
    if let Some(model) = crate::fx::nano_fx_get_allowed_model(u32::from(block), next) {
        let _ = crate::fx::nano_fx_write_block_model(u32::from(block), model);
    }
}

async fn flush_writes<C: Controller, P: PacketPool>(
    client: &GattClient<'_, C, P, 8>,
    handle: u16,
) {
    while let Some(packet) = crate::link::pop_write() {
        match client.write_handle(handle, &packet).await {
            Ok(()) => log::info!("C304 write {} bytes", packet.len()),
            Err(_) => log::warn!("C304 write failed"),
        }
    }
}

fn slider_payload(write: SliderWrite, buf: &mut [u8]) -> usize {
    match write.id {
        0..=4 => {
            let Some(knob) = nano_wire::Knob::from_u8(write.id) else {
                return 0;
            };
            nano_wire::amp_knob_payload(knob, write.raw, buf)
        }
        5 => nano_wire::capture_volume_payload(write.raw, buf),
        6 => nano_wire::gate_reduction_payload(write.raw, buf),
        7 => nano_wire::global_toggle_payload(0x09, write.raw, buf),
        _ => 0,
    }
}

async fn foot_link<C: Controller>(stack: &Stack<'_, C, DefaultPacketPool>, addr: Option<Address>) {
    let Some(addr) = addr else {
        core::future::pending::<()>().await;
        return;
    };
    let mut central = stack.central();
    let foot_filter = [addr];
    let connect = ConnectConfig {
        connect_params: peer_params(),
        scan_config: listen(&foot_filter),
    };
    let conn = match central.connect(&connect).await {
        Ok(conn) => conn,
        Err(_) => {
            log::warn!("footswitch connect failed");
            core::future::pending::<()>().await;
            return;
        }
    };
    log::info!("footswitch connected");
    let client = match GattClient::<C, DefaultPacketPool, 4>::new(stack, &conn).await {
        Ok(client) => client,
        Err(_) => {
            log::warn!("footswitch gatt client failed");
            core::future::pending::<()>().await;
            return;
        }
    };

    let _ = embassy_futures::join::join(
        async {
            if client.task().await.is_err() {
                log::warn!("footswitch gatt task ended");
            }
        },
        async {
            let svcs = match client.services_by_uuid(&Uuid::new_long(MIDI_SVC)).await {
                Ok(svcs) => svcs,
                Err(_) => {
                    log::warn!("midi service missing");
                    core::future::pending::<()>().await;
                    return;
                }
            };
            let Some(svc) = svcs.first() else {
                log::warn!("midi service missing");
                core::future::pending::<()>().await;
                return;
            };
            let io = match client
                .characteristic_by_uuid::<[u8]>(svc, &Uuid::new_long(MIDI_IO))
                .await
            {
                Ok(io) => io,
                Err(_) => {
                    log::warn!("midi io missing");
                    core::future::pending::<()>().await;
                    return;
                }
            };
            let mut listener = match client.subscribe(&io, false).await {
                Ok(listener) => listener,
                Err(_) => {
                    log::warn!("midi subscribe failed");
                    core::future::pending::<()>().await;
                    return;
                }
            };
            log::info!("midi io handle {}", io.handle);
            let mut seen = 0u32;
            loop {
                let note = listener.next().await;
                seen = seen.wrapping_add(1);
                if seen <= 4 || seen % 32 == 0 {
                    log_pkt("midi", note.as_ref(), seen);
                }
            }
        },
    )
    .await;
}

async fn find_char<C: Controller, P: PacketPool>(
    client: &GattClient<'_, C, P, 8>,
    uuid: u16,
) -> Option<Characteristic<[u8]>> {
    for svc_uuid in [UUID_A002, UUID_A003] {
        let svcs = client
            .services_by_uuid(&Uuid::new_short(svc_uuid))
            .await
            .ok()?;
        for svc in svcs.iter() {
            if let Ok(ch) = client
                .characteristic_by_uuid::<[u8]>(svc, &Uuid::new_short(uuid))
                .await
            {
                return Some(ch);
            }
        }
    }
    None
}

fn listen(filter: &[Address]) -> ScanConfig<'_> {
    // The complete name is in the scan response. A passive scan never
    // asks for that packet, so the Nano is invisible. The window stays
    // short so one advert does not fill the controller's 9 ACL buffers.
    ScanConfig {
        active: true,
        filter_accept_list: filter,
        interval: Duration::from_millis(80),
        window: Duration::from_millis(30),
        ..Default::default()
    }
}

fn peer_params() -> RequestedConnParams {
    // Exchange memory holds 9 ACL RX buffers. The radio asserts in rwble.c
    // (line 505, param 0x2000, RX data pointer) when one connection event
    // delivers more packets than that. TrouBLE's default is an 80 ms
    // interval with a zero event length. esp-radio's adapter is patched to
    // CE mode so this cap is used. 2.5 ms is about five 27-byte PDUs, or
    // one 251-byte PDU. The interval is the 7.5 ms minimum, so a missed
    // event-length cap still cannot hold a long burst.
    RequestedConnParams {
        min_connection_interval: Duration::from_micros(7_500),
        max_connection_interval: Duration::from_micros(7_500),
        max_latency: 0,
        min_event_length: Duration::from_micros(0),
        max_event_length: Duration::from_micros(2_500),
        supervision_timeout: Duration::from_secs(4),
    }
}

fn local_name(data: &[u8]) -> Option<&[u8]> {
    let mut i = 0;
    let mut short = None;
    while i < data.len() {
        let len = data[i] as usize;
        if len == 0 || i + 1 + len > data.len() {
            break;
        }
        let kind = data[i + 1];
        let val = &data[i + 2..i + 1 + len];
        if kind == 0x09 {
            return Some(val);
        }
        if kind == 0x08 {
            short = Some(val);
        }
        i += 1 + len;
    }
    short
}

fn eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.to_ascii_lowercase() == y.to_ascii_lowercase())
}

fn log_pkt(label: &str, data: &[u8], n: u32) {
    let mut hex = [0u8; 24];
    let show = data.len().min(8);
    let digits = b"0123456789ABCDEF";
    let mut o = 0;
    for b in &data[..show] {
        hex[o] = digits[(b >> 4) as usize];
        hex[o + 1] = digits[(b & 0x0f) as usize];
        hex[o + 2] = b' ';
        o += 3;
    }
    let text = core::str::from_utf8(&hex[..o]).unwrap_or("");
    log::info!("{label} #{n} len={} {text}", data.len());
}

async fn touch_loop(board: &mut Board<'_>) {
    loop {
        match board.touch.get_touch(&mut board.i2c) {
            Ok(Some(point)) => {
                crate::ui::pointer(&mut board.framebuffer, Some((point.x, point.y)));
            }
            Ok(None) | Err(gt911::Error::NotReady) => {
                crate::ui::pointer(&mut board.framebuffer, None);
            }
            Err(_) => log::info!("touch read failed"),
        }
        Timer::after(Duration::from_millis(50)).await;
    }
}
