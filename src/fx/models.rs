//! Catalog lookups, categories and the display/acronym helpers
//! (port of the model-side helpers in `nano_fx.c`).

use core::ffi::{c_char, CStr};
use core::ptr;

use super::c_deps::{c_str, write_c_buf};
use super::catalog::catalog;
use super::types::*;

pub const BLOCK_LABELS: [&str; NANO_FX_BLOCK_COUNT] = [
    "Pre FX 1",
    "Pre FX 2",
    "Post FX 1",
    "Post FX 2",
    "Post FX 3",
];

const BLOCK_LABELS_C: [&CStr; NANO_FX_BLOCK_COUNT] = [
    c"Pre FX 1",
    c"Pre FX 2",
    c"Post FX 1",
    c"Post FX 2",
    c"Post FX 3",
];

pub fn block_label(block: u32) -> &'static str {
    BLOCK_LABELS.get(block as usize).copied().unwrap_or("?")
}

pub fn model_name(model: &NanoFxModelDef) -> &'static str {
    c_str(model.name)
}

pub fn model_ptr(model: Option<&'static NanoFxModelDef>) -> *const NanoFxModelDef {
    model.map_or(ptr::null(), |m| m as *const NanoFxModelDef)
}

pub fn model_ref(model: *const NanoFxModelDef) -> Option<&'static NanoFxModelDef> {
    unsafe { model.as_ref() }
}

pub fn catalog_index(model: &NanoFxModelDef) -> Option<usize> {
    catalog().iter().position(|entry| ptr::eq(entry, model))
}

pub fn find_model(id: &[u8]) -> Option<&'static NanoFxModelDef> {
    if id.is_empty() {
        return None;
    }
    catalog().iter().find(|entry| entry.id_bytes() == id)
}

pub fn find_model_by_name(name: &str) -> Option<&'static NanoFxModelDef> {
    catalog().iter().find(|entry| model_name(entry) == name)
}

/// Exact ID match first, then any catalog ID embedded anywhere in the blob.
pub fn find_model_in_blob(raw: &[u8]) -> Option<&'static NanoFxModelDef> {
    if raw.is_empty() {
        return None;
    }

    if let Some(direct) = find_model(raw) {
        return Some(direct);
    }

    for entry in catalog() {
        let id = entry.id_bytes();
        if id.is_empty() || raw.len() < id.len() {
            continue;
        }
        if raw.windows(id.len()).any(|window| window == id) {
            return Some(entry);
        }
    }

    None
}

pub fn find_model_id_in_preset(
    data: &[u8],
    id: &[u8],
) -> Option<&'static NanoFxModelDef> {
    if id.is_empty() || data.len() < id.len() {
        return None;
    }
    if data.windows(id.len()).any(|window| window == id) {
        find_model(id)
    } else {
        None
    }
}

/// Map firmware preset varints that are not catalog wire IDs (e.g. 25 = Transpose).
pub fn resolve_legacy_model(block: u32, val: u8) -> Option<&'static NanoFxModelDef> {
    if let Some(direct) = find_model(&[val]) {
        return Some(direct);
    }

    const FIRMWARE_ENUM: [(u32, u8, &str); 2] = [
        (NANO_FX_PRE1, 25, "Transpose"),
        (NANO_FX_POST1, 15, "Doubler"),
    ];

    FIRMWARE_ENUM
        .iter()
        .find(|(b, v, _)| *b == block && *v == val)
        .and_then(|(_, _, name)| find_model_by_name(name))
}

pub fn is_reverb_model(model: &NanoFxModelDef) -> bool {
    let name = model_name(model);
    name.contains("Hall")
        || name.contains("Room")
        || name.contains("Cave")
        || name.contains("Ambience")
        || name.contains("Modulated")
}

pub fn is_delay_model(model: &NanoFxModelDef) -> bool {
    model_name(model).contains("Delay")
}

pub fn is_modulation_model(model: &NanoFxModelDef) -> bool {
    let name = model_name(model);
    name.contains("Chorus")
        || name.contains("Phaser")
        || name.contains("Flanger")
        || name.contains("Tremolo")
        || name.contains("Vibe")
        || name.contains("CE2")
        || name.contains("DC2")
}

/* --------------------------------------------------------------- category */

fn category_from_catalog_index(idx: usize) -> u32 {
    if idx <= 10 {
        return if idx == 3 || idx == 7 {
            NANO_FX_CAT_BASS_OD
        } else {
            NANO_FX_CAT_GUITAR_OD
        };
    }
    if idx == 11 || idx == 15 || idx == 16 {
        return NANO_FX_CAT_UTILITY;
    }
    if (12..=14).contains(&idx) || (21..=22).contains(&idx) {
        return NANO_FX_CAT_FILTER;
    }
    if (17..=20).contains(&idx) {
        return NANO_FX_CAT_WAH;
    }
    if (23..=26).contains(&idx) || (29..=32).contains(&idx) {
        return NANO_FX_CAT_COMPRESSOR;
    }
    if (27..=28).contains(&idx) {
        return NANO_FX_CAT_PITCH;
    }
    if (33..=40).contains(&idx) {
        return NANO_FX_CAT_MODULATION;
    }
    if (41..=46).contains(&idx) {
        return NANO_FX_CAT_DELAY;
    }
    if (47..=52).contains(&idx) {
        return NANO_FX_CAT_REVERB;
    }
    NANO_FX_CAT_NONE
}

pub fn model_category(model: &NanoFxModelDef) -> u32 {
    if let Some(idx) = catalog_index(model) {
        return category_from_catalog_index(idx);
    }

    let name = model_name(model);
    if name.is_empty() {
        return NANO_FX_CAT_GUITAR_OD;
    }
    if name.contains("Wah") {
        return NANO_FX_CAT_WAH;
    }
    if name.contains("Comp") {
        return NANO_FX_CAT_COMPRESSOR;
    }
    if name.contains("Filter")
        || name.contains("EQ")
        || name.contains("Graphic")
        || name.contains("Parametric")
    {
        return NANO_FX_CAT_FILTER;
    }
    if name.contains("Transpose") || name.contains("Doubler") || name.contains("Pitch") {
        return NANO_FX_CAT_PITCH;
    }
    if name.contains("Bass") {
        return NANO_FX_CAT_BASS_OD;
    }
    if name.contains("Volume") || name.contains("Gate") {
        return NANO_FX_CAT_UTILITY;
    }
    if name.contains("Delay") {
        return NANO_FX_CAT_DELAY;
    }
    if name.contains("Reverb")
        || name.contains("Hall")
        || name.contains("Room")
        || name.contains("Cave")
        || name.contains("Ambience")
    {
        return NANO_FX_CAT_REVERB;
    }
    if name.contains("Chorus")
        || name.contains("Flanger")
        || name.contains("Phase")
        || name.contains("Tremolo")
        || name.contains("Vibe")
    {
        return NANO_FX_CAT_MODULATION;
    }

    NANO_FX_CAT_GUITAR_OD
}

/* ----------------------------------------------------------- model text */

fn hex_string(bytes: &[u8]) -> String {
    let mut out = String::new();
    // C formats into a 24-byte buffer, stopping while `pos + 3 < sizeof(hex)`.
    for b in bytes.iter().take(11) {
        out.push_str(&format!("{b:02X}"));
    }
    out
}

pub fn block_model_text(block: &NanoFxBlock) -> String {
    if block.has_model_id && block.model_id_len >= 2 {
        if let Some(from_id) = find_model_in_blob(block.model_id()) {
            let name = model_name(from_id);
            if !name.is_empty() {
                return String::from(name);
            }
        }
    }

    if let Some(model) = model_ref(block.model) {
        let name = model_name(model);
        if !name.is_empty() {
            return String::from(name);
        }
    }

    if block.has_model_id && block.model_id_len > 0 {
        return format!("ID {}", hex_string(block.model_id()));
    }

    String::from("Empty slot")
}

/* -------------------------------------------------------- chain acronym */

const CHAIN_ABBR: [(&str, &str); 53] = [
    ("Chief BD2", "BD2"),
    ("Chief OD1", "OD1"),
    ("Exotic", "EXOTIC"),
    ("Exotic Bass Z Boost", "BASS-Z"),
    ("Exotic Z Boost", "ZBOOST"),
    ("Facial Fuzz", "FUZZ"),
    ("Green 808", "GRN808"),
    ("Microtubes B3K", "B3K"),
    ("OD250", "OD250"),
    ("Obsessive Drive", "OBSESS"),
    ("Rodent Drive", "RODENT"),
    ("Adaptive Gate", "A.GATE"),
    ("Graphic 9", "GRAPH9"),
    ("Low-High Cut", "LO-HI"),
    ("Parametric 3", "PARAM3"),
    ("Utility Gate", "U.GATE"),
    ("Volume", "VOLUME"),
    ("Bass Wah", "BASSW"),
    ("Bubba Wah", "BUBBA"),
    ("Crying Clyde Wah", "CLYDE"),
    ("Crying Wah", "C.WAH"),
    ("Envelope Filter", "ENVELOP"),
    ("Love Meat", "LOVE"),
    ("Legendary 87 (M)", "LEG87M"),
    ("Opto Comp (M)", "OPTO-M"),
    ("Solid State Comp (M)", "SSD-M"),
    ("VCA Comp (M)", "VCA-M"),
    ("Transpose", "TRANS"),
    ("Doubler", "DOUBLE"),
    ("Legendary 87 (ST)", "LEG87"),
    ("Opto Comp (ST)", "OPTO"),
    ("Solid State Comp (ST)", "SSD"),
    ("VCA Comp (ST)", "VCA"),
    ("Chief CE2W (ST)", "CE2W"),
    ("Chief DC2W (ST)", "DC2W"),
    ("Chorus 229T", "CHR229"),
    ("Dream Chorus", "DREAM"),
    ("MX Flanger", "FLANG"),
    ("MX Phase 95", "PHASE95"),
    ("MX Vibe", "VIBE"),
    ("Tremolo", "TREMO"),
    ("Analog Delay", "ANALOG"),
    ("Circular Delay", "CIRC"),
    ("Digital Delay (ST)", "DIGDLY"),
    ("Dual Delay", "DUAL"),
    ("Dual Reverse Delay", "DREV"),
    ("Tape Delay", "TAPE"),
    ("Ambience", "AMBIEN"),
    ("Cave", "CAVE"),
    ("Hall", "HALL"),
    ("Mind Hall", "MINDH"),
    ("Modulated", "MODREV"),
    ("Room", "ROOM"),
];

const SKIP_CHAIN_WORDS: [&str; 12] = [
    "Chief",
    "MX",
    "Exotic",
    "Microtubes",
    "Solid",
    "State",
    "Legendary",
    "Digital",
    "Dual",
    "Utility",
    "Adaptive",
    "Crying",
];

fn is_skip_chain_word(word: &str) -> bool {
    SKIP_CHAIN_WORDS.contains(&word)
}

fn token_has_digit(token: &str) -> bool {
    token.bytes().any(|b| b.is_ascii_digit())
}

fn strip_model_parens(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '(' {
            for inner in chars.by_ref() {
                if inner == ')' {
                    break;
                }
            }
            continue;
        }
        out.push(ch);
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Split on spaces; leading dashes are eaten, inner ones are not. Tokens are
/// capped at 15 chars and 8 tokens, matching the C fixed-size buffers.
fn split_chain_tokens(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut tokens: Vec<String> = Vec::new();
    let mut p = 0;

    while p < bytes.len() && tokens.len() < 8 {
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'-') {
            p += 1;
        }
        if p >= bytes.len() {
            break;
        }
        let mut token = String::new();
        while p < bytes.len() && bytes[p] != b' ' && token.len() + 1 < 16 {
            token.push(bytes[p] as char);
            p += 1;
        }
        tokens.push(token);
    }

    tokens
}

fn truncate_to(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn chain_abbr_from_name(name: &str, cap: usize) -> Option<String> {
    if cap < 2 {
        return None;
    }

    if let Some((_, abbr)) = CHAIN_ABBR.iter().find(|(model, _)| *model == name) {
        return Some(truncate_to(abbr, cap - 1));
    }

    let clean = strip_model_parens(name);
    let tokens = split_chain_tokens(&clean);
    if tokens.is_empty() {
        return None;
    }

    let mut start = 0;
    while start < tokens.len() && is_skip_chain_word(&tokens[start]) {
        start += 1;
    }
    if start >= tokens.len() {
        start = 0;
    }

    if tokens.len() - start == 1 {
        let out = truncate_to(&tokens[start].to_ascii_uppercase(), (cap - 1).min(10));
        return (!out.is_empty()).then_some(out);
    }

    let digit_idx = (start..tokens.len())
        .rev()
        .find(|&i| token_has_digit(&tokens[i]));

    if let Some(idx) = digit_idx {
        if idx > start && !token_has_digit(&tokens[idx - 1]) {
            let mut out = String::new();
            out.push(tokens[idx - 1].chars().next().unwrap_or(' '));
            out.push_str(&truncate_to(&tokens[idx], cap - 2));
            return Some(out.to_ascii_uppercase());
        }
        let out = truncate_to(&tokens[idx], 10.min(cap - 1));
        return Some(out.to_ascii_uppercase());
    }

    if tokens.len() - start == 2 {
        let w1 = tokens[start].as_bytes();
        let w2 = tokens[start + 1].as_bytes();
        let mut out = String::new();
        out.push(w1.first().copied().unwrap_or(0) as char);
        out.push(w2.first().copied().unwrap_or(0) as char);
        out.push(w2.last().copied().unwrap_or(0) as char);
        return Some(truncate_to(&out.to_ascii_uppercase(), cap - 1));
    }

    let mut out = String::new();
    for token in tokens.iter().skip(start) {
        if out.len() + 1 >= cap || out.len() >= 8 {
            break;
        }
        if let Some(first) = token.chars().next() {
            out.push(first.to_ascii_uppercase());
        }
    }
    (!out.is_empty()).then_some(out)
}

/* ------------------------------------------------------- allowed models */

fn catalog_index_allowed(block: u32, idx: usize) -> bool {
    if idx >= catalog().len() {
        return false;
    }

    // EQ utilities: any pre/post slot.
    if (12..=14).contains(&idx) {
        return true;
    }

    if idx <= 10 {
        return block <= NANO_FX_PRE2;
    }
    if idx == 11 || idx == 15 || idx == 16 {
        return block <= NANO_FX_PRE2;
    }
    if (17..=22).contains(&idx) {
        return block <= NANO_FX_PRE2;
    }
    if (23..=26).contains(&idx) {
        return block <= NANO_FX_PRE2;
    }
    if idx == 27 {
        return block == NANO_FX_PRE1;
    }
    if idx == 28 {
        return block == NANO_FX_POST1;
    }
    if (29..=32).contains(&idx) {
        return block >= NANO_FX_POST1;
    }
    if (33..=40).contains(&idx) {
        return block == NANO_FX_POST1;
    }
    if (41..=46).contains(&idx) {
        return block == NANO_FX_POST2;
    }
    if (47..=52).contains(&idx) {
        return block == NANO_FX_POST3;
    }

    false
}

pub fn is_model_allowed(block: u32, model: &NanoFxModelDef) -> bool {
    if block as usize >= NANO_FX_BLOCK_COUNT || model.name.is_null() || model.id_len == 0 {
        return false;
    }
    catalog_index(model).is_some_and(|idx| catalog_index_allowed(block, idx))
}

/* ------------------------------------------------------------------ C ABI */

#[no_mangle]
pub extern "C" fn nano_fx_block_label(block: u32) -> *const c_char {
    BLOCK_LABELS_C
        .get(block as usize)
        .map_or(c"?".as_ptr(), |label| label.as_ptr())
}

/// Effect family for signal-chain icons (PRE/PFX slots).
#[no_mangle]
pub unsafe extern "C" fn nano_fx_model_category(model: *const NanoFxModelDef) -> u32 {
    match model_ref(model) {
        Some(model) => model_category(model),
        None => NANO_FX_CAT_NONE,
    }
}

#[no_mangle]
pub extern "C" fn nano_fx_category_label(cat: u32) -> *const c_char {
    match cat {
        NANO_FX_CAT_GUITAR_OD => c"Guitar OD".as_ptr(),
        NANO_FX_CAT_BASS_OD => c"Bass OD".as_ptr(),
        NANO_FX_CAT_COMPRESSOR => c"Compressor".as_ptr(),
        NANO_FX_CAT_PITCH => c"Pitch".as_ptr(),
        NANO_FX_CAT_FILTER => c"Filter".as_ptr(),
        NANO_FX_CAT_WAH => c"Wah".as_ptr(),
        NANO_FX_CAT_UTILITY => c"Utility".as_ptr(),
        NANO_FX_CAT_MODULATION => c"Modulation".as_ptr(),
        NANO_FX_CAT_DELAY => c"Delay".as_ptr(),
        NANO_FX_CAT_REVERB => c"Reverb".as_ptr(),
        _ => c"-".as_ptr(),
    }
}

/// Model name, or "ID xx yy" / "Empty" when unknown.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_block_model_text(
    block: *const NanoFxBlock,
    buf: *mut c_char,
    cap: usize,
) {
    if buf.is_null() || cap == 0 {
        return;
    }
    let text = match unsafe { block.as_ref() } {
        Some(block) => block_model_text(block),
        None => String::from("Empty slot"),
    };
    write_c_buf(buf, cap, &text);
}

/// Short label for signal-chain icon (e.g. "P95", "MHL").
#[no_mangle]
pub unsafe extern "C" fn nano_fx_model_chain_acronym(
    block: *const NanoFxBlock,
    buf: *mut c_char,
    cap: usize,
) {
    if buf.is_null() || cap == 0 {
        return;
    }

    let Some(block) = (unsafe { block.as_ref() }) else {
        write_c_buf(buf, cap, "-");
        return;
    };
    if !block.valid {
        write_c_buf(buf, cap, "-");
        return;
    }

    // C formats the model text into a 56-byte scratch buffer first.
    let name = truncate_to(&block_model_text(block), 55);
    if name == "Empty slot" || name.starts_with("ID ") {
        write_c_buf(buf, cap, "-");
        return;
    }

    match chain_abbr_from_name(&name, cap) {
        Some(abbr) => write_c_buf(buf, cap, &abbr),
        None => write_c_buf(buf, cap, "FX"),
    }
}

/// Whether catalog entry may be loaded in this FX slot (NanOS 2.0 rules).
#[no_mangle]
pub unsafe extern "C" fn nano_fx_is_model_allowed(
    block: u32,
    model: *const NanoFxModelDef,
) -> bool {
    match model_ref(model) {
        Some(model) => is_model_allowed(block, model),
        None => false,
    }
}

#[no_mangle]
pub extern "C" fn nano_fx_allowed_model_count(block: u32) -> usize {
    (0..catalog().len())
        .filter(|&idx| catalog_index_allowed(block, idx))
        .count()
}

/// Nth allowed catalog model for block (0 .. count-1).
#[no_mangle]
pub extern "C" fn nano_fx_get_allowed_model(block: u32, index: usize) -> *const NanoFxModelDef {
    let mut seen = 0;
    for (idx, entry) in catalog().iter().enumerate() {
        if !catalog_index_allowed(block, idx) {
            continue;
        }
        if seen == index {
            return entry as *const NanoFxModelDef;
        }
        seen += 1;
    }
    ptr::null()
}

/// Not in `nano_fx.h`, but the C port exported it — keep the symbol available.
#[no_mangle]
pub unsafe extern "C" fn nano_fx_find_model(id: *const u8, id_len: u8) -> *const NanoFxModelDef {
    if id.is_null() || id_len == 0 {
        return ptr::null();
    }
    let id = unsafe { core::slice::from_raw_parts(id, id_len as usize) };
    model_ptr(find_model(id))
}
