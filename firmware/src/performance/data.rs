use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const MAX_BANKS: usize = 8;
pub const SLOTS: usize = 4;
pub const SCENES: usize = 4;
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
    #[serde(default)]
    pub scenes: [[Option<Scene>; SCENES]; SLOTS],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub name: String,
    pub snapshot: Snapshot,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub model: Vec<u8>,
    pub enabled: bool,
    pub parameters: Vec<f32>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub amp: [u8; 5],
    pub capture_volume: Option<u8>,
    pub cab: Option<[f32; 3]>,
    pub gate_on: bool,
    pub gate_amount: Option<u8>,
    pub effects: [Effect; 5],
}

impl Effect {
    pub fn validate(&self, block: usize) -> Result<(), &'static str> {
        if self.model.is_empty() && !self.enabled && self.parameters.is_empty() {
            return Ok(());
        }
        let model = crate::fx::models::find_model(&self.model).ok_or("Unknown saved FX model")?;
        if !crate::fx::models::is_model_allowed(block as u32, model) {
            return Err("Saved FX model is incompatible with this block");
        }
        if self.parameters.len() != usize::from(model.param_count)
            || self
                .parameters
                .iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err("Saved FX parameters are incomplete or outside 0-1");
        }
        Ok(())
    }
}

impl Snapshot {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.gate_amount.is_some_and(|amount| amount > 100)
            || self.cab.is_some_and(|values| {
                values
                    .iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            })
        {
            return Err("Scene cab/gate values are outside their ranges");
        }
        if self.gate_on && self.gate_amount.is_none() {
            return Err("Enabled scene gate needs an amount");
        }
        for (block, effect) in self.effects.iter().enumerate() {
            effect.validate(block)?;
        }
        Ok(())
    }
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
            for (slot, scenes) in bank.scenes.iter().enumerate() {
                for (index, scene) in scenes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, scene)| scene.as_ref().map(|scene| (index, scene)))
                {
                    if bank.slots[slot].is_none() {
                        return Err("Scenes require an assigned bank slot");
                    }
                    let error = name_error(&scene.name);
                    if !error.is_empty() {
                        return Err(error);
                    }
                    if scenes[..index]
                        .iter()
                        .flatten()
                        .any(|old| old.name.eq_ignore_ascii_case(&scene.name))
                    {
                        return Err("Scene names must be unique within their bank slot");
                    }
                    scene.snapshot.validate()?;
                }
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
            scenes: Default::default(),
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
