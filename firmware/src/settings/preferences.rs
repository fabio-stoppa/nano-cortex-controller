use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const MAX_JSON: usize = 4096;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Landscape,
    Portrait,
    PortraitFlipped,
    LandscapeFlipped,
}

impl Orientation {
    pub fn from_layout(mode: i32) -> Result<Self, &'static str> {
        match mode {
            0 => Ok(Self::Landscape),
            1 => Ok(Self::Portrait),
            2 => Ok(Self::PortraitFlipped),
            3 => Ok(Self::LandscapeFlipped),
            _ => Err("Invalid controller screen orientation"),
        }
    }

    pub fn layout(self) -> u8 {
        match self {
            Self::Landscape => 0,
            Self::Portrait => 1,
            Self::PortraitFlipped => 2,
            Self::LandscapeFlipped => 3,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    format: String,
    version: u8,
    pub orientation: Orientation,
    pub autosave: bool,
}

impl Preferences {
    pub fn new(mode: i32, autosave: bool) -> Result<Self, &'static str> {
        Ok(Self {
            format: String::from("nano-controller-preferences"),
            version: 1,
            orientation: Orientation::from_layout(mode)?,
            autosave,
        })
    }

    pub fn decode(json: &str) -> Result<Self, &'static str> {
        if json.len() > MAX_JSON {
            return Err("preferences.json exceeds 4 KiB");
        }
        let preferences: Self =
            serde_json::from_str(json).map_err(|_| "Invalid preferences.json")?;
        if preferences.format != "nano-controller-preferences" || preferences.version != 1 {
            return Err("Unsupported controller preferences format/version");
        }
        Ok(preferences)
    }

    pub fn encode(&self) -> Result<String, &'static str> {
        serde_json::to_string_pretty(self).map_err(|_| "Cannot encode preferences.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_orientation_and_autosave_value_round_trips() {
        for mode in 0..4 {
            for autosave in [false, true] {
                let preferences = Preferences::new(mode, autosave).unwrap();
                let json = preferences.encode().unwrap();
                assert!(json.len() < MAX_JSON);
                let decoded = Preferences::decode(&json).unwrap();
                assert_eq!(decoded, preferences);
                assert_eq!(i32::from(decoded.orientation.layout()), mode);
                assert_eq!(decoded.autosave, autosave);
            }
        }
        assert!(Preferences::new(-1, false).is_err());
        assert!(Preferences::new(4, false).is_err());
    }

    #[test]
    fn strict_schema_rejects_bad_types_missing_fields_and_unknown_settings() {
        let json = Preferences::new(0, false).unwrap().encode().unwrap();
        for invalid in [
            String::from("{"),
            String::from("{}"),
            json.replace("\"autosave\": false", "\"autosave\": \"false\""),
            json.replace("\"orientation\": \"landscape\",", ""),
            json.replace("\"autosave\": false", "\"unrecognized\": false"),
            json.replace("\"landscape\"", "\"sideways\""),
            json.replace("\"landscape\"", "4"),
            json.replace("\"version\": 1", "\"version\": 2"),
            json.replace("nano-controller-preferences", "nano-preferences"),
            json.replace(
                "\"autosave\": false",
                "\"autosave\": false, \"autosave\": true",
            ),
            " ".repeat(MAX_JSON + 1),
        ] {
            assert!(Preferences::decode(&invalid).is_err(), "{invalid}");
        }
    }
}
