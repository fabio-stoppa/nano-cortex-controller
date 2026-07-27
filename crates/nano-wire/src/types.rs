//! Shared constants and value types for the Nano Cortex wire protocol.

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
    pub have_capture_slot: bool,
    pub have_capture_name: bool,
    pub have_capture_volume: bool,
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
            have_capture_slot: amp.have_capture_slot,
            have_capture_name: amp.have_capture_name,
            have_capture_volume: amp.have_capture_volume,
            valid: amp.valid,
        };
        write_c_string(&mut out.capture_name, &amp.capture_name);
        out
    }
}

impl NanoAmpC {
    /// Convert to the heap `Amp` type (for logging / shared parsers).
    pub fn to_amp(&self) -> Amp {
        let end = self
            .capture_name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.capture_name.len());
        Amp {
            gain: self.gain,
            level: self.level,
            bass: self.bass,
            mid: self.mid,
            treble: self.treble,
            capture_volume: self.capture_volume,
            capture_slot: self.capture_slot,
            capture_name: String::from_utf8_lossy(&self.capture_name[..end]).into_owned(),
            have_capture_slot: self.have_capture_slot,
            have_capture_name: self.have_capture_name,
            have_capture_volume: self.have_capture_volume,
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
