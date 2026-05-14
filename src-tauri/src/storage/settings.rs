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
    let gpu_acceleration = map
        .get("gpu_acceleration")
        .map(|v| v == "true")
        .unwrap_or(defaults.gpu_acceleration);

    let active_context_mode_id = map
        .get("active_context_mode_id")
        .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) });

    let live_preview = map
        .get("live_preview")
        .map(|v| v == "true")
        .unwrap_or(defaults.live_preview);

    let noise_reduction = map
        .get("noise_reduction")
        .map(|v| v == "true")
        .unwrap_or(defaults.noise_reduction);

    let auto_switch_modes = map
        .get("auto_switch_modes")
        .map(|v| v == "true")
        .unwrap_or(defaults.auto_switch_modes);

    let voice_commands = map
        .get("voice_commands")
        .map(|v| v == "true")
        .unwrap_or(defaults.voice_commands);

    let command_send = map
        .get("command_send")
        .map(|v| v == "true")
        .unwrap_or(defaults.command_send);

    let ship_mode = map
        .get("ship_mode")
        .map(|v| v == "true")
        .unwrap_or(defaults.ship_mode);

    let ghost_mode = map
        .get("ghost_mode")
        .map(|v| v == "true")
        .unwrap_or(defaults.ghost_mode);

    let writing_style = map
        .get("writing_style")
        .cloned()
        .unwrap_or(defaults.writing_style);

    let audio_ducking = map
        .get("audio_ducking")
        .map(|v| v == "true")
        .unwrap_or(defaults.audio_ducking);

    let ducking_amount = map
        .get("ducking_amount")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(defaults.ducking_amount);

    let structured_mode = map
        .get("structured_mode")
        .map(|v| v == "true")
        .unwrap_or(defaults.structured_mode);

    let active_llm_model_id = map
        .get("active_llm_model_id")
        .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
        .or(defaults.active_llm_model_id);

    let llm_timeout_secs = map
        .get("llm_timeout_secs")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(defaults.llm_timeout_secs);

    let structured_min_chars = map
        .get("structured_min_chars")
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(defaults.structured_min_chars);

    let structured_voice_command = map
        .get("structured_voice_command")
        .map(|v| v == "true")
        .unwrap_or(defaults.structured_voice_command);

    let use_screen_context = map
        .get("use_screen_context")
        .map(|v| v == "true")
        .unwrap_or(defaults.use_screen_context);

    let structured_use_screen_context = map
        .get("structured_use_screen_context")
        .map(|v| v == "true")
        .unwrap_or(defaults.structured_use_screen_context);

    Ok(AppSettings {
        theme,
        language,
        auto_start,
        minimize_to_tray,
        output_mode,
        sample_rate,
        active_model_id,
        hotkey,
        gpu_acceleration,
        active_context_mode_id,
        live_preview,
        noise_reduction,
        auto_switch_modes,
        voice_commands,
        command_send,
        ship_mode,
        ghost_mode,
        writing_style,
        audio_ducking,
        ducking_amount,
        structured_mode,
        active_llm_model_id,
        llm_timeout_secs,
        structured_min_chars,
        structured_voice_command,
        use_screen_context,
        structured_use_screen_context,
    })
}

/// Set a single setting key-value pair.
pub fn set_setting(db: &Database, key: &str, value: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
        params![key, value],
    )?;
    Ok(())
}

/// Persist updated application settings to the database atomically.
///
/// Uses a single prepared statement reused for all key/value pairs inside one
/// transaction — ~19× faster than calling `tx.execute()` with a fresh SQL
/// string per key (which forces SQLite to re-parse and re-plan each call).
pub fn update_settings(db: &Database, settings: &AppSettings) -> AppResult<()> {
    // Pre-compute every owned String so the pairs slice below holds &str refs
    // that live for the duration of the loop.  `params!` borrows its inputs.
    const BOOL_TRUE: &str = "true";
    const BOOL_FALSE: &str = "false";
    fn b(v: bool) -> &'static str { if v { BOOL_TRUE } else { BOOL_FALSE } }

    let hotkey_json = serde_json::to_string(&settings.hotkey).unwrap_or_default();
    let sample_rate_str = settings.sample_rate.to_string();
    let ducking_amount_str = settings.ducking_amount.to_string();
    let llm_timeout_str = settings.llm_timeout_secs.to_string();
    let structured_min_chars_str = settings.structured_min_chars.to_string();

    let pairs: [(&str, &str); 26] = [
        ("theme", settings.theme.as_str()),
        ("language", settings.language.as_str()),
        ("auto_start", b(settings.auto_start)),
        ("minimize_to_tray", b(settings.minimize_to_tray)),
        ("output_mode", settings.output_mode.as_str()),
        ("sample_rate", sample_rate_str.as_str()),
        ("active_model_id", settings.active_model_id.as_deref().unwrap_or("")),
        ("hotkey", hotkey_json.as_str()),
        ("gpu_acceleration", b(settings.gpu_acceleration)),
        ("live_preview", b(settings.live_preview)),
        ("noise_reduction", b(settings.noise_reduction)),
        ("auto_switch_modes", b(settings.auto_switch_modes)),
        ("voice_commands", b(settings.voice_commands)),
        ("command_send", b(settings.command_send)),
        ("ship_mode", b(settings.ship_mode)),
        ("ghost_mode", b(settings.ghost_mode)),
        ("writing_style", settings.writing_style.as_str()),
        ("audio_ducking", b(settings.audio_ducking)),
        ("ducking_amount", ducking_amount_str.as_str()),
        ("structured_mode", b(settings.structured_mode)),
        ("active_llm_model_id", settings.active_llm_model_id.as_deref().unwrap_or("")),
        ("llm_timeout_secs", llm_timeout_str.as_str()),
        ("structured_min_chars", structured_min_chars_str.as_str()),
        ("structured_voice_command", b(settings.structured_voice_command)),
        ("use_screen_context", b(settings.use_screen_context)),
        ("structured_use_screen_context", b(settings.structured_use_screen_context)),
    ];

    let conn = db.conn()?;
    let tx = conn.unchecked_transaction()?;
    {
        // `prepare` once, execute 19× — SQLite parses the SQL string once.
        // `prepare_cached` would additionally persist across calls, but since
        // `update_settings` is called infrequently (only on user change) the
        // per-call prepare is cheap and avoids cache eviction surprises.
        let mut stmt =
            tx.prepare("INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)")?;
        for (k, v) in pairs.iter() {
            stmt.execute(params![k, v])?;
        }
    }
    tx.commit()?;
    Ok(())
}
