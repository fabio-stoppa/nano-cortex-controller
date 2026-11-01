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
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Note,
    Cc,
    Program,
}

pub fn encode(keys: &[u16; ACTION_COUNT]) -> Result<String, &'static str> {
    let bindings: Result<Vec<_>, _> = keys
        .iter()
        .enumerate()
        .filter(|(_, key)| **key != 0)
        .map(|(action, &key)| {
            if key & 0x8000 == 0 || key & 0x4080 != 0 {
                return Err("Invalid stored MIDI message");
            }
            let kind = match (key >> 12) & 7 {
                0 => Kind::Note,
                1 => Kind::Cc,
                2 => Kind::Program,
                _ => return Err("Invalid stored MIDI message kind"),
            };
            Ok(Binding {
                action,
                kind,
                channel: ((key >> 8) as u8 & 15) + 1,
                number: (key & 127) as u8,
            })
        })
        .collect();
    serde_json::to_string_pretty(&Backup {
        format: String::from("nano-midi"),
        version: 1,
        bindings: bindings?,
    })
    .map_err(|_| "Could not encode MIDI backup")
}

pub fn decode(json: &str) -> Result<[u16; ACTION_COUNT], &'static str> {
    let backup: Backup = serde_json::from_str(json).map_err(|_| "Invalid MIDI backup JSON")?;
    if backup.format != "nano-midi" || backup.version != 1 {
        return Err("Unsupported MIDI backup format/version");
    }
    if backup.bindings.len() > ACTION_COUNT {
        return Err("Too many MIDI backup bindings");
    }
    let mut keys = [0u16; ACTION_COUNT];
    for binding in backup.bindings {
        if binding.action >= ACTION_COUNT
            || !(1..=16).contains(&binding.channel)
            || binding.number > 127
        {
            return Err("MIDI backup binding out of range");
        }
        if keys[binding.action] != 0 {
            return Err("Duplicate MIDI backup action");
        }
        let kind = match binding.kind {
            Kind::Note => 0,
            Kind::Cc => 1,
            Kind::Program => 2,
        };
        if continuous_action(binding.action) && kind != 1 {
            return Err("MIDI parameter binding requires CC");
        }
        keys[binding.action] =
            0x8000 | kind << 12 | u16::from(binding.channel - 1) << 8 | u16::from(binding.number);
    }
    Ok(keys)
}
