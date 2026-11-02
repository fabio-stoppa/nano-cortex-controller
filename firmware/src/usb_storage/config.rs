pub fn import(midi: &str, scenes: &str, preferences: &str) -> Result<(), &'static str> {
    crate::midi::keys_from_json(midi)?;
    crate::scenes::data::Config::decode(scenes)?;
    let preferences = crate::settings::preferences::Preferences::decode(preferences)?;
    crate::scenes::import_json(scenes)?;
    crate::midi::import_json(midi).map_err(|error| {
        crate::midi::report(error);
        "Scenes saved, but MIDI save failed; previous MIDI retained. Retry after restarting."
    })?;
    crate::settings::import_preferences(&preferences).map_err(|error| {
        log::warn!("USB preferences save: {error}");
        "MIDI and scenes saved, but preferences save failed. Check preferences after restarting."
    })
}
