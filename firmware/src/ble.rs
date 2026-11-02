//! Dual BLE central: Neural DSP Nano Cortex and an M-Vave FootCtrl.
//!
//! Scans for the same names as the ESP-IDF client, connects to both, raises
//! the ATT MTU with the 512-byte packet pool, writes the settings read on
//! C304, and subscribes to C305 notify. C306 is the indicate mirror of C305
//! and is left unsubscribed. FootCtrl / FootCtrlPlus use the BLE-MIDI service.
//! The notify queue is 16 deep: each slot is 512 bytes and sits on the main
//! heap-backed GATT client so it does not inflate the main task's stack.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use bt_hci::cmd::le::{LeReadLocalSupportedFeatures, LeSetDataLength, LeSetScanParams};
use bt_hci::controller::ControllerCmdSync;
use core::cell::{Cell, RefCell};
use embassy_futures::join::{join, join3};
use embassy_futures::select::select;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::with_timeout;

#[derive(Clone, Copy)]
pub struct SliderWrite {
    pub id: u8,
    pub raw: u8,
}

pub static SLIDER: Signal<CriticalSectionRawMutex, SliderWrite> = Signal::new();

/// The old host dropped queued notifies here. The link task reads them directly.
pub fn nano_ble_flush_c305_backlog() {}
use embassy_time::{Duration, Instant, Timer};
use trouble_host::prelude::*;

use crate::panel::Board;

mod advertisement;
pub mod connection;
use connection::{Failure, Operation, CONNECT_TIMEOUT, LOAD_TIMEOUT, SETUP_TIMEOUT, WRITE_TIMEOUT};

fn stage(stage: connection::Stage) {
    log::info!("Nano connection: {}", stage.label());
    crate::ui::set_connection_stage(stage.label());
}

async fn bounded<T, E: core::fmt::Debug>(
    operation_kind: Operation,
    deadline: Duration,
    operation: impl core::future::Future<Output = Result<T, E>>,
) -> Result<T, ()> {
    match connection::with_deadline(deadline, operation).await {
        Ok(value) => Ok(value),
        Err(Failure::Bluetooth(error)) => {
            report_failure(operation_kind, &error);
            Err(())
        }
        Err(Failure::Timeout) => {
            report_failure(operation_kind, &"deadline exceeded");
            Err(())
        }
    }
}

fn report_failure(operation: Operation, error: &impl core::fmt::Debug) {
    let message = alloc::format!("{} failed: {error:?}. {}",
        operation.label(), operation.recovery());
    log::error!("{message}");
    crate::ui::set_connection_error(&message);
    crate::screen::nano_ui_set_protocol_status(&message);
}

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

/// A supported device heard during the scan, as listed in the Bluetooth picker.
#[derive(Clone)]
pub struct Seen {
    pub addr: Address,
    pub name: String,
    pub nano: bool,
    /// 0 = nearby, 1 = connecting, 2 = connected.
    pub state: u8,
    seen_at: embassy_time::Instant,
}

/// How long a device stays listed as available after its last advertisement.
const AVAILABLE_FOR: Duration = Duration::from_secs(8);

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static RX_PACKETS: AtomicU32 = AtomicU32::new(0);
static TX_PACKETS: AtomicU32 = AtomicU32::new(0);
static WRITE_FAILURES: AtomicU32 = AtomicU32::new(0);

fn traffic_summary() {
    log::info!("Nano traffic: rx={}, tx={}, write_failures={}",
        RX_PACKETS.load(Ordering::Relaxed),
        TX_PACKETS.load(Ordering::Relaxed),
        WRITE_FAILURES.load(Ordering::Relaxed));
}

type Shared<T> = Mutex<CriticalSectionRawMutex, RefCell<T>>;

const SEEN_MAX: usize = 8;
static SEEN: Shared<Vec<Seen>> = Mutex::new(RefCell::new(Vec::new()));
static PICK_NANO: Shared<Option<Address>> = Mutex::new(RefCell::new(None));
static PICK_FOOT: Shared<Option<Address>> = Mutex::new(RefCell::new(None));
// Set by `forget`: the kind stays disconnected until a device is picked again.
static PAUSED_NANO: AtomicBool = AtomicBool::new(false);
static PAUSED_FOOT: AtomicBool = AtomicBool::new(false);
static NANO_RESTART: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static FOOT_RESTART: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Devices that are linked, being linked, or were heard recently.
pub fn devices() -> Vec<Seen> {
    let now = embassy_time::Instant::now();
    SEEN.lock(|seen| {
        seen.borrow()
            .iter()
            .filter(|d| d.state > 0 || now - d.seen_at < AVAILABLE_FOR)
            .cloned()
            .collect()
    })
}

/// Chooses the device at `index` of [`devices`] and restarts the link to use it.
pub fn pick(index: usize) {
    let Some(device) = devices().get(index).cloned() else {
        return;
    };
    if device.state == 2 {
        return;
    }
    let slot = if device.nano { &PICK_NANO } else { &PICK_FOOT };
    slot.lock(|pick| *pick.borrow_mut() = Some(device.addr));
    paused(device.nano).store(false, Ordering::Relaxed);
    // The two links are independent, so only the chosen one restarts.
    if device.nano {
        NANO_RESTART.signal(());
    } else {
        FOOT_RESTART.signal(());
    }
}

fn paused(nano: bool) -> &'static AtomicBool {
    if nano {
        &PAUSED_NANO
    } else {
        &PAUSED_FOOT
    }
}

/// Disconnects the device at `index` of [`devices`]. It stays listed and can be picked again.
pub fn forget(index: usize) {
    let Some(device) = devices().get(index).cloned() else {
        return;
    };
    if device.state == 0 {
        return;
    }
    let slot = if device.nano { &PICK_NANO } else { &PICK_FOOT };
    slot.lock(|pick| *pick.borrow_mut() = None);
    paused(device.nano).store(true, Ordering::Relaxed);
    if device.nano {
        NANO_RESTART.signal(());
    } else {
        FOOT_RESTART.signal(());
    }
}

fn remember(addr: Address, name: &[u8], nano: bool) {
    SEEN.lock(|seen| {
        let mut seen = seen.borrow_mut();
        if let Some(d) = seen.iter_mut().find(|d| d.addr == addr) {
            d.seen_at = embassy_time::Instant::now();
            if !name.is_empty() && d.name.as_bytes() != name {
                d.name = String::from_utf8_lossy(name).into_owned();
            }
            return;
        }
        if seen.len() >= SEEN_MAX {
            let now = embassy_time::Instant::now();
            let Some(idle) = seen
                .iter()
                .position(|d| d.state == 0 && now - d.seen_at >= AVAILABLE_FOR)
            else {
                return;
            };
            seen.remove(idle);
        }
        seen.push(Seen {
            addr,
            name: if name.is_empty() {
                String::from("Nano Cortex")
            } else {
                String::from_utf8_lossy(name).into_owned()
            },
            nano,
            state: 0,
            seen_at: embassy_time::Instant::now(),
        });
    });
}

fn mark(addr: Address, state: u8) {
    SEEN.lock(|seen| {
        if let Some(d) = seen.borrow_mut().iter_mut().find(|d| d.addr == addr) {
            d.state = state;
        }
    });
}

/// Marks devices of one kind as nearby again after their link ended.
fn reset_state(nano: bool) {
    SEEN.lock(|seen| {
        for d in seen.borrow_mut().iter_mut().filter(|d| d.nano == nano) {
            d.state = 0;
        }
    });
}

fn wanted(slot: &Shared<Option<Address>>, addr: Address) -> bool {
    let nano = core::ptr::eq(slot, &PICK_NANO);
    if paused(nano).load(Ordering::Relaxed) {
        return false;
    }
    slot.lock(|pick| pick.borrow().map_or(true, |p| p == addr))
}

fn is_foot(name: &[u8]) -> bool {
    eq_ignore_ascii_case(name, FOOT_PLUS)
        || eq_ignore_ascii_case(name, FOOT)
        || name.len() >= 9 && eq_ignore_ascii_case(&name[..9], b"chocolate")
}

struct Hunt {
    nano: Signal<NoopRawMutex, Address>,
    foot: Signal<NoopRawMutex, Address>,
    nano_armed: core::sync::atomic::AtomicBool,
    foot_armed: core::sync::atomic::AtomicBool,
    reports: Cell<u32>,
    named_reports: Cell<u32>,
    malformed_reports: Cell<u32>,
    scan_attempts: Cell<u32>,
    samples_left: Cell<u8>,
}

impl Hunt {
    fn new() -> Self {
        Self {
            nano: Signal::new(),
            foot: Signal::new(),
            nano_armed: core::sync::atomic::AtomicBool::new(true),
            foot_armed: core::sync::atomic::AtomicBool::new(true),
            reports: Cell::new(0),
            named_reports: Cell::new(0),
            malformed_reports: Cell::new(0),
            scan_attempts: Cell::new(0),
            samples_left: Cell::new(8),
        }
    }
}

impl EventHandler for Hunt {
    fn on_adv_reports(&self, mut reports: LeAdvReportsIter<'_>) {
        use core::sync::atomic::Ordering;
        while let Some(report) = reports.next() {
            let report = match report {
                Ok(report) => report,
                Err(_) => {
                    self.malformed_reports
                        .set(self.malformed_reports.get().wrapping_add(1));
                    continue;
                }
            };
            self.reports.set(self.reports.get().wrapping_add(1));
            let name = local_name(report.data).unwrap_or_default();
            if !name.is_empty() {
                self.named_reports.set(self.named_reports.get().wrapping_add(1));
            }
            let addr = Address::new(report.addr_kind, report.addr);
            if !name.is_empty() && self.samples_left.get() > 0 {
                self.samples_left.set(self.samples_left.get() - 1);
                log::info!("BLE scan heard {} at {addr}", String::from_utf8_lossy(name));
                log_pkt("BLE discovery data", report.data, None);
            }
            // Service UUIDs and the local name may arrive in separate reports.
            let known_nano = SEEN.lock(|seen| {
                seen.borrow().iter().any(|device| {
                    device.addr == addr && device.nano
                        && (device.state > 0 || Instant::now() - device.seen_at < AVAILABLE_FOR)
                })
            });
            let nano = advertisement::has_nano_service(report.data)
                || name.starts_with(NANO_PREFIX)
                || known_nano;
            let foot = !nano && is_foot(name);
            if !nano && !foot {
                continue;
            }
            remember(addr, name, nano);
            if nano && wanted(&PICK_NANO, addr) && self.nano_armed.swap(false, Ordering::Relaxed) {
                log::info!("found Nano Cortex at {addr}");
                log_pkt("Nano advertisement", report.data, None);
                self.nano.signal(addr);
            } else if foot
                && wanted(&PICK_FOOT, addr)
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
    crate::ui::set_bluetooth_busy();

    let _ = join3(
        async {
            if let Err(error) = runner.run_with_handler(&hunt).await {
                log::error!("ble runner stopped: {error:?}");
                crate::scenes::report(
                    "Bluetooth runner stopped; restart controller and check serial log",
                );
            }
        },
        Box::pin(links(&stack, &hunt)),
        touch_loop(board),
    )
    .await;
}

pub async fn touch_only(board: &mut Board<'_>) {
    touch_loop(board).await;
}

type Radio = embassy_sync::mutex::Mutex<NoopRawMutex, ()>;
type RadioGuard<'a> = embassy_sync::mutex::MutexGuard<'a, NoopRawMutex, ()>;

/// The Nano and the footswitch each get their own loop, so one dropping or
/// reconnecting never touches the other. The radio lock only keeps their scans
/// and connection attempts from overlapping.
async fn links<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    let radio = Radio::new(());
    join(
        Box::pin(nano_loop(stack, hunt, &radio)),
        Box::pin(foot_loop(stack, hunt, &radio)),
    )
    .await;
}

async fn nano_loop<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt, radio: &Radio)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    loop {
        NANO_RESTART.reset();
        RX_PACKETS.store(0, Ordering::Relaxed);
        TX_PACKETS.store(0, Ordering::Relaxed);
        WRITE_FAILURES.store(0, Ordering::Relaxed);
        // GATT construction has large temporary buffers. Keep session polling
        // out of the parent future's stack frame.
        select(Box::pin(nano_session(stack, hunt, radio)), NANO_RESTART.wait()).await;
        traffic_summary();
        crate::link::set_up(false);
        SLIDER.reset();
        LIBRARY_WANTED.store(false, Ordering::Relaxed);
        crate::state::nano_state_reset();
        crate::screen::nano_ui_set_ble_connected(false);
        stage(connection::Stage::Searching);
        reset_state(true);
        log::warn!("nano link ended, scanning again");
        Timer::after(Duration::from_secs(2)).await;
    }
}

async fn foot_loop<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt, radio: &Radio)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    loop {
        FOOT_RESTART.reset();
        select(Box::pin(foot_session(stack, hunt, radio)), FOOT_RESTART.wait()).await;
        reset_state(false);
        log::warn!("footswitch link ended, scanning again");
        Timer::after(Duration::from_secs(1)).await;
    }
}

/// Scans for up to three seconds for one kind of device. Hold the radio lock while calling.
async fn scan_for<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt, nano: bool) -> Option<Address>
where
    C: Controller + ControllerCmdSync<LeSetScanParams>,
{
    use core::sync::atomic::Ordering;
    hunt.nano_armed.store(nano, Ordering::Relaxed);
    hunt.foot_armed.store(!nano, Ordering::Relaxed);
    let signal = if nano { &hunt.nano } else { &hunt.foot };
    signal.reset();
    let attempt = hunt.scan_attempts.get().wrapping_add(1);
    hunt.scan_attempts.set(attempt);
    let report_counts = (
        hunt.reports.get(),
        hunt.named_reports.get(),
        hunt.malformed_reports.get(),
    );
    let mut scanner = Scanner::new(stack.central());
    let config = listen(&[]);
    let scan = match with_timeout(Duration::from_secs(5), scanner.scan(&config)).await {
        Ok(Ok(scan)) => scan,
        Ok(Err(error)) => {
            log::warn!("scan start failed: {error:?}");
            if nano { report_failure(Operation::Scan, &error); }
            return None;
        }
        Err(_) => {
            log::error!("BLE scan start timed out waiting for host/controller; restart and check HCI");
            if nano { report_failure(Operation::Scan, &"controller deadline exceeded"); }
            return None;
        }
    };
    if attempt <= 4 {
        log::info!(
            "BLE active scan started for {}",
            if nano { "Nano" } else { "footswitch" }
        );
    }
    let found = with_timeout(Duration::from_secs(3), signal.wait()).await.ok();
    if found.is_none() && (attempt <= 4 || attempt % 10 == 0) {
        log::warn!(
            "BLE scan #{attempt}: no {}; advertisements={}, named={}, malformed={}",
            if nano { "Nano" } else { "footswitch" },
            hunt.reports.get().wrapping_sub(report_counts.0),
            hunt.named_reports.get().wrapping_sub(report_counts.1),
            hunt.malformed_reports.get().wrapping_sub(report_counts.2),
        );
    }
    // Dropping the session asks the runner to disable scanning.
    drop(scan);
    drop(scanner);
    Timer::after(Duration::from_millis(200)).await;
    found
}

/// Waits for a device of one kind and returns its address with the radio lock still held.
async fn find<'a, C>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    hunt: &Hunt,
    radio: &'a Radio,
    nano: bool,
) -> (Address, RadioGuard<'a>)
where
    C: Controller + ControllerCmdSync<LeSetScanParams>,
{
    loop {
        let guard = radio.lock().await;
        if let Some(addr) = scan_for(stack, hunt, nano).await {
            return (addr, guard);
        }
        drop(guard);
        // Gives the other link a turn at the radio.
        Timer::after(Duration::from_millis(300)).await;
    }
}

async fn nano_session<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt, radio: &Radio)
where
    C: Controller
        + ControllerCmdSync<LeSetScanParams>
        + ControllerCmdSync<LeSetDataLength>
        + ControllerCmdSync<LeReadLocalSupportedFeatures>,
{
    stage(connection::Stage::Searching);
    log::info!("scanning for nano");
    let (nano, guard) = find(stack, hunt, radio, true).await;

    let mut central = stack.central();
    let nano_filter = [nano];
    let connect = ConnectConfig {
        connect_params: peer_params(),
        scan_config: listen(&nano_filter),
    };
    log::info!("connecting to nano");
    stage(connection::Stage::Connecting);
    mark(nano, 1);
    let conn = bounded(Operation::Connect, CONNECT_TIMEOUT, central.connect(&connect)).await;
    drop(guard);
    let conn = match conn {
        Ok(conn) => conn,
        Err(_) => {
            log::warn!("nano connect failed");
            return;
        }
    };
    let interval_us = conn.params().conn_interval.as_micros();
    log::info!("nano connected, interval {interval_us} us");
    mark(nano, 2);
    stage(connection::Stage::Discovering);
    // One 251-byte PDU is about 2.1 ms. Without this, the link stays at
    // 27-byte PDUs and one event still carries more packets than the
    // controller can buffer.
    match connection::with_deadline(SETUP_TIMEOUT, conn.update_data_length(stack, 251, 2120)).await {
        Ok(()) => log::info!("data length 251 requested"),
        Err(Failure::Bluetooth(error)) => {
            log::warn!("Data length update rejected; keeping negotiated default: {error:?}");
        }
        Err(Failure::Timeout) => {
            report_failure(Operation::DataLength, &"controller deadline exceeded");
            return;
        }
    };

    let client = match bounded(
        Operation::Gatt, SETUP_TIMEOUT,
        Box::pin(GattClient::<C, DefaultPacketPool, 8>::new(stack, &conn)),
    ).await {
        Ok(client) => client,
        Err(_) => {
            log::warn!("nano gatt client failed");
            return;
        }
    };
    let client = Box::new(client);
    log::info!("nano ATT MTU {}", conn.att_mtu());

    // Ends when the Nano drops, so `nano_loop` scans again.
    let _ = select(
        async {
            if let Err(error) = client.task().await {
                report_failure(Operation::Receive, &error);
            }
        },
        nano_link(&client),
    )
    .await;
}

async fn foot_session<C>(stack: &Stack<'_, C, DefaultPacketPool>, hunt: &Hunt, radio: &Radio)
where
    C: Controller + ControllerCmdSync<LeSetScanParams>,
{
    let (addr, guard) = find(stack, hunt, radio, false).await;
    foot_link(stack, addr, guard).await;
}

async fn nano_link<C: Controller, P: PacketPool>(client: &GattClient<'_, C, P, 8>) {
    let services = match bounded(Operation::Services, SETUP_TIMEOUT, client.services()).await {
        Ok(services) => services,
        Err(()) => return,
    };
    let required = [UUID_A002, UUID_A003].map(|uuid|
        services.iter().any(|svc| svc.uuid() == Uuid::new_short(uuid)));
    if let Err(error) = connection::validate_capabilities(required[0], required[1], true, true) {
        report_failure(Operation::Services, &error);
        return;
    }
    let mut c304 = None;
    let mut c305 = None;
    for service in services.iter().filter(|svc|
        svc.uuid() == Uuid::new_short(UUID_A002) || svc.uuid() == Uuid::new_short(UUID_A003))
    {
        let characteristics = match bounded(Operation::Characteristics, SETUP_TIMEOUT,
            client.characteristics::<16>(service)).await {
            Ok(characteristics) => characteristics,
            Err(()) => return,
        };
        for ch in characteristics {
            if ch.uuid == Uuid::new_short(UUID_C304) {
                c304 = Some(ch);
            } else if ch.uuid == Uuid::new_short(UUID_C305) {
                c305 = Some(ch);
            } else if ch.uuid == Uuid::new_short(UUID_C306) {
                log::info!("C306 handle {} left unsubscribed", ch.handle);
            }
        }
    }
    let Some(c304) = c304 else {
        report_failure(Operation::Characteristics, &"C304 characteristic missing");
        return;
    };
    let Some(c305) = c305 else {
        report_failure(Operation::Characteristics, &"C305 characteristic missing");
        return;
    };
    log::info!("C304 handle {} C305 handle {}", c304.handle, c305.handle);
    let c304_no_rsp = c304.props.any(&[CharacteristicProp::WriteWithoutResponse]);
    if let Err(error) = connection::validate_capabilities(
        required[0],
        required[1],
        c304.props.any(&[CharacteristicProp::Write]),
        c305.props.any(&[CharacteristicProp::Notify]),
    ) {
        report_failure(Operation::Characteristics, &error);
        return;
    }
    log::info!(
        "C304 write without response {}",
        if c304_no_rsp { "yes" } else { "no" }
    );

    stage(connection::Stage::Subscribing);
    let mut listener = match bounded(Operation::Subscribe, SETUP_TIMEOUT, client.subscribe(&c305, false)).await {
        Ok(listener) => listener,
        Err(_) => {
            log::warn!("C305 subscribe failed");
            return;
        }
    };

    connection::run_io(
        receive_notifications(&mut listener),
        nano_commands(client, &c304, c304_no_rsp),
    ).await;
}

async fn receive_notifications(listener: &mut NotificationListener<'_, 512>) {
    let mut seen = 0u32;
    loop {
        let note = listener.next().await;
        RX_PACKETS.fetch_add(1, Ordering::Relaxed);
        seen = seen.wrapping_add(1);
        if seen <= 4 || seen % 32 == 0 {
            log_pkt("C305 RX", note.as_ref(), Some(seen));
        }
        crate::state::feed_c305_bytes(note.as_ref());
        embassy_futures::yield_now().await;
    }
}

async fn nano_commands<C: Controller, P: PacketPool>(
    client: &GattClient<'_, C, P, 8>,
    c304: &Characteristic<[u8]>,
    c304_no_rsp: bool,
) {
    stage(connection::Stage::Loading);
    SLIDER.reset();
    LIBRARY_WANTED.store(false, Ordering::Relaxed);
    crate::link::set_up(true);
    crate::screen::nano_ui_set_ble_connected(true);
    let initial_snapshot = crate::state::snapshot_generation();
    let initial_library = crate::state::library_generation();
    let mut library_asked = false;
    let mut ready = false;
    let loading_started = Instant::now();
    let mut next_summary = loading_started + Duration::from_secs(10);
    crate::state::nano_state_reset();
    let mut payload = [0u8; 16];
    let n = nano_wire::editor::read_settings_payload(&mut payload);
    if n > 0 {
        match write_packet(client, c304.handle, false, &payload[..n]).await {
            Ok(()) => {
                log::info!("C304 read-settings write ({n} bytes)");
                log_pkt("C304 TX settings", &payload[..n], None);
            }
            Err(error) => {
                log::error!("Initial C304 settings write failed: {error:?}");
                return;
            }
        }
    }
    crate::state::nano_state_begin_sync();
    if flush_writes(client, c304.handle, c304_no_rsp).await.is_err() { return; }

    loop {
        if Instant::now() >= next_summary {
            traffic_summary();
            next_summary = Instant::now() + Duration::from_secs(10);
        }
        if !ready && Instant::now() - loading_started >= LOAD_TIMEOUT {
            let missing = if crate::state::snapshot_generation() == initial_snapshot {
                "complete initial state snapshot timed out"
            } else if crate::state::nano_state_preset_names().is_empty() {
                "initial preset catalog timed out"
            } else {
                "initial library timed out"
            };
            report_failure(Operation::Loading, &missing);
            return;
        }
        match with_timeout(Duration::from_millis(10), SLIDER.wait()).await {
            Ok(write) => {
                if !ready {
                    log::warn!("Slider edit refused while Nano is loading");
                    continue;
                }
                if crate::scenes::busy() {
                    crate::scenes::report("Slider ignored while scene recall is running");
                    continue;
                }
                crate::scenes::sound_edited();
                let mut buf = [0u8; 24];
                let n = slider_payload(write, &mut buf);
                if n > 0 {
                    let result = write_packet(client, c304.handle, c304_no_rsp, &buf[..n]).await;
                    match result {
                        Ok(()) => {
                            log::debug!("slider {} = {}", write.id, write.raw);
                            log_pkt("C304 TX slider", &buf[..n], None);
                        }
                        Err(()) => return,
                    }
                }
            }
            Err(_) => {}
        }
        for _ in 0..8 {
            if !ready || crate::scenes::busy() { break; }
            let Some(cmd) = crate::link::pop_cmd() else { break; };
            apply_cmd(cmd);
            if flush_writes(client, c304.handle, c304_no_rsp).await.is_err() { return; }
            embassy_futures::yield_now().await;
        }
        if let Some(cmd) = crate::scenes::poll() {
            apply_cmd(cmd);
            if flush_writes(client, c304.handle, c304_no_rsp).await.is_err() { return; }
        }
        if LIBRARY_WANTED.load(core::sync::atomic::Ordering::Relaxed)
            && crate::state::nano_rx_mode_get() == 0
            && !crate::scenes::busy()
        {
            LIBRARY_WANTED.store(false, core::sync::atomic::Ordering::Relaxed);
            let sent = crate::state::nano_state_request_library();
            log::info!("Library request sent={sent}");
        }
        if let Some(param) = crate::link::take_param() {
            if !ready {
                log::warn!("Parameter edit refused while Nano is loading");
            } else if crate::scenes::busy() {
                crate::scenes::report("Parameter edit ignored while scene recall is running");
            } else {
                crate::scenes::sound_edited();
                if !crate::fx::nano_fx_write_param_normalized(
                    u32::from(param.block),
                    param.wire,
                    param.norm,
                ) {
                    log::warn!("FX parameter edit refused");
                }
            }
        }
        crate::state::poll();
        if !ready {
            let snapshot = crate::state::snapshot_generation() != initial_snapshot;
            let presets = !crate::state::nano_state_preset_names().is_empty();
            if snapshot && presets && !library_asked && crate::state::nano_rx_mode_get() == 0 {
                library_asked = crate::state::nano_state_request_library();
                if !library_asked {
                    report_failure(Operation::Loading, &"initial library request refused");
                    return;
                }
            }
            if connection::initial_state_loaded(
                snapshot,
                presets,
                crate::state::library_generation() != initial_library,
            ) {
                ready = true;
                stage(connection::Stage::Ready);
            }
        }
        if crate::scenes::autosave_due() {
            let result = crate::scenes::autosave_selected();
            match result {
                Err(error) => crate::scenes::report(error),
                Ok(()) => {
                    if crate::state::nano_state_preset_dirty() {
                        if crate::state::nano_state_save_preset() {
                            crate::scenes::report("Autosave sent to Nano");
                        } else {
                            crate::scenes::report("Autosave failed: Nano preset save was refused");
                        }
                    } else {
                        crate::scenes::report("Autosave complete");
                    }
                }
            }
        }
        if flush_writes(client, c304.handle, c304_no_rsp).await.is_err() { return; }
    }
}

/// Set by the picker; the request goes out once no state transfer is running.
pub static LIBRARY_WANTED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

fn apply_cmd(cmd: crate::link::Cmd) {
    if !crate::scenes::busy() && matches!(&cmd,
        crate::link::Cmd::Param(_) | crate::link::Cmd::Block { .. }
        | crate::link::Cmd::Model { .. } | crate::link::Cmd::ModelAt { .. }
        | crate::link::Cmd::Gate(_) | crate::link::Cmd::GateAmount(_)
        | crate::link::Cmd::AmpKnob { .. } | crate::link::Cmd::CaptureVolume(_)
        | crate::link::Cmd::CabParam { .. } | crate::link::Cmd::Capture(_)
        | crate::link::Cmd::CaptureAt(_) | crate::link::Cmd::CabAt(_)
    ) {
        crate::scenes::sound_edited();
    }
    match cmd {
        crate::link::Cmd::Gig(enabled) => crate::ui::show_gig(enabled),
        crate::link::Cmd::SceneRecall(index) => {
            if let Err(error) = crate::scenes::recall_scene(usize::from(index)) { crate::scenes::report(error); }
            else { SLIDER.reset(); crate::link::take_param(); }
        }
        crate::link::Cmd::SceneRecallAt { preset, scene } => {
            if preset != crate::state::nano_state_preset_slot() {
                crate::scenes::report("Nano preset changed; reopen the scene selector");
            } else if let Err(error) = crate::scenes::recall_scene(usize::from(scene)) {
                crate::scenes::report(error);
            } else { SLIDER.reset(); crate::link::take_param(); }
        }
        crate::link::Cmd::SceneCapture { preset, scene, name } => {
            if let Err(error) = crate::scenes::capture_scene(preset, usize::from(scene), &name) {
                crate::scenes::report(error);
            }
        }
        crate::link::Cmd::SceneRefresh => {
            crate::fx::nano_fx_clear_model_trust();
            crate::state::nano_state_request_preset_refresh("Scene recall");
        }
        crate::link::Cmd::SceneFxRefresh => {
            crate::fx::nano_fx_refresh_all();
            if let Some(slot) = crate::scenes::view().amp.active_cab_slot() {
                if !crate::state::nano_state_read_cab_settings(slot) {
                    crate::scenes::cancel("Cab readback request failed; sound may be partially updated");
                }
            }
        }
        crate::link::Cmd::FxModelId { block, id } => {
            let sent = crate::fx::models::find_model(&id)
                .is_some_and(|model| crate::fx::nano_fx_write_block_model(u32::from(block), model));
            if !sent { crate::scenes::cancel("FX model recall refused; sound may be partially updated"); }
        }
        crate::link::Cmd::Gate(on) => {
            if !crate::fx::nano_fx_write_gate_enabled(on) {
                log::warn!("Gate toggle write failed");
                crate::scenes::cancel("Gate recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::GateAmount(pct) => {
            if !crate::fx::nano_fx_write_gate_reduction(pct) {
                log::warn!("MIDI gate amount write failed");
                crate::scenes::cancel("Gate amount recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::TunerToggle => crate::ui::midi_toggle_tuner(),
        crate::link::Cmd::MidiAction { index, value, continuous } => {
            if let Some(cmd) = crate::midi::command(index, value, continuous) {
                apply_cmd(cmd);
            }
        }
        crate::link::Cmd::Param(param) => {
            if !crate::fx::nano_fx_write_param_normalized(u32::from(param.block), param.wire, param.norm) {
                log::warn!("MIDI FX parameter write failed");
                crate::scenes::cancel("FX parameter recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::Tuner(on) => {
            let _ = crate::tuner::nano_tuner_set_active(on);
        }
        crate::link::Cmd::TunerReference(hz) => {
            let settings = crate::state::nano_state_tuner_settings();
            let reference_hz = f32::from(hz).clamp(400.0, 480.0);
            write_tuner_settings(reference_hz, settings.muted.unwrap_or(false));
        }
        crate::link::Cmd::TunerOutputMute(muted) => {
            let settings = crate::state::nano_state_tuner_settings();
            write_tuner_settings(settings.reference_hz.unwrap_or(440.0), muted);
        }
        crate::link::Cmd::Tempo => {
            if !crate::state::nano_state_tempo_tap() {
                log::warn!("Tap tempo write failed");
            }
        }
        crate::link::Cmd::Focus(slot) => crate::fx::nano_fx_focus_from_ui(slot),
        crate::link::Cmd::Block { slot, on } => {
            if !crate::fx::nano_fx_write_block_enabled(u32::from(slot), on) {
                log::warn!("FX toggle write failed");
                crate::scenes::cancel("FX bypass recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::Model { block, delta } => step_model(block, delta),
        crate::link::Cmd::ModelAt { block, index } => {
            if let Some(model) =
                crate::fx::nano_fx_get_allowed_model(u32::from(block), usize::from(index))
            {
                let _ = crate::fx::nano_fx_write_block_model(u32::from(block), model);
            }
        }
        crate::link::Cmd::Save => {
            if !crate::state::nano_state_save_preset() {
                log::warn!("Preset save failed");
            }
        }
        crate::link::Cmd::Copy => {
            let _ = crate::state::nano_state_copy_current();
        }
        crate::link::Cmd::Library => {
            // Wait for any state transfer to finish so the two streams don't mix.
            LIBRARY_WANTED.store(true, core::sync::atomic::Ordering::Relaxed);
        }
        crate::link::Cmd::Preset(delta) => {
            if !crate::state::nano_state_step_preset(delta) {
                log::warn!("Preset step failed");
            }
        }
        crate::link::Cmd::Capture(delta) => {
            if !crate::state::nano_state_step_capture(delta) {
                log::warn!("Capture step failed");
            }
        }
        crate::link::Cmd::PresetAt(slot) => {
            if !crate::state::nano_state_select_preset(slot) {
                log::warn!("Preset selection failed");
            }
        }
        crate::link::Cmd::PresetRename { slot, name } => {
            if !crate::state::nano_state_rename_preset(slot, &name) {
                log::warn!("Preset rename refused");
            }
        }
        crate::link::Cmd::PresetDelete(slot) => {
            if !crate::state::nano_state_delete_preset(slot) {
                log::warn!("Preset delete failed");
            }
        }
        crate::link::Cmd::PresetTransfer {
            src,
            dst,
            remove_src,
        } => {
            if !crate::state::nano_state_transfer_preset(src, dst, remove_src) {
                log::warn!("Preset transfer failed");
            }
        }
        crate::link::Cmd::CaptureAt(slot) => {
            if !crate::state::nano_state_write_capture_slot(slot) {
                log::warn!("Capture selection failed");
            }
        }
        crate::link::Cmd::CabAt(position) => {
            let library = crate::state::nano_state_library_snapshot();
            let Some(cab) = library.cabs.get(usize::from(position)) else {
                return;
            };
            let mut amp = nano_wire::types::NanoAmpC::from(&nano_wire::types::Amp::default());
            crate::state::nano_state_snapshot_amp(&mut amp);
            let slot = amp.active_cab_slot().unwrap_or(1);
            let kind = if cab.user {
                nano_wire::editor::LibraryKind::User
            } else {
                nano_wire::editor::LibraryKind::Factory
            };
            let _ = crate::state::nano_state_load_library_cab(slot, kind, cab.index, &cab.name);
        }
        crate::link::Cmd::CabParam { which, normalized } => {
            if !crate::state::nano_state_write_cab_float(which, normalized) {
                log::warn!("Cab parameter write failed");
                crate::scenes::cancel("Cab recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::AmpKnob { knob, raw } => {
            if !crate::state::nano_state_write_amp_knob(u32::from(knob), raw) {
                log::warn!("MIDI amp control write failed");
                crate::scenes::cancel("Amp recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::CaptureVolume(raw) => {
            if !crate::state::nano_state_write_capture_volume(raw) {
                log::warn!("MIDI capture volume write failed");
                crate::scenes::cancel("Capture level recall failed; sound may be partially updated");
            }
        }
        crate::link::Cmd::ExpressionSave { slot, assignments } => {
            if !crate::state::nano_state_write_expression(
                slot,
                &assignments.ranges,
                &assignments.bypasses,
            ) {
                log::warn!("Expression assignment write failed");
            } else if !crate::state::nano_state_read_expression(slot) {
                log::warn!("Expression assignment verification request failed");
            }
        }
        crate::link::Cmd::ToolsOpen => {
            if !crate::state::nano_state_tools_open() {
                log::warn!("Tools refresh could not send every request");
            }
        }
        crate::link::Cmd::JackMode(mode) => {
            if !crate::state::nano_state_write_jack_mode(mode) {
                log::warn!("EXP/MIDI jack setting write failed");
            }
        }
        crate::link::Cmd::UsbPlayback(db) => {
            if !crate::state::nano_state_write_usb_playback(libm::roundf(db).clamp(-40.0, 0.0)) {
                log::warn!("USB playback setting write failed");
            }
        }
        crate::link::Cmd::Mute { input, muted } => {
            let which = if input {
                nano_wire::editor::MuteTarget::Input
            } else {
                nano_wire::editor::MuteTarget::Output
            };
            if !crate::state::nano_state_write_mute(which, muted) {
                log::warn!("Mute setting write failed");
            }
        }
        crate::link::Cmd::ExpressionCalibrationStart => {
            if !crate::state::nano_state_expression_calibration_start() {
                log::warn!("Expression calibration could not start");
            }
        }
        crate::link::Cmd::ExpressionCalibrationSave => {
            if !crate::state::nano_state_expression_calibration_save() {
                log::warn!("Expression calibration needs a full pedal sweep before saving");
            }
        }
    }
}

fn write_tuner_settings(reference_hz: f32, muted: bool) {
    let active = crate::tuner::nano_tuner_is_active();
    let live = crate::tuner::nano_tuner_is_live();
    if crate::state::nano_state_set_tuner_reference(active, reference_hz, live, muted) {
        crate::screen::nano_ui_set_tuner_settings(reference_hz, muted);
    } else {
        log::warn!("tuner settings write failed");
        let settings = crate::state::nano_state_tuner_settings();
        crate::screen::nano_ui_set_tuner_settings(
            settings.reference_hz.unwrap_or(440.0),
            settings.muted.unwrap_or(false),
        );
    }
}

fn step_model(block: u8, delta: i8) {
    if delta == 0 || block as usize >= crate::fx::types::NANO_FX_BLOCK_COUNT {
        log::warn!("Invalid FX model step");
        return;
    }
    let mut fx = crate::fx::types::NanoFxState::zeroed();
    if !crate::fx::nano_fx_snapshot(&mut fx) {
        log::warn!("FX model step needs current FX state");
        return;
    }
    let count = crate::fx::nano_fx_allowed_model_count(u32::from(block));
    if count == 0 {
        log::warn!("No allowed models for FX block {block}");
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
        if !crate::fx::nano_fx_write_block_model(u32::from(block), model) {
            log::warn!("FX model step write failed");
        }
    } else {
        log::warn!("FX model step could not resolve model");
    }
}

async fn write_packet<C: Controller, P: PacketPool>(
    client: &GattClient<'_, C, P, 8>,
    handle: u16,
    no_response_supported: bool,
    packet: &[u8],
) -> Result<(), ()> {
    let result = bounded(Operation::Write, WRITE_TIMEOUT, async {
        if no_response_supported && nano_wire::editor::is_live_value_write(packet) {
            client.write_handle_without_response(handle, packet).await
        } else {
            client.write_handle(handle, packet).await
        }
    }).await;
    if result.is_ok() {
        TX_PACKETS.fetch_add(1, Ordering::Relaxed);
    } else {
        WRITE_FAILURES.fetch_add(1, Ordering::Relaxed);
    }
    result
}

async fn flush_writes<C: Controller, P: PacketPool>(
    client: &GattClient<'_, C, P, 8>,
    handle: u16,
    no_response_supported: bool,
) -> Result<(), ()> {
    while let Some(packet) = crate::link::pop_write() {
        let result = write_packet(client, handle, no_response_supported, &packet).await;
        match result {
            Ok(()) => {
                log::debug!("C304 write {} bytes", packet.len());
                log_pkt("C304 TX", &packet, None);
                embassy_futures::yield_now().await;
            }
            Err(error) => {
                log::warn!("C304 write failed: {error:?}");
                crate::scenes::cancel("Bluetooth write failed; recall may be partially applied");
                return Err(());
            }
        }
    }
    Ok(())
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

async fn foot_link<C: Controller>(
    stack: &Stack<'_, C, DefaultPacketPool>,
    addr: Address,
    guard: RadioGuard<'_>,
) {
    mark(addr, 1);
    let mut central = stack.central();
    let foot_filter = [addr];
    let connect = ConnectConfig {
        connect_params: peer_params(),
        scan_config: listen(&foot_filter),
    };
    let conn = central.connect(&connect).await;
    drop(guard);
    let conn = match conn {
        Ok(conn) => conn,
        Err(_) => {
            log::warn!("footswitch connect failed");
            return;
        }
    };
    mark(addr, 2);
    log::info!("footswitch connected");
    let client = match Box::pin(GattClient::<C, DefaultPacketPool, 4>::new(stack, &conn)).await {
        Ok(client) => client,
        Err(_) => {
            log::warn!("footswitch gatt client failed");
            return;
        }
    };
    let client = Box::new(client);

    let _ = select(
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
                    return;
                }
            };
            let Some(svc) = svcs.first() else {
                log::warn!("midi service missing");
                return;
            };
            let io = match client
                .characteristic_by_uuid::<[u8]>(svc, &Uuid::new_long(MIDI_IO))
                .await
            {
                Ok(io) => io,
                Err(_) => {
                    log::warn!("midi io missing");
                    return;
                }
            };
            let mut listener = match client.subscribe(&io, false).await {
                Ok(listener) => listener,
                Err(_) => {
                    log::warn!("midi subscribe failed");
                    return;
                }
            };
            log::info!("midi io handle {}", io.handle);
            let mut seen = 0u32;
            loop {
                let note = listener.next().await;
                seen = seen.wrapping_add(1);
                log_pkt("MIDI RX", note.as_ref(), Some(seen));
                crate::midi::feed(note.as_ref());
            }
        },
    )
    .await;
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

fn log_pkt(label: &str, data: &[u8], packet_number: Option<u32>) {
    if !log::log_enabled!(log::Level::Debug) {
        return;
    }
    let digits = b"0123456789ABCDEF";
    for (chunk_index, chunk) in data.chunks(16).enumerate() {
        let mut hex = [0u8; 48];
        let mut o = 0;
        for b in chunk {
            hex[o] = digits[(b >> 4) as usize];
            hex[o + 1] = digits[(b & 0x0f) as usize];
            hex[o + 2] = b' ';
            o += 3;
        }
        let text = core::str::from_utf8(&hex[..o]).unwrap_or("");
        let offset = chunk_index * 16;
        match packet_number {
            Some(n) => log::debug!("{label} #{n} len={} offset={offset} {text}", data.len()),
            None => log::debug!("{label} len={} offset={offset} {text}", data.len()),
        }
    }
}

async fn touch_loop(board: &mut Board<'_>) {
    #[cfg(feature = "scan-diag")]
    let mut tick = 0u32;
    let mut ticks = 0u32;
    let mut next_refresh = Instant::now();
    loop {
        if Instant::now() >= next_refresh {
            next_refresh = Instant::now() + Duration::from_millis(50);
            ticks = ticks.wrapping_add(1);
            if ticks % 3 == 0 {
                crate::ui::refresh_bluetooth(&devices());
            }
            crate::ui::refresh_midi();
            #[cfg(feature = "scan-diag")]
            {
                tick = tick.wrapping_add(1);
                if tick % 20 == 0 {
                    crate::bounce::report_diagnostics();
                }
            }
        }
        match board.touch.get_touch(&mut board.i2c) {
            Ok(Some(point)) => {
                crate::ui::pointer(&mut board.framebuffer, Some((point.x, point.y)));
            }
            Ok(None) => crate::ui::pointer(&mut board.framebuffer, None),
            // No new sample yet; the finger is not necessarily lifted.
            Err(gt911::Error::NotReady) => crate::ui::frame(&mut board.framebuffer),
            Err(_) => log::info!("touch read failed"),
        }
        let poll_ms = if crate::ui::keyboard_open() { 10 } else { 50 };
        Timer::after(Duration::from_millis(poll_ms)).await;
    }
}
