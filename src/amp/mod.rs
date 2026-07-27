//! Factory capture catalog + bank helpers.

use core::ffi::c_char;

use nano_wire::ffi::NanoAmpC;

pub const NANO_AMP_CAPTURE_COUNT: u8 = 25;

/// `nano_amp_bank_t` — a C enum, i.e. `int`-sized on Xtensa.
pub const NANO_AMP_BANK_CLEAN: u32 = 0;
pub const NANO_AMP_BANK_2: u32 = 1;
pub const NANO_AMP_BANK_3: u32 = 2;
pub const NANO_AMP_BANK_HIGH_GAIN: u32 = 3;
pub const NANO_AMP_BANK_BASS: u32 = 4;

/// Mirror of `nano_amp_capture_t`.
#[repr(C)]
pub struct NanoAmpCapture {
    pub slot: u8,
    pub name: *const c_char,
    pub bank: u32,
}

// Catalog entries are immutable `const char *` rows shared with the C UI.
unsafe impl Sync for NanoAmpCapture {}

pub const NANO_AMP_CATALOG_COUNT: usize = 25;

#[no_mangle]
pub static nano_amp_catalog: [NanoAmpCapture; NANO_AMP_CATALOG_COUNT] = [
    NanoAmpCapture { slot: 1, name: c"CA John's Ch1".as_ptr(), bank: NANO_AMP_BANK_CLEAN },
    NanoAmpCapture { slot: 2, name: c"Comet 60".as_ptr(), bank: NANO_AMP_BANK_CLEAN },
    NanoAmpCapture { slot: 3, name: c"Bogna X100B Ch1".as_ptr(), bank: NANO_AMP_BANK_CLEAN },
    NanoAmpCapture { slot: 4, name: c"Watt Custom Clean".as_ptr(), bank: NANO_AMP_BANK_CLEAN },
    NanoAmpCapture { slot: 5, name: c"US HRDLX ChA".as_ptr(), bank: NANO_AMP_BANK_CLEAN },
    NanoAmpCapture { slot: 6, name: c"Comet 60".as_ptr(), bank: NANO_AMP_BANK_2 },
    NanoAmpCapture { slot: 7, name: c"NoMatch Chief".as_ptr(), bank: NANO_AMP_BANK_2 },
    NanoAmpCapture { slot: 8, name: c"US Prince 65".as_ptr(), bank: NANO_AMP_BANK_2 },
    NanoAmpCapture { slot: 9, name: c"Watt Custom VH".as_ptr(), bank: NANO_AMP_BANK_2 },
    NanoAmpCapture { slot: 10, name: c"D-Cell H4 Ch2".as_ptr(), bank: NANO_AMP_BANK_2 },
    NanoAmpCapture { slot: 11, name: c"Bogna Fish+290".as_ptr(), bank: NANO_AMP_BANK_3 },
    NanoAmpCapture { slot: 12, name: c"Brit 2555 Rhy".as_ptr(), bank: NANO_AMP_BANK_3 },
    NanoAmpCapture { slot: 13, name: c"Custom 3SE+290".as_ptr(), bank: NANO_AMP_BANK_3 },
    NanoAmpCapture { slot: 14, name: c"D-Cell H4 Ch3".as_ptr(), bank: NANO_AMP_BANK_3 },
    NanoAmpCapture { slot: 15, name: c"ENG Energy".as_ptr(), bank: NANO_AMP_BANK_3 },
    NanoAmpCapture { slot: 16, name: c"Bogna X100B Ch3".as_ptr(), bank: NANO_AMP_BANK_HIGH_GAIN },
    NanoAmpCapture { slot: 17, name: c"CA Studio+290".as_ptr(), bank: NANO_AMP_BANK_HIGH_GAIN },
    NanoAmpCapture { slot: 18, name: c"CA John's Ch3".as_ptr(), bank: NANO_AMP_BANK_HIGH_GAIN },
    NanoAmpCapture { slot: 19, name: c"PV 505Sig".as_ptr(), bank: NANO_AMP_BANK_HIGH_GAIN },
    NanoAmpCapture { slot: 20, name: c"Brit 2555 Lead".as_ptr(), bank: NANO_AMP_BANK_HIGH_GAIN },
    NanoAmpCapture { slot: 21, name: c"Aggi 751".as_ptr(), bank: NANO_AMP_BANK_BASS },
    NanoAmpCapture { slot: 22, name: c"Amped SV Classic".as_ptr(), bank: NANO_AMP_BANK_BASS },
    NanoAmpCapture { slot: 23, name: c"Rodent+SV".as_ptr(), bank: NANO_AMP_BANK_BASS },
    NanoAmpCapture { slot: 24, name: c"CA MixBass".as_ptr(), bank: NANO_AMP_BANK_BASS },
    NanoAmpCapture { slot: 25, name: c"Anima Fuzz".as_ptr(), bank: NANO_AMP_BANK_BASS },
];

#[no_mangle]
pub static nano_amp_catalog_count: usize = NANO_AMP_CATALOG_COUNT;

pub fn catalog() -> &'static [NanoAmpCapture] {
    &nano_amp_catalog
}

fn bank_short_name(bank: u32) -> Option<&'static str> {
    match bank {
        NANO_AMP_BANK_CLEAN => Some("Clean"),
        NANO_AMP_BANK_HIGH_GAIN => Some("High Gain"),
        NANO_AMP_BANK_BASS => Some("Bass"),
        _ => None,
    }
}

/// `strncpy(out, text, cap - 1)` + explicit NUL — truncate and always terminate.
fn write_c_buf(out: *mut c_char, cap: usize, text: &str) {
    if out.is_null() || cap == 0 {
        return;
    }
    let dst = unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), cap) };
    let bytes = text.as_bytes();
    let n = bytes.len().min(cap - 1);
    dst[..n].copy_from_slice(&bytes[..n]);
    dst[n] = 0;
}

#[no_mangle]
pub extern "C" fn nano_amp_bank_from_slot(slot: u8) -> u32 {
    const BANKS: [u32; 5] = [
        NANO_AMP_BANK_CLEAN,
        NANO_AMP_BANK_2,
        NANO_AMP_BANK_3,
        NANO_AMP_BANK_HIGH_GAIN,
        NANO_AMP_BANK_BASS,
    ];

    if slot < 1 || slot > NANO_AMP_CAPTURE_COUNT {
        return NANO_AMP_BANK_CLEAN;
    }
    BANKS[((slot - 1) / 5) as usize]
}

#[no_mangle]
pub unsafe extern "C" fn nano_amp_format_bank_line(slot: u8, out: *mut c_char, cap: usize) {
    if out.is_null() || cap == 0 {
        return;
    }
    unsafe { *out = 0 };
    if slot < 1 || slot > NANO_AMP_CAPTURE_COUNT {
        return;
    }

    let bank = nano_amp_bank_from_slot(slot);
    let bank_num = (slot - 1) / 5 + 1;
    let line = match bank_short_name(bank) {
        Some(name) if !name.is_empty() => format!("Bank {bank_num} - {name}"),
        _ => format!("Bank {bank_num}"),
    };
    write_c_buf(out, cap, &line);
}

#[no_mangle]
pub extern "C" fn nano_amp_bank_label(bank: u32) -> *const c_char {
    match bank {
        NANO_AMP_BANK_CLEAN => c"Bank 1 - Clean".as_ptr(),
        NANO_AMP_BANK_2 => c"Bank 2".as_ptr(),
        NANO_AMP_BANK_3 => c"Bank 3".as_ptr(),
        NANO_AMP_BANK_HIGH_GAIN => c"Bank 4 - High Gain".as_ptr(),
        NANO_AMP_BANK_BASS => c"Bank 5 - Bass".as_ptr(),
        _ => c"Captures".as_ptr(),
    }
}

#[no_mangle]
pub extern "C" fn nano_amp_capture_by_slot(slot: u8) -> *const NanoAmpCapture {
    for entry in catalog() {
        if entry.slot == slot {
            return entry as *const NanoAmpCapture;
        }
    }
    core::ptr::null()
}

/// True for empty names and factory placeholders like `"Capture 12"`.
fn is_generic_capture_name(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }

    let p = name.trim_start_matches(|c: char| c.is_ascii_whitespace());
    if p.len() < 7 || !p[..7].eq_ignore_ascii_case("capture") {
        return false;
    }
    let p = p[7..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    let digits = p.trim_start_matches(|c: char| c.is_ascii_digit());
    if digits.len() == p.len() {
        return false;
    }
    digits.trim_start_matches(|c: char| c.is_ascii_whitespace()).is_empty()
}

/// Read a NUL-terminated fixed buffer coming from a C struct.
fn c_field_str(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end])
}

#[no_mangle]
pub unsafe extern "C" fn nano_amp_capture_header_text(
    amp: *const NanoAmpC,
    out: *mut c_char,
    cap: usize,
) {
    if out.is_null() || cap == 0 {
        return;
    }
    unsafe { *out = 0 };

    let Some(amp) = (unsafe { amp.as_ref() }) else {
        write_c_buf(out, cap, "Amp");
        return;
    };

    let name = c_field_str(&amp.capture_name);
    if amp.have_capture_name && !name.is_empty() && !is_generic_capture_name(&name) {
        write_c_buf(out, cap, &name);
        return;
    }

    if amp.have_capture_slot && amp.capture_slot > 0 {
        let capture = nano_amp_capture_by_slot(amp.capture_slot);
        if let Some(capture) = unsafe { capture.as_ref() } {
            if !capture.name.is_null() {
                let name = unsafe { core::ffi::CStr::from_ptr(capture.name) };
                write_c_buf(out, cap, name.to_str().unwrap_or("Amp"));
                return;
            }
        }
    }

    write_c_buf(out, cap, "Amp");
}

/// Factory capture name for slot 1..25 (`None` when the slot is unknown).
pub fn capture_name(slot: u8) -> Option<&'static str> {
    let capture = unsafe { nano_amp_capture_by_slot(slot).as_ref()? };
    if capture.name.is_null() {
        return None;
    }
    unsafe { core::ffi::CStr::from_ptr(capture.name) }.to_str().ok()
}

/// Touch every exported symbol so rustc does not dead-strip them from the firmware binary.
pub fn ensure_linked() {
    let fns: &[*const ()] = &[
        nano_amp_bank_from_slot as *const (),
        nano_amp_format_bank_line as *const (),
        nano_amp_bank_label as *const (),
        nano_amp_capture_by_slot as *const (),
        nano_amp_capture_header_text as *const (),
    ];
    core::hint::black_box(fns);

    // The C UI walks the catalog directly, so keep both statics too.
    let data: &[*const ()] = &[
        nano_amp_catalog.as_ptr() as *const (),
        &nano_amp_catalog_count as *const usize as *const (),
    ];
    core::hint::black_box(data);
}
