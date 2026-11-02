use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const SCENES: usize = 8;
pub const NAME_LIMIT: usize = 16;
pub const FX_PRESETS: usize = 32;
pub const FX_PRESETS_PER_MODEL: usize = 4;
// Each performance journal slot is 1 MiB, including its 32-byte record header.
pub const MAX_JSON: usize = 1024 * 1024 - 32;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format: String,
    pub version: u8,
    #[serde(default)]
    pub preset_scenes: Vec<PresetScenes>,
    #[serde(default)]
    pub model_presets: Vec<ModelPresets>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPresets {
    pub model: Vec<u8>,
    pub presets: [Option<FxPreset>; FX_PRESETS_PER_MODEL],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresetScenes {
    pub preset: u8,
    pub scenes: [Option<Scene>; SCENES],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FxPreset {
    pub name: String,
    pub effect: Effect,
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
            version: 3,
            preset_scenes: Vec::new(),
            model_presets: Vec::new(),
        }
    }
}

pub fn name_error(name: &str) -> &'static str {
    if name.is_empty() || name.len() > NAME_LIMIT {
        "Use 1-16 characters"
    } else {
        valid_name_characters(name)
    }
}

fn valid_name_characters(name: &str) -> &'static str {
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || b" -_".contains(&byte))
        || name.trim() != name
    {
        "Use letters, numbers, spaces, - or _; no outer spaces"
    } else {
        ""
    }
}

pub fn model_key(model: &[u8]) -> String {
    let mut key = String::new();
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in model {
        key.push(char::from(HEX[usize::from(byte >> 4)]));
        key.push(char::from(HEX[usize::from(byte & 15)]));
    }
    key
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format != "nano-performance" || self.version != 3 {
            return Err("Unsupported performance configuration format/version");
        }
        if self.preset_scenes.len() > 64 {
            return Err("At most 64 preset scene collections are supported");
        }
        for (index, collection) in self.preset_scenes.iter().enumerate() {
            if !(1..=64).contains(&collection.preset)
                || self.preset_scenes[..index]
                    .iter()
                    .any(|old| old.preset == collection.preset)
            {
                return Err("Scene preset numbers must be unique and within 1-64");
            }
            for (index, scene) in collection.scenes.iter().enumerate() {
                if let Some(scene) = scene {
                    let error = name_error(&scene.name);
                    if !error.is_empty() {
                        return Err(error);
                    }
                    if collection.scenes[..index]
                        .iter()
                        .flatten()
                        .any(|old| old.name.eq_ignore_ascii_case(&scene.name))
                    {
                        return Err("Scene names must be unique within their Nano preset");
                    }
                    scene.snapshot.validate()?;
                }
            }
        }
        for (index, collection) in self.model_presets.iter().enumerate() {
            let model = crate::fx::models::find_model(&collection.model)
                .ok_or("Unknown FX preset collection model")?;
            if self.model_presets[..index]
                .iter()
                .any(|old| old.model == collection.model)
            {
                return Err("FX preset collections must have unique models");
            }
            let block = (0..5)
                .find(|block| crate::fx::models::is_model_allowed(*block, model))
                .ok_or("FX preset model has no compatible block")?;
            for (index, preset) in collection.presets.iter().enumerate() {
                if let Some(preset) = preset {
                    let error = name_error(&preset.name);
                    if !error.is_empty() {
                        return Err(error);
                    }
                    if preset.effect.model != collection.model {
                        return Err("FX preset belongs to another model");
                    }
                    preset.effect.validate(block as usize)?;
                    if collection.presets[..index]
                        .iter()
                        .flatten()
                        .any(|old| old.name.eq_ignore_ascii_case(&preset.name))
                    {
                        return Err("FX preset names must be unique within their model");
                    }
                }
            }
        }
        Ok(())
    }

    pub fn decode(json: &str) -> Result<Self, &'static str> {
        if json.len() > MAX_JSON {
            return Err("performance.json exceeds the 1 MiB journal slot");
        }
        let config: Self = serde_json::from_str(json).map_err(|_| "Invalid performance.json")?;
        config.validate()?;
        Ok(config)
    }

    pub fn encode(&self) -> Result<String, &'static str> {
        self.validate()?;
        let json = serde_json::to_string(self).map_err(|_| "Cannot encode performance.json")?;
        if json.len() > MAX_JSON {
            return Err("performance.json exceeds the 1 MiB journal slot");
        }
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        Snapshot {
            amp: [255; 5],
            capture_volume: Some(255),
            cab: Some([0.12345679; 3]),
            gate_on: true,
            gate_amount: Some(100),
            effects: core::array::from_fn(|block| {
                let model = crate::fx::catalog::catalog()
                    .iter()
                    .filter(|model| crate::fx::models::is_model_allowed(block as u32, model))
                    .max_by_key(|model| model.param_count)
                    .unwrap();
                Effect {
                    model: model.id_bytes().to_vec(),
                    enabled: true,
                    parameters: alloc::vec![0.12345679; usize::from(model.param_count)],
                }
            }),
        }
    }

    #[test]
    fn names_accept_sixteen_ascii_characters_and_reject_seventeen() {
        assert_eq!(name_error("1234567890123456"), "");
        assert_eq!(name_error("12345678901234567"), "Use 1-16 characters");
        assert!(!name_error("").is_empty());
        assert!(!name_error(" leading").is_empty());
        assert!(!name_error("trailing ").is_empty());
        assert!(!name_error("Caf\u{e9}").is_empty());
    }

    #[test]
    fn obsolete_four_scene_records_are_rejected() {
        let scene = Scene {
            name: "Old".into(),
            snapshot: snapshot(),
        };
        let old = alloc::format!(
            r#"{{"format":"nano-performance","version":2,"preset_scenes":[{{"preset":1,"scenes":[{},null,null,null]}}],"model_presets":[]}}"#,
            serde_json::to_string(&scene).unwrap(),
        );
        assert!(Config::decode(&old).is_err());
        assert!(Config::decode(&old.replace("\"version\":2", "\"version\":3")).is_err());
    }

    #[test]
    fn all_scenes_and_all_model_presets_fit_storage_and_round_trip() {
        let mut config = Config::default();
        let snapshot = snapshot();
        for preset in 1..=64 {
            config.preset_scenes.push(PresetScenes {
                preset,
                scenes: core::array::from_fn(|index| {
                    Some(Scene {
                        name: alloc::format!("Scene name {:02}", index),
                        snapshot: snapshot.clone(),
                    })
                }),
            });
        }
        for model in crate::fx::catalog::catalog() {
            config.model_presets.push(ModelPresets {
                model: model.id_bytes().to_vec(),
                presets: core::array::from_fn(|index| {
                    Some(FxPreset {
                        name: alloc::format!("Effect name {:02}", index),
                        effect: Effect {
                            model: model.id_bytes().to_vec(),
                            enabled: true,
                            parameters: alloc::vec![0.12345679; usize::from(model.param_count)],
                        },
                    })
                }),
            });
        }
        let json = config.encode().unwrap();
        assert!(json.len() <= MAX_JSON);
        let loaded = Config::decode(&json).unwrap();
        assert_eq!(loaded.preset_scenes.len(), 64);
        assert_eq!(
            loaded.model_presets.len(),
            crate::fx::catalog::catalog().len()
        );
        config.preset_scenes.push(config.preset_scenes[0].clone());
        assert!(config.encode().is_err());
    }

    #[test]
    fn model_collections_reject_unknown_duplicate_and_mismatched_models() {
        let effects = snapshot().effects;
        let mut config = Config::default();
        config.model_presets.push(ModelPresets {
            model: effects[0].model.clone(),
            presets: core::array::from_fn(|index| {
                (index == 0).then(|| FxPreset {
                    name: "Saved".into(),
                    effect: effects[0].clone(),
                })
            }),
        });
        assert!(config.encode().is_ok());
        config.model_presets.push(config.model_presets[0].clone());
        assert!(config.encode().is_err());
        config.model_presets.pop();
        config.model_presets[0].presets[0]
            .as_mut()
            .unwrap()
            .effect
            .model = Vec::new();
        assert!(config.encode().is_err());
        config.model_presets[0].model = alloc::vec![255; 8];
        assert!(config.encode().is_err());
    }

    #[test]
    fn only_current_format_is_accepted() {
        assert!(Config::decode(&Config::default().encode().unwrap()).is_ok());
        for json in [
            r#"{"format":"other","version":1}"#,
            r#"{"format":"nano-performance","version":1}"#,
            r#"{"format":"nano-performance","version":2}"#,
            "broken",
        ] {
            assert!(Config::decode(json).is_err());
        }
        assert!(
            Config::decode(r#"{"format":"nano-performance","version":2,"unknown":0}"#).is_err()
        );
    }
}
