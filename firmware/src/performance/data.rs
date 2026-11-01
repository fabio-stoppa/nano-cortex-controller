use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const MAX_BANKS: usize = 8;
pub const SLOTS: usize = 4;
pub const MAX_JSON: usize = 512 * 1024;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format: String,
    pub version: u8,
    pub banks: Vec<Bank>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bank {
    pub name: String,
    pub slots: [Option<u8>; SLOTS],
}

impl Default for Config {
    fn default() -> Self {
        Self {
            format: String::from("nano-performance"),
            version: 1,
            banks: Vec::new(),
        }
    }
}

pub fn name_error(name: &str) -> &'static str {
    if name.is_empty() || name.len() > 32 {
        "Use 1-32 characters"
    } else if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b" -_".contains(&byte))
        || name.trim() != name
    {
        "Use letters, numbers, spaces, - or _; no outer spaces"
    } else {
        ""
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format != "nano-performance" || self.version != 1 {
            return Err("Unsupported performance configuration format/version");
        }
        if self.banks.len() > MAX_BANKS {
            return Err("At most 8 banks are supported");
        }
        for (index, bank) in self.banks.iter().enumerate() {
            let error = name_error(&bank.name);
            if !error.is_empty() {
                return Err(error);
            }
            if self.banks[..index]
                .iter()
                .any(|old| old.name.eq_ignore_ascii_case(&bank.name))
            {
                return Err("Bank names must be unique");
            }
            if bank
                .slots
                .iter()
                .flatten()
                .any(|preset| !(1..=64).contains(preset))
            {
                return Err("Bank presets must be 1-64 or null");
            }
        }
        Ok(())
    }

    pub fn decode(json: &str) -> Result<Self, &'static str> {
        if json.len() > MAX_JSON {
            return Err("performance.json exceeds 512 KiB");
        }
        let config: Self = serde_json::from_str(json).map_err(|_| "Invalid performance.json")?;
        config.validate()?;
        Ok(config)
    }

    pub fn encode(&self) -> Result<String, &'static str> {
        self.validate()?;
        let json =
            serde_json::to_string_pretty(self).map_err(|_| "Cannot encode performance.json")?;
        if json.len() > MAX_JSON {
            return Err("performance.json exceeds 512 KiB");
        }
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bank_json_round_trip_and_validation() {
        let mut config = Config::default();
        config.banks.push(Bank {
            name: String::from("Set 1"),
            slots: [Some(1), Some(64), None, Some(7)],
        });
        let loaded = Config::decode(&config.encode().unwrap()).unwrap();
        assert_eq!(loaded.banks[0].slots, config.banks[0].slots);
        config.banks[0].slots[0] = Some(0);
        assert!(config.encode().is_err());
        config.banks[0].slots[0] = Some(1);
        config.banks.push(config.banks[0].clone());
        assert!(config.encode().is_err());
        assert!(Config::decode(r#"{"format":"nano-performance","version":2,"banks":[]}"#).is_err());
        assert!(Config::decode(
            r#"{"format":"nano-performance","version":1,"banks":[],"unknown":0}"#
        )
        .is_err());
    }
}
