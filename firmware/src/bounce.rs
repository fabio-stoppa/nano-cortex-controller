//! DRAM bounce buffers for the RGB scan.
//!
//! DMA cannot read PSRAM at 14 MHz. Six 10-line strips live in internal RAM
//! and a descriptor list one frame long walks them in order. Each end-of-strip
//! interrupt refills the strip that just finished, using the descriptor that
//! raised it so a late copy cannot rotate the picture. The same buffer is not
//! scanned again for five strip-times, so a refill that Bluetooth delayed is
//! not on screen while it is still being copied. A missed interrupt is filled
//! on the next one, oldest first. The first vertical blank restarts DMA at
//! column 0: the LCD startup throws away the pixels already sitting in its
//! FIFO, and without that restart the picture sits to the side.

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use esp_hal::dma::{BurstConfig, DmaDescriptor, DmaTxBuffer, Owner, Preparation};
use esp_hal::interrupt::{self, InterruptHandler, Priority};
use esp_hal::peripherals::Interrupt;
use esp_hal::ram;

pub const WIDTH: usize = 800;
pub const HEIGHT: usize = 480;
pub const BYTES: usize = WIDTH * HEIGHT * 2;

/// Six strips, 10 lines each: the same 96 KB as three 20-line strips, so the
/// radio keeps its stack. The strip count has to divide the frame so the same
/// buffer is waiting at the top of the next frame. Five strip-times of lead
/// is enough to finish a 16 KB copy after the radio has used external memory.
/// One interrupt copies at most two strips; more than that holds the CPU
/// longer than a strip and the queue never drains.
const N_BUFS: usize = 6;
const N_LINES_PER_HALF: usize = 10;
const MAX_COPIES_PER_EOF: usize = 2;
const HALF_BYTES: usize = N_LINES_PER_HALF * WIDTH * 2;
const CHUNK: usize = 4032;
const DESC_PER_HALF: usize = HALF_BYTES.div_ceil(CHUNK);
const HALVES_PER_FRAME: usize = BYTES / HALF_BYTES;
const N_DESC: usize = DESC_PER_HALF * HALVES_PER_FRAME;

const _: () = assert!(BYTES % HALF_BYTES == 0);
const _: () = assert!(HALVES_PER_FRAME % N_BUFS == 0);
const _: () = assert!(HALF_BYTES % 64 == 0);
const _: () = assert!(N_BUFS >= 2);
const _: () = assert!(HALF_BYTES <= 32 * 1024);

#[repr(align(64))]
#[allow(dead_code)]
struct BounceHalf([u8; HALF_BYTES]);

static mut BOUNCE: [BounceHalf; N_BUFS] = [const { BounceHalf([0; HALF_BYTES]) }; N_BUFS];
static mut DESCRIPTORS: [DmaDescriptor; N_DESC] = [DmaDescriptor::EMPTY; N_DESC];

static FRAMEBUFFER: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static EOF_COUNT: AtomicU32 = AtomicU32::new(0);
/// Set after the first blanking-interval rewind, so DMA is not restarted
/// on every frame.
static PHASE_FIXED: AtomicU32 = AtomicU32::new(0);
/// Last strip whose refill has been issued. Starts at the end of the frame
/// so the first interrupt (strip 0) is a gap of one, not the whole frame.
static LAST_HALF: AtomicU32 = AtomicU32::new((HALVES_PER_FRAME - 1) as u32);

pub struct Framebuffer {
    ptr: *mut u8,
}

impl Framebuffer {
    pub fn from_psram(ptr: *mut u8, len: usize) -> Option<Self> {
        if ptr.is_null() || len < BYTES {
            return None;
        }
        Some(Self { ptr })
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr
    }

    pub fn as_pixels_mut(&mut self) -> &mut [u16] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast(), BYTES / 2) }
    }

    pub fn fill_bands(&mut self) {
        let colors = [0xF800u16, 0x07E0, 0x001F, 0xFFFF];
        let pixels = unsafe { core::slice::from_raw_parts_mut(self.ptr, BYTES) };
        let band = BYTES / colors.len();
        for (index, color) in colors.iter().enumerate() {
            let bytes = color.to_le_bytes();
            let start = index * band;
            let end = if index + 1 == colors.len() {
                pixels.len()
            } else {
                start + band
            };
            for chunk in pixels[start..end].chunks_exact_mut(2) {
                chunk.copy_from_slice(&bytes);
            }
        }
    }
}

pub struct BounceRing {
    head: *mut DmaDescriptor,
}

impl BounceRing {
    /// # Safety
    ///
    /// `framebuffer` must stay valid for [`BYTES`] bytes, and this may be
    /// called once per boot.
    pub unsafe fn new(framebuffer: *const u8) -> Self {
        unsafe {
            let descs = ptr::addr_of_mut!(DESCRIPTORS) as *mut DmaDescriptor;
            let bufs = ptr::addr_of_mut!(BOUNCE) as *mut u8;

            for half in 0..HALVES_PER_FRAME {
                let base = bufs.add((half % N_BUFS) * HALF_BYTES);
                let desc_offset = half * DESC_PER_HALF;
                for i in 0..DESC_PER_HALF {
                    let byte_start = i * CHUNK;
                    let chunk_len = (HALF_BYTES - byte_start).min(CHUNK);
                    let descriptor = descs.add(desc_offset + i);
                    (*descriptor).set_size(chunk_len);
                    (*descriptor).set_length(chunk_len);
                    (*descriptor).buffer = base.add(byte_start);
                    (*descriptor).set_owner(Owner::Dma);
                    let end_of_half = i + 1 == DESC_PER_HALF;
                    (*descriptor).set_suc_eof(end_of_half);
                    let next = if end_of_half && half + 1 == HALVES_PER_FRAME {
                        0
                    } else {
                        desc_offset + i + 1
                    };
                    (*descriptor).next = descs.add(next);
                }
            }

            for i in 0..N_BUFS {
                copy_half(framebuffer, i, bufs.add(i * HALF_BYTES));
            }
            FRAMEBUFFER.store(framebuffer as *mut u8, Ordering::Release);
            EOF_COUNT.store(0, Ordering::Relaxed);
            LAST_HALF.store((HALVES_PER_FRAME - 1) as u32, Ordering::Relaxed);
            Self { head: descs }
        }
    }
}

unsafe impl DmaTxBuffer for BounceRing {
    type View = Self;
    type Final = Self;

    fn prepare(&mut self) -> Preparation {
        Preparation {
            start: self.head,
            accesses_psram: false,
            burst_transfer: BurstConfig::default(),
            check_owner: Some(false),
            auto_write_back: false,
        }
    }

    fn into_view(self) -> Self::View {
        self
    }

    fn from_view(view: Self::View) -> Self::Final {
        view
    }
}

/// # Safety
///
/// Call once, after [`BounceRing`] exists and before the DPI transfer starts.
pub unsafe fn enable_eof_interrupt() {
    interrupt::bind_handler(
        Interrupt::DMA_OUT_CH0,
        InterruptHandler::new(eof_isr_trampoline, Priority::Priority3),
    );
    interrupt::bind_handler(
        Interrupt::LCD_CAM,
        InterruptHandler::new(vsync_isr_trampoline, Priority::Priority3),
    );
    let dma = esp_hal::peripherals::DMA::regs();
    dma.ch(0)
        .out_conf0()
        .modify(|_, w| w.out_eof_mode().set_bit());
    dma.ch(0)
        .out_int()
        .ena()
        .modify(|_, w| w.out_eof().set_bit());
    dma.ch(0)
        .out_int()
        .clr()
        .write(|w| w.out_eof().clear_bit_by_one());

    let lcd = esp_hal::peripherals::LCD_CAM::regs();
    lcd.lc_dma_int_clr()
        .write(|w| w.lcd_vsync_int_clr().set_bit());
    lcd.lc_dma_int_ena()
        .modify(|_, w| w.lcd_vsync_int_ena().set_bit());
}

// The handler touches PSRAM. It has to live in IRAM: a flash-resident ISR
// faults with an illegal instruction once the radio uses the cache.
#[ram]
extern "C" fn eof_isr_trampoline() {
    eof_isr();
}

#[ram]
extern "C" fn vsync_isr_trampoline() {
    vsync_isr();
}

pub fn eof_count() -> u32 {
    EOF_COUNT.load(Ordering::Relaxed)
}

#[ram]
fn eof_isr() {
    let dma = esp_hal::peripherals::DMA::regs();
    if !dma.ch(0).out_int().st().read().out_eof().bit_is_set() {
        return;
    }
    // Read the descriptor before clearing. The address register keeps the
    // strip that just finished; clearing only the flag would drop a second
    // strip that completes during the copy.
    let finished = finished_half();
    // Clear before the copy. A strip that finishes during the copy stays
    // pending and the interrupt runs again, instead of being acknowledged
    // with its pixels still old.
    dma.ch(0)
        .out_int()
        .clr()
        .write(|w| w.out_eof().clear_bit_by_one());

    let framebuffer = FRAMEBUFFER.load(Ordering::Acquire);
    if framebuffer.is_null() {
        return;
    }
    // The descriptor that raised this interrupt is the strip that just left
    // the panel. A missed interrupt must not move the count, or the next
    // refill writes the wrong lines and the picture stays wrapped.
    let Some(finished) = finished else {
        return;
    };
    EOF_COUNT.fetch_add(1, Ordering::Relaxed);
    refill_through(framebuffer, finished);
}

/// Fill strips the DMA has finished since the previous interrupt.
///
/// The end-of-strip flag is a single bit, so a copy that runs past the next
/// strip drops the one in between. Walking forward from the last strip we
/// filled repairs that. The buffer on screen right now is left alone. At most
/// [`MAX_COPIES_PER_EOF`] strips are copied; the rest stay pending for the
/// next interrupt, which is what keeps a slow PSRAM stretch from piling up.
#[ram]
fn refill_through(framebuffer: *mut u8, finished: usize) {
    let last = LAST_HALF.load(Ordering::Relaxed) as usize;
    if last == finished {
        return;
    }
    let live = (finished + 1) % N_BUFS;
    let bounce = ptr::addr_of_mut!(BOUNCE) as *mut u8;
    let mut half = (last + 1) % HALVES_PER_FRAME;
    let mut copied = 0usize;
    let mut progressed = last;
    for _ in 0..HALVES_PER_FRAME {
        let buf = half % N_BUFS;
        if buf != live {
            let strip = (half + N_BUFS) % HALVES_PER_FRAME;
            unsafe {
                copy_half_at(framebuffer.add(strip * HALF_BYTES), bounce.add(buf * HALF_BYTES));
            }
            copied += 1;
        }
        progressed = half;
        if half == finished || copied >= MAX_COPIES_PER_EOF {
            break;
        }
        half = (half + 1) % HALVES_PER_FRAME;
    }
    LAST_HALF.store(progressed as u32, Ordering::Relaxed);
}

#[ram]
fn vsync_isr() {
    let lcd = esp_hal::peripherals::LCD_CAM::regs();
    if !lcd.lc_dma_int_st().read().lcd_vsync_int_st().bit_is_set() {
        return;
    }
    lcd.lc_dma_int_clr()
        .write(|w| w.lcd_vsync_int_clr().set_bit());

    if PHASE_FIXED.load(Ordering::Relaxed) != 0 {
        return;
    }
    // Only rewind once the bottom strip has actually finished, so the bounce
    // buffers already hold the top of the next frame and this runs in the blank.
    let Some(half) = finished_half() else {
        return;
    };
    if half + 1 < HALVES_PER_FRAME {
        return;
    }
    rewind_to_left_edge();
    PHASE_FIXED.store(1, Ordering::Relaxed);
}

/// Last strip whose end-of-strip descriptor the DMA reported.
#[ram]
fn finished_half() -> Option<usize> {
    let addr = esp_hal::peripherals::DMA::regs()
        .ch(0)
        .out_eof_des_addr()
        .read()
        .out_eof_des_addr()
        .bits() as usize;
    let base = ptr::addr_of!(DESCRIPTORS) as usize;
    let bytes = addr.wrapping_sub(base);
    let step = core::mem::size_of::<DmaDescriptor>();
    if step == 0 || bytes % step != 0 {
        return None;
    }
    let index = bytes / step;
    if index >= N_DESC {
        return None;
    }
    Some(index / DESC_PER_HALF)
}

/// Drop the pixels the LCD FIFO already took and start the next frame at
/// column 0. No framebuffer copy: the strips for the top of the frame are
/// already in the bounce buffers, and a copy here runs past the blank.
#[ram]
fn rewind_to_left_edge() {
    let dma = esp_hal::peripherals::DMA::regs();
    let ch = dma.ch(0);
    ch.out_link().modify(|_, w| w.outlink_stop().set_bit());
    ch.out_conf0().modify(|_, w| w.out_rst().set_bit());
    ch.out_conf0().modify(|_, w| w.out_rst().clear_bit());
    ch.out_conf0().modify(|_, w| w.out_eof_mode().set_bit());

    let lcd = esp_hal::peripherals::LCD_CAM::regs();
    lcd.lcd_misc()
        .modify(|_, w| w.lcd_afifo_reset().set_bit());
    lcd.lcd_misc()
        .modify(|_, w| w.lcd_afifo_reset().clear_bit());

    ch.out_int()
        .clr()
        .write(|w| w.out_eof().clear_bit_by_one());
    let head = (ptr::addr_of!(DESCRIPTORS) as u32) & 0x000f_ffff;
    ch.out_link()
        .modify(|_, w| unsafe { w.outlink_addr().bits(head) });
    ch.out_link().modify(|_, w| w.outlink_start().set_bit());
    // DMA is back at strip 0. The next interrupt must refill that strip, not
    // treat the restart as a gap covering the whole frame.
    LAST_HALF.store((HALVES_PER_FRAME - 1) as u32, Ordering::Relaxed);
}

#[ram]
unsafe fn copy_half(framebuffer: *const u8, chunk: usize, dest: *mut u8) {
    unsafe { copy_half_at(framebuffer.add(chunk * HALF_BYTES), dest) }
}

unsafe extern "C" {
    /// `dst` and `src` are 16-byte aligned, `len` is a multiple of 64.
    /// Windowed ABI, lives in IRAM. Saves q0–q3 around the vector moves.
    fn copy_psram_block(dst: *mut u8, src: *const u8, len: usize);
}

// Four loads are posted before the matching stores, so PSRAM latency is hidden
// behind the next load. A one-register loop waited on every 16 bytes and a
// strip was still being written when the panel reached it.
core::arch::global_asm!(
    r#"
    .pushsection .rwtext,"ax",@progbits
    .align 4
    .global copy_psram_block
copy_psram_block:
    entry a1, 80
    or a5, a1, a1
    .byte 0x54, 0x01, 0x8a
    .byte 0x54, 0x81, 0x8a
    .byte 0x54, 0x01, 0x9a
    .byte 0x54, 0x81, 0x9a
    srli a4, a4, 6
1:
    .byte 0x34, 0x01, 0x83
    .byte 0x34, 0x81, 0x83
    .byte 0x34, 0x01, 0x93
    .byte 0x34, 0x81, 0x93
    .byte 0x24, 0x01, 0x8a
    .byte 0x24, 0x81, 0x8a
    .byte 0x24, 0x01, 0x9a
    .byte 0x24, 0x81, 0x9a
    addi a4, a4, -1
    bnez a4, 1b
    or a5, a1, a1
    .byte 0x54, 0x01, 0x83
    .byte 0x54, 0x81, 0x83
    .byte 0x54, 0x01, 0x93
    .byte 0x54, 0x81, 0x93
    memw
    retw.n
    .popsection
    "#
);

/// PSRAM to internal RAM.
#[ram]
unsafe fn copy_half_at(src: *const u8, dest: *mut u8) {
    unsafe { copy_psram_block(dest, src, HALF_BYTES) }
}
