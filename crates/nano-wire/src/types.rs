//! Shared constants and value types for the Nano Cortex wire protocol.

#[cfg(not(test))]
use alloc::{string::String, vec::Vec};

pub const BPM_MIN: u16 = 40;
pub const BPM_MAX: u16 = 240;

pub const AMP_CAPTURE_NAME_MAX: usize = 40;
pub const AMP_CAB_NAME_MAX: usize = 64;
pub const PRESET_COUNT: usize = 64;
pub const PRESET_NAME_MAX: usize = 48;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    Gain = 0,
    Level = 1,
    Bass = 2,
    Mid = 3,
    Treble = 4,
}

impl Knob {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Gain),
            1 => Some(Self::Level),
            2 => Some(Self::Bass),
            3 => Some(Self::Mid),
            4 => Some(Self::Treble),
            _ => None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunerRoute {
    Unknown = 0,
    Mute = 1,
    Bypass = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TunerSettings {
    pub reference_hz: Option<f32>,
    pub muted: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JackMode {
    #[default]
    Midi = 0,
    Expression = 1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeviceSettings {
    pub jack_mode: Option<JackMode>,
    pub usb_playback_db: Option<f32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CabSettings {
    pub output: Option<f32>,
    pub high_pass: Option<f32>,
    pub low_pass: Option<f32>,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeteringKind {
    CaptureInput = 0,
    CaptureOutput = 1,
    ExpressionPedal = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metering {
    pub kind: MeteringKind,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryCategory {
    Amp,
    AmpCab,
    Cab,
    Pedal,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryItem {
    /// Original zero-based index within its factory or user library.
    pub index: u32,
    pub name: String,
    pub user: bool,
    pub category: LibraryCategory,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryCatalog {
    pub captures: Vec<LibraryItem>,
    pub cabs: Vec<LibraryItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amp {
    pub gain: u8,
    pub level: u8,
    pub bass: u8,
    pub mid: u8,
    pub treble: u8,
    pub have_gain: bool,
    pub have_level: bool,
    pub have_bass: bool,
    pub have_mid: bool,
    pub have_treble: bool,
    pub capture_volume: u8,
    pub capture_slot: u8,
    pub capture_name: String,
    pub cab_slot: u8,
    pub cab_name: String,
    pub have_capture_slot: bool,
    pub have_capture_name: bool,
    pub have_capture_volume: bool,
    pub have_cab_slot: bool,
    pub have_cab_name: bool,
    pub valid: bool,
}

impl Default for Amp {
    fn default() -> Self {
        Self {
            gain: 0,
            level: 0,
            bass: 0,
            mid: 0,
            treble: 0,
            have_gain: false,
            have_level: false,
            have_bass: false,
            have_mid: false,
            have_treble: false,
            capture_volume: 128,
            capture_slot: 0,
            capture_name: String::new(),
            cab_slot: 0,
            cab_name: String::new(),
            have_capture_slot: false,
            have_capture_name: false,
            have_capture_volume: false,
            have_cab_slot: false,
            have_cab_name: false,
            valid: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresetCatalog {
    pub names: Vec<String>,
    pub valid: bool,
}

/// Fixed-layout amp snapshot used by the firmware UI / state machine.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct NanoAmpC {
    pub gain: u8,
    pub level: u8,
    pub bass: u8,
    pub mid: u8,
    pub treble: u8,
    pub capture_volume: u8,
    pub capture_slot: u8,
    pub capture_name: [u8; AMP_CAPTURE_NAME_MAX],
    pub cab_slot: u8,
    pub cab_name: [u8; AMP_CAB_NAME_MAX],
    pub have_capture_slot: bool,
    pub have_capture_name: bool,
    pub have_capture_volume: bool,
    pub have_cab_slot: bool,
    pub have_cab_name: bool,
    pub valid: bool,
}

/// Fixed-layout preset name table used by the firmware state machine.
#[repr(C)]
pub struct NanoPresetCatalogC {
    pub names: [[u8; PRESET_NAME_MAX]; PRESET_COUNT],
    pub count: u8,
    pub valid: bool,
}

fn write_c_string(dst: &mut [u8], src: &str) {
    dst.fill(0);
    let bytes = src.as_bytes();
    let n = bytes.len().min(dst.len().saturating_sub(1));
    dst[..n].copy_from_slice(&bytes[..n]);
}

impl From<&Amp> for NanoAmpC {
    fn from(amp: &Amp) -> Self {
        let mut out = Self {
            gain: amp.gain,
            level: amp.level,
            bass: amp.bass,
            mid: amp.mid,
            treble: amp.treble,
            capture_volume: amp.capture_volume,
            capture_slot: amp.capture_slot,
            capture_name: [0; AMP_CAPTURE_NAME_MAX],
            cab_slot: amp.cab_slot,
            cab_name: [0; AMP_CAB_NAME_MAX],
            have_capture_slot: amp.have_capture_slot,
            have_capture_name: amp.have_capture_name,
            have_capture_volume: amp.have_capture_volume,
            have_cab_slot: amp.have_cab_slot,
            have_cab_name: amp.have_cab_name,
            valid: amp.valid,
        };
        write_c_string(&mut out.capture_name, &amp.capture_name);
        write_c_string(&mut out.cab_name, &amp.cab_name);
        out
    }
}

impl NanoAmpC {
    /// Return the active 1-based cab slot, falling back to the first slot when
    /// the state includes a cab name but omits its slot.
    pub fn active_cab_slot(&self) -> Option<u8> {
        if self.have_cab_slot && (1..=5).contains(&self.cab_slot) {
            Some(self.cab_slot)
        } else if self.have_cab_name {
            Some(1)
        } else {
            None
        }
    }

    /// Convert to the heap `Amp` type (for logging / shared parsers).
    pub fn to_amp(&self) -> Amp {
        let end = self
            .capture_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.capture_name.len());
        let cab_end = self
            .cab_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.cab_name.len());
        Amp {
            gain: self.gain,
            level: self.level,
            bass: self.bass,
            mid: self.mid,
            treble: self.treble,
            have_gain: self.valid,
            have_level: self.valid,
            have_bass: self.valid,
            have_mid: self.valid,
            have_treble: self.valid,
            capture_volume: self.capture_volume,
            capture_slot: self.capture_slot,
            capture_name: String::from_utf8_lossy(&self.capture_name[..end]).into_owned(),
            cab_slot: self.cab_slot,
            cab_name: String::from_utf8_lossy(&self.cab_name[..cab_end]).into_owned(),
            have_capture_slot: self.have_capture_slot,
            have_capture_name: self.have_capture_name,
            have_capture_volume: self.have_capture_volume,
            have_cab_slot: self.have_cab_slot,
            have_cab_name: self.have_cab_name,
            valid: self.valid,
        }
    }
}

impl From<&PresetCatalog> for NanoPresetCatalogC {
    fn from(cat: &PresetCatalog) -> Self {
        let mut out = Self {
            names: [[0; PRESET_NAME_MAX]; PRESET_COUNT],
            count: 0,
            valid: false,
        };
        out.count = cat.names.len().min(PRESET_COUNT) as u8;
        out.valid = cat.valid;
        for (i, name) in cat.names.iter().take(PRESET_COUNT).enumerate() {
            write_c_string(&mut out.names[i], name);
        }
        out
    }
}
