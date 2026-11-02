use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use super::{continuous_action, ACTION_COUNT};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    format: String,
    version: u8,
    bindings: Vec<Binding>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    action: usize,
    kind: Kind,
    channel: u8,
    number: u8,
    #[serde(default)]
    zero_is_press: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Note,
    Cc,
    Program,
}

pub fn encode(
    keys: &[u16; ACTION_COUNT],
    zero_is_press: &[bool; ACTION_COUNT],
) -> Result<String, &'static str> {
    let bindings: Result<Vec<_>, _> = keys
        .iter()
        .enumerate()
        .filter(|(action, key)| **key != 0 || zero_is_press[*action])
        .map(|(action, &key)| {
            if key == 0 {
                return Err("Invalid stored MIDI press value");
            }
            if key & 0x8000 == 0 || key & 0x4080 != 0 {
                return Err("Invalid stored MIDI message");
            }
            let (kind, kind_code) = match (key >> 12) & 7 {
                0 => (Kind::Note, 0),
                1 => (Kind::Cc, 1),
                2 => (Kind::Program, 2),
                _ => return Err("Invalid stored MIDI message kind"),
            };
            if zero_is_press[action] && (kind_code != 1 || continuous_action(action)) {
                return Err("Invalid stored MIDI press value");
            }
            Ok(Binding {
                action,
                kind,
                channel: ((key >> 8) as u8 & 15) + 1,
                number: (key & 127) as u8,
                zero_is_press: zero_is_press[action],
            })
        })
        .collect();
    serde_json::to_string_pretty(&Backup {
        format: String::from("nano-midi"),
        version: 2,
        bindings: bindings?,
    })
    .map_err(|_| "Could not encode MIDI backup")
}

pub fn decode(json: &str) -> Result<([u16; ACTION_COUNT], [bool; ACTION_COUNT]), &'static str> {
    let backup: Backup = serde_json::from_str(json).map_err(|_| "Invalid MIDI backup JSON")?;
    if backup.format != "nano-midi" || !(1..=2).contains(&backup.version) {
        return Err("Unsupported MIDI backup format/version");
    }
    // The last two historical IDs recalled scenes 7-8; other IDs are unchanged.
    const PREVIOUS_ACTION_COUNT: usize = 273;
    if backup.bindings.len() > PREVIOUS_ACTION_COUNT {
        return Err("Too many MIDI backup bindings");
    }
    let mut keys = [0u16; ACTION_COUNT];
    let mut zero_is_press = [false; ACTION_COUNT];
    let mut seen = [false; PREVIOUS_ACTION_COUNT];
    for binding in backup.bindings {
        if binding.action >= PREVIOUS_ACTION_COUNT
            || !(1..=16).contains(&binding.channel)
            || binding.number > 127
        {
            return Err("MIDI backup binding out of range");
        }
        if seen[binding.action] {
            return Err("Duplicate MIDI backup action");
        }
        seen[binding.action] = true;
        let kind = match binding.kind {
            Kind::Note => 0,
            Kind::Cc => 1,
            Kind::Program => 2,
        };
        if continuous_action(binding.action) && kind != 1 {
            return Err("MIDI parameter binding requires CC");
        }
        if binding.zero_is_press
            && (backup.version == 1 || kind != 1 || continuous_action(binding.action))
        {
            return Err("Invalid MIDI press value");
        }
        if binding.action >= ACTION_COUNT {
            log::warn!(
                "Removed MIDI binding for retired scene action {}",
                binding.action
            );
            continue;
        }
        keys[binding.action] =
            0x8000 | kind << 12 | u16::from(binding.channel - 1) << 8 | u16::from(binding.number);
        zero_is_press[binding.action] = binding.zero_is_press;
    }
    Ok((keys, zero_is_press))
}
