use alloc::{string::String, vec::Vec};
use serde::{Deserialize, Serialize};

pub const SCENES: usize = 6;
pub const NAME_LIMIT: usize = 16;
// Each scenes journal slot is 1 MiB, including its 32-byte record header.
pub const MAX_JSON: usize = 1024 * 1024 - 32;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub format: String,
    pub version: u8,
    #[serde(default)]
    pub keep_master_eq: bool,
    #[serde(default)]
    pub preset_scenes: Vec<PresetScenes>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PresetScenes {
    pub preset: u8,
    pub scenes: [Option<Scene>; SCENES],
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredConfig {
    format: String,
    version: u8,
    #[serde(default)]
    keep_master_eq: bool,
    #[serde(default)]
    preset_scenes: Vec<StoredPresetScenes>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPresetScenes {
    preset: u8,
    scenes: Vec<Option<Scene>>,
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
    #[serde(default)]
    pub capture_slot: Option<u8>,
    #[serde(default)]
    pub cab_selection: Option<CabSelection>,
    pub capture_volume: Option<u8>,
    pub cab: Option<[f32; 3]>,
    pub gate_on: bool,
    pub gate_amount: Option<u8>,
    pub effects: [Effect; 5],
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CabSelection {
    pub slot: u8,
    pub name: String,
}

impl CabSelection {
    pub fn matches(&self, amp: &nano_wire::types::NanoAmpC) -> bool {
        let end = amp
            .cab_name
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(amp.cab_name.len());
        amp.valid
            && amp.active_cab_slot() == Some(self.slot)
            && amp.have_cab_name
            && &amp.cab_name[..end] == self.name.as_bytes()
    }

    pub fn library_item<'a>(
        &self,
        library: &'a nano_wire::types::LibraryCatalog,
    ) -> Result<&'a nano_wire::types::LibraryItem, &'static str> {
        let mut matches = library.cabs.iter().filter(|cab| cab.name == self.name);
        let cab = matches
            .next()
            .ok_or("Saved scene CAB/IR is not in the Nano library")?;
        if matches.next().is_some() {
            return Err("Saved scene CAB/IR name is ambiguous in the Nano library");
        }
        Ok(cab)
    }
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
        if self
            .capture_slot
            .is_some_and(|slot| !(1..=25).contains(&slot))
        {
            return Err("Scene capture slot must be within 1-25");
        }
        if self.cab_selection.as_ref().is_some_and(|cab| {
            !(1..=5).contains(&cab.slot)
                || cab.name.is_empty()
                || cab.name.len() >= nano_wire::types::AMP_CAB_NAME_MAX
                || cab.name.contains('\0')
        }) {
            return Err("Scene CAB/IR selection is invalid");
        }
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
            format: String::from("nano-scenes"),
            version: 4,
            keep_master_eq: false,
            preset_scenes: Vec::new(),
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

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.format != "nano-scenes" || self.version != 4 {
            return Err("Unsupported scene configuration format/version");
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
        Ok(())
    }

    pub fn decode(json: &str) -> Result<Self, &'static str> {
        Self::decode_with_migration(json).map(|(config, _)| config)
    }

    pub fn decode_with_migration(json: &str) -> Result<(Self, bool), &'static str> {
        if json.len() > MAX_JSON {
            return Err("scenes.json exceeds the 1 MiB journal slot");
        }
        let stored: StoredConfig = serde_json::from_str(json).map_err(|_| "Invalid scenes.json")?;
        if stored.format != "nano-scenes" || !matches!(stored.version, 3 | 4) {
            return Err("Unsupported scene configuration format/version");
        }
        let migrated = stored.version == 3;
        let mut preset_scenes = Vec::new();
        for mut collection in stored.preset_scenes {
            if collection.scenes.len() != if migrated { 8 } else { SCENES } {
                return Err("Invalid scene slot count");
            }
            collection.scenes.truncate(SCENES);
            preset_scenes.push(PresetScenes {
                preset: collection.preset,
                scenes: collection
                    .scenes
                    .try_into()
                    .map_err(|_| "Invalid scene slot count")?,
            });
        }
        let config = Self {
            format: stored.format,
            version: 4,
            keep_master_eq: stored.keep_master_eq,
            preset_scenes,
        };
        config.validate()?;
        Ok((config, migrated))
    }

    pub fn encode(&self) -> Result<String, &'static str> {
        self.validate()?;
        let json = serde_json::to_string(self).map_err(|_| "Cannot encode scenes.json")?;
        if json.len() > MAX_JSON {
            return Err("scenes.json exceeds the 1 MiB journal slot");
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
            capture_slot: Some(25),
            cab_selection: Some(CabSelection {
                slot: 5,
                name: "Saved IR".into(),
            }),
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
            r#"{{"format":"nano-scenes","version":2,"preset_scenes":[{{"preset":1,"scenes":[{},null,null,null]}}]}}"#,
            serde_json::to_string(&scene).unwrap(),
        );
        assert!(Config::decode(&old).is_err());
        assert!(Config::decode(&old.replace("\"version\":2", "\"version\":3")).is_err());
    }

    #[test]
    fn eight_slot_records_migrate_to_six_and_reencode_as_version_four() {
        let scenes: Vec<_> = (0..8)
            .map(|index| {
                Some(Scene {
                    name: alloc::format!("Scene {}", index + 1),
                    snapshot: snapshot(),
                })
            })
            .collect();
        let old = alloc::format!(
            r#"{{"format":"nano-scenes","version":3,"preset_scenes":[{{"preset":1,"scenes":{}}}]}}"#,
            serde_json::to_string(&scenes).unwrap(),
        );
        let (config, migrated) = Config::decode_with_migration(&old).unwrap();
        assert!(migrated);
        assert_eq!(config.version, 4);
        assert_eq!(config.preset_scenes[0].scenes.len(), 6);
        assert_eq!(
            config.preset_scenes[0].scenes[5].as_ref().unwrap().name,
            "Scene 6"
        );
        let canonical = config.encode().unwrap();
        assert!(!canonical.contains("Scene 7"));
        assert!(!canonical.contains("Scene 8"));
        assert!(!Config::decode_with_migration(&canonical).unwrap().1);
        assert!(Config::decode(&old.replace("\"version\":3", "\"version\":4")).is_err());
        assert!(Config::decode(&canonical.replace("\"version\":4", "\"version\":3")).is_err());
    }

    #[test]
    fn all_scenes_fit_storage_and_round_trip() {
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
        let json = config.encode().unwrap();
        assert!(json.len() <= MAX_JSON);
        let loaded = Config::decode(&json).unwrap();
        assert_eq!(loaded.preset_scenes.len(), 64);
        let saved = &loaded.preset_scenes[0].scenes[0].as_ref().unwrap().snapshot;
        assert_eq!(saved.capture_slot, Some(25));
        assert_eq!(saved.cab_selection.as_ref().unwrap().slot, 5);
        assert_eq!(saved.cab_selection.as_ref().unwrap().name, "Saved IR");
        config.preset_scenes.push(config.preset_scenes[0].clone());
        assert!(config.encode().is_err());
    }

    #[test]
    fn legacy_snapshots_without_selections_remain_loadable() {
        let mut json = serde_json::to_value(snapshot()).unwrap();
        let fields = json.as_object_mut().unwrap();
        fields.remove("capture_slot");
        fields.remove("cab_selection");
        let saved: Snapshot = serde_json::from_value(json).unwrap();
        saved.validate().unwrap();
        assert!(saved.capture_slot.is_none());
        assert!(saved.cab_selection.is_none());
    }

    #[test]
    fn invalid_selections_are_rejected() {
        let mut saved = snapshot();
        for slot in [0, 26, 255] {
            saved.capture_slot = Some(slot);
            assert!(saved.validate().is_err());
        }
        saved.capture_slot = Some(1);
        for slot in [0, 6, 255] {
            saved.cab_selection.as_mut().unwrap().slot = slot;
            assert!(saved.validate().is_err());
        }
        saved.cab_selection.as_mut().unwrap().slot = 1;
        saved.cab_selection.as_mut().unwrap().name.clear();
        assert!(saved.validate().is_err());
    }

    #[test]
    fn cab_lookup_uses_library_identity_not_list_position_and_refuses_ambiguity() {
        use nano_wire::types::{LibraryCatalog, LibraryCategory, LibraryItem};
        let selection = snapshot().cab_selection.unwrap();
        let mut library = LibraryCatalog::default();
        assert!(selection.library_item(&library).is_err());
        library.cabs.push(LibraryItem {
            index: 42,
            name: selection.name.clone(),
            user: true,
            category: LibraryCategory::Cab,
        });
        let cab = selection.library_item(&library).unwrap();
        assert_eq!(cab.index, 42);
        assert!(cab.user);
        library.cabs.insert(
            0,
            LibraryItem {
                index: 9,
                name: "Other IR".into(),
                user: false,
                category: LibraryCategory::Cab,
            },
        );
        assert_eq!(selection.library_item(&library).unwrap().index, 42);
        library.cabs.push(library.cabs[1].clone());
        assert!(selection.library_item(&library).is_err());
    }

    #[test]
    fn removed_model_presets_are_rejected() {
        assert!(
            Config::decode(r#"{"format":"nano-scenes","version":3,"model_presets":[]}"#).is_err()
        );
    }

    #[test]
    fn only_current_format_is_accepted() {
        assert!(Config::decode(&Config::default().encode().unwrap()).is_ok());
        for json in [
            r#"{"format":"other","version":1}"#,
            r#"{"format":"nano-scenes","version":1}"#,
            r#"{"format":"nano-scenes","version":2}"#,
            r#"{"format":"nano-scenes","version":2,"unknown":0}"#,
            r#"{"format":"nano-gig","version":3}"#,
            "broken",
        ] {
            assert!(Config::decode(json).is_err());
        }
    }
}
