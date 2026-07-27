//! Shared constants and value types (mirrors `nano_protocol.h` / `nano_proto.h`).

pub const BPM_MIN: u16 = 40;
pub const BPM_MAX: u16 = 240;

pub const AMP_CAPTURE_NAME_MAX: usize = 40;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Amp {
    pub gain: u8,
    pub level: u8,
    pub bass: u8,
    pub mid: u8,
    pub treble: u8,
    pub capture_volume: u8,
    pub capture_slot: u8,
    pub capture_name: String,
    pub have_capture_slot: bool,
    pub have_capture_name: bool,
    pub have_capture_volume: bool,
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
            capture_volume: 128,
            capture_slot: 0,
            capture_name: String::new(),
            have_capture_slot: false,
            have_capture_name: false,
            have_capture_volume: false,
            valid: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PresetCatalog {
    pub names: Vec<String>,
    pub valid: bool,
}
