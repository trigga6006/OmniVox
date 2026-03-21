use crate::error::AppResult;
use crate::hotkey::HotkeyConfig;
use crate::storage::database::Database;
use crate::storage::types::AppSettings;
use rusqlite::params;
use std::collections::HashMap;

/// Retrieve the current application settings from the database.
/// Falls back to default values for any missing keys.
pub fn get_settings(db: &Database) -> AppResult<AppSettings> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        let key: String = row.get(0)?;
        let value: String = row.get(1)?;
        Ok((key, value))
    })?;

    let mut map = HashMap::new();
    for row in rows {
        let (key, value) = row?;
        map.insert(key, value);
    }

    let defaults = AppSettings::default();

    let theme = map
        .get("theme")
        .cloned()
        .unwrap_or(defaults.theme);
    let language = map
        .get("language")
        .cloned()
        .unwrap_or(defaults.language);
    let auto_start = map
        .get("auto_start")
        .map(|v| v == "true")
        .unwrap_or(defaults.auto_start);
    let minimize_to_tray = map
        .get("minimize_to_tray")
        .map(|v| v == "true")
        .unwrap_or(defaults.minimize_to_tray);
    let output_mode = map
        .get("output_mode")
        .cloned()
        .unwrap_or(defaults.output_mode);
    let sample_rate = map
        .get("sample_rate")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(defaults.sample_rate);
    let active_model_id = map
        .get("active_model_id")
        .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
        .or(defaults.active_model_id);
    let hotkey = map
        .get("hotkey")
        .and_then(|v| serde_json::from_str::<HotkeyConfig>(v).ok())
        .or(defaults.hotkey);

    Ok(AppSettings {
        theme,
        language,
        auto_start,
        minimize_to_tray,
        output_mode,
        sample_rate,
        active_model_id,
        hotkey,
    })
}

/// Persist updated application settings to the database atomically.
pub fn update_settings(db: &Database, settings: &AppSettings) -> AppResult<()> {
    let conn = db.conn()?;
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["theme", &settings.theme],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["language", &settings.language],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["auto_start", settings.auto_start.to_string()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["minimize_to_tray", settings.minimize_to_tray.to_string()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["output_mode", &settings.output_mode],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params!["sample_rate", settings.sample_rate.to_string()],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![
            "active_model_id",
            settings.active_model_id.as_deref().unwrap_or("")
        ],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![
            "hotkey",
            serde_json::to_string(&settings.hotkey).unwrap_or_default()
        ],
    )?;

    tx.commit()?;
    Ok(())
}
