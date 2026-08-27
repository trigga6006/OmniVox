use crate::error::AppResult;
use crate::hotkey::HotkeyConfig;
use crate::storage::database::Database;
use crate::storage::types::{AppSettings, SettingsPatch};
use rusqlite::params;
use serde::Serialize;
use std::collections::HashMap;

const AUDIO_DEVICE_SETTING: &str = "audio_device_id";

pub fn get_audio_device_id(db: &Database) -> AppResult<Option<String>> {
    use rusqlite::OptionalExtension;
    let value = db
        .conn()?
        .query_row(
            "SELECT value FROM settings WHERE key=?1",
            [AUDIO_DEVICE_SETTING],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value.filter(|value| !value.trim().is_empty()))
}

pub fn set_audio_device_id(db: &Database, device_id: &str) -> AppResult<()> {
    db.conn()?.execute(
        "INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![AUDIO_DEVICE_SETTING, device_id],
    )?;
    Ok(())
}

/// In-memory authority for settings used on the recording hot path.
///
/// SQLite remains the durable store, but capture must never need to scan the
/// complete settings table.  Updates are field-level and monotonically
/// versioned so isolated WebViews can rebase a stale patch without clobbering
/// fields they did not edit.
#[derive(Debug, Clone, Default)]
pub struct RuntimeSettings {
    revision: u64,
    values: AppSettings,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsSnapshot {
    pub revision: u64,
    pub settings: AppSettings,
    /// True when a patch was based on an older snapshot and was safely rebased
    /// onto the latest backend value.
    pub rebased: bool,
}

impl RuntimeSettings {
    pub fn load(db: &Database) -> AppResult<Self> {
        Ok(Self {
            revision: 0,
            values: get_settings(db)?,
        })
    }

    pub fn values(&self) -> &AppSettings {
        &self.values
    }

    pub fn snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            revision: self.revision,
            settings: self.values.clone(),
            rebased: false,
        }
    }

    /// Applies a partial update in memory.  The caller must persist `values()`;
    /// if persistence fails it should restore the previous RuntimeSettings
    /// while holding the same application lock.
    pub fn apply_patch(
        &mut self,
        patch: SettingsPatch,
        expected_revision: Option<u64>,
    ) -> (SettingsSnapshot, bool) {
        let rebased = expected_revision.is_some_and(|revision| revision != self.revision);
        let changed = patch.apply_to(&mut self.values);
        if changed {
            self.revision = self.revision.saturating_add(1);
        }
        (
            SettingsSnapshot {
                revision: self.revision,
                settings: self.values.clone(),
                rebased,
            },
            changed,
        )
    }

    /// Compatibility path for older IPC clients that still submit a complete
    /// object. New clients must use `patch_settings` so stale WebViews cannot
    /// overwrite unrelated fields.
    pub fn replace_legacy(&mut self, settings: AppSettings) -> (SettingsSnapshot, bool) {
        let changed = !same_settings(&self.values, &settings);
        if changed {
            self.values = settings;
            self.revision = self.revision.saturating_add(1);
        }
        (self.snapshot(), changed)
    }
}

// AppSettings contains a nested HotkeyConfig but intentionally does not derive
// PartialEq as it is also an IPC schema. Compare its stable JSON form only on
// the legacy path (which is cold and compatibility-only).
fn same_settings(left: &AppSettings, right: &AppSettings) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

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

    let theme = map.get("theme").cloned().unwrap_or(defaults.theme);
    let auto_start = map
        .get("auto_start")
        .map(|v| v == "true")
        .unwrap_or(defaults.auto_start);
    let output_mode = map
        .get("output_mode")
        .cloned()
        .unwrap_or(defaults.output_mode);
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

    let active_context_mode_id = map.get("active_context_mode_id").and_then(|v| {
        if v.is_empty() {
            None
        } else {
            Some(v.clone())
        }
    });

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

    let command_mode = map
        .get("command_mode")
        .map(|v| v == "true")
        .unwrap_or(defaults.command_mode);

    let launch_app_voice_commands_enabled = map
        .get("launch_app_voice_commands_enabled")
        .map(|v| v == "true")
        .unwrap_or(defaults.launch_app_voice_commands_enabled);

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

    let filler_removal = map
        .get("filler_removal")
        .map(|v| v == "true")
        .unwrap_or(defaults.filler_removal);

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

    let cleanup_mode = map
        .get("cleanup_mode")
        .map(|v| v == "true")
        .unwrap_or(defaults.cleanup_mode);

    let active_cleanup_model_id = map
        .get("active_cleanup_model_id")
        .and_then(|v| if v.is_empty() { None } else { Some(v.clone()) })
        .or(defaults.active_cleanup_model_id);

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

    let history_enabled = map
        .get("history_enabled")
        .map(|v| v == "true")
        .unwrap_or(defaults.history_enabled);

    let history_retention_days = map
        .get("history_retention_days")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|days| *days <= crate::storage::types::MAX_HISTORY_RETENTION_DAYS)
        .unwrap_or(defaults.history_retention_days);

    Ok(AppSettings {
        theme,
        auto_start,
        output_mode,
        active_model_id,
        hotkey,
        gpu_acceleration,
        active_context_mode_id,
        live_preview,
        noise_reduction,
        auto_switch_modes,
        voice_commands,
        command_send,
        command_mode,
        launch_app_voice_commands_enabled,
        ship_mode,
        ghost_mode,
        writing_style,
        filler_removal,
        audio_ducking,
        ducking_amount,
        structured_mode,
        active_llm_model_id,
        cleanup_mode,
        active_cleanup_model_id,
        llm_timeout_secs,
        structured_min_chars,
        structured_voice_command,
        use_screen_context,
        structured_use_screen_context,
        history_enabled,
        history_retention_days,
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
    settings
        .validate()
        .map_err(crate::error::AppError::Storage)?;
    // Pre-compute every owned String so the pairs slice below holds &str refs
    // that live for the duration of the loop.  `params!` borrows its inputs.
    const BOOL_TRUE: &str = "true";
    const BOOL_FALSE: &str = "false";
    fn b(v: bool) -> &'static str {
        if v {
            BOOL_TRUE
        } else {
            BOOL_FALSE
        }
    }

    let hotkey_json = serde_json::to_string(&settings.hotkey).unwrap_or_default();
    let ducking_amount_str = settings.ducking_amount.to_string();
    let llm_timeout_str = settings.llm_timeout_secs.to_string();
    let structured_min_chars_str = settings.structured_min_chars.to_string();
    let history_retention_days_str = settings.history_retention_days.to_string();

    let pairs: [(&str, &str); 30] = [
        ("theme", settings.theme.as_str()),
        ("auto_start", b(settings.auto_start)),
        ("output_mode", settings.output_mode.as_str()),
        (
            "active_model_id",
            settings.active_model_id.as_deref().unwrap_or(""),
        ),
        ("hotkey", hotkey_json.as_str()),
        ("gpu_acceleration", b(settings.gpu_acceleration)),
        ("live_preview", b(settings.live_preview)),
        ("noise_reduction", b(settings.noise_reduction)),
        ("auto_switch_modes", b(settings.auto_switch_modes)),
        ("voice_commands", b(settings.voice_commands)),
        ("command_send", b(settings.command_send)),
        ("command_mode", b(settings.command_mode)),
        (
            "launch_app_voice_commands_enabled",
            b(settings.launch_app_voice_commands_enabled),
        ),
        ("ship_mode", b(settings.ship_mode)),
        ("ghost_mode", b(settings.ghost_mode)),
        ("writing_style", settings.writing_style.as_str()),
        ("filler_removal", b(settings.filler_removal)),
        ("audio_ducking", b(settings.audio_ducking)),
        ("ducking_amount", ducking_amount_str.as_str()),
        ("structured_mode", b(settings.structured_mode)),
        (
            "active_llm_model_id",
            settings.active_llm_model_id.as_deref().unwrap_or(""),
        ),
        ("cleanup_mode", b(settings.cleanup_mode)),
        (
            "active_cleanup_model_id",
            settings.active_cleanup_model_id.as_deref().unwrap_or(""),
        ),
        ("llm_timeout_secs", llm_timeout_str.as_str()),
        ("structured_min_chars", structured_min_chars_str.as_str()),
        (
            "structured_voice_command",
            b(settings.structured_voice_command),
        ),
        ("use_screen_context", b(settings.use_screen_context)),
        (
            "structured_use_screen_context",
            b(settings.structured_use_screen_context),
        ),
        ("history_enabled", b(settings.history_enabled)),
        (
            "history_retention_days",
            history_retention_days_str.as_str(),
        ),
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

#[cfg(test)]
mod tests {
    use super::{
        get_audio_device_id, get_settings, set_audio_device_id, update_settings, RuntimeSettings,
    };
    use crate::storage::database::Database;
    use crate::storage::types::{SettingsPatch, MAX_HISTORY_RETENTION_DAYS};

    fn test_db() -> (Database, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("omnivox-settings-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::init(&dir.join("omnivox.db")).unwrap();
        (db, dir)
    }

    #[test]
    fn stale_field_patch_rebases_without_clobbering_other_fields() {
        let mut runtime = RuntimeSettings::default();
        let (first, changed) = runtime.apply_patch(
            SettingsPatch {
                command_mode: Some(true),
                ..SettingsPatch::default()
            },
            Some(0),
        );
        assert!(changed);
        assert_eq!(first.revision, 1);

        let (second, changed) = runtime.apply_patch(
            SettingsPatch {
                theme: Some("light".into()),
                ..SettingsPatch::default()
            },
            Some(0),
        );
        assert!(changed);
        assert!(second.rebased);
        assert_eq!(second.revision, 2);
        assert!(second.settings.command_mode);
        assert_eq!(second.settings.theme, "light");
    }

    #[test]
    fn no_op_patch_does_not_bump_revision() {
        let mut runtime = RuntimeSettings::default();
        let (snapshot, changed) = runtime.apply_patch(
            SettingsPatch {
                command_mode: Some(false),
                ..SettingsPatch::default()
            },
            Some(0),
        );
        assert!(!changed);
        assert_eq!(snapshot.revision, 0);
    }

    #[test]
    fn model_id_patch_without_revision_is_accepted_and_normalized() {
        let mut runtime = RuntimeSettings::default();
        let (snapshot, changed) = runtime.apply_patch(
            SettingsPatch {
                active_llm_model_id: Some("qwen3-1.7b-instruct-q8".into()),
                ..SettingsPatch::default()
            },
            None,
        );
        assert!(changed);
        assert!(!snapshot.rebased);
        assert_eq!(snapshot.revision, 1);
        assert_eq!(
            snapshot.settings.active_llm_model_id.as_deref(),
            Some("qwen3-1.7b-instruct-q8")
        );

        let (cleared, changed) = runtime.apply_patch(
            SettingsPatch {
                active_llm_model_id: Some(String::new()),
                ..SettingsPatch::default()
            },
            None,
        );
        assert!(changed);
        assert_eq!(cleared.settings.active_llm_model_id, None);
    }

    #[test]
    fn history_policy_defaults_and_round_trips() {
        let (db, dir) = test_db();
        let defaults = get_settings(&db).unwrap();
        assert!(defaults.history_enabled);
        assert_eq!(defaults.history_retention_days, 30);

        let mut changed = defaults;
        changed.history_enabled = false;
        changed.history_retention_days = 30;
        update_settings(&db, &changed).unwrap();

        let loaded = get_settings(&db).unwrap();
        assert!(!loaded.history_enabled);
        assert_eq!(loaded.history_retention_days, 30);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn selected_audio_device_round_trips_without_rewriting_app_settings() {
        let (db, dir) = test_db();
        assert!(get_audio_device_id(&db).unwrap().is_none());
        set_audio_device_id(&db, "wasapi-device-123").unwrap();
        assert_eq!(
            get_audio_device_id(&db).unwrap().as_deref(),
            Some("wasapi-device-123")
        );
        assert_eq!(get_settings(&db).unwrap().theme, "dark");
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn invalid_history_retention_patch_is_rejected() {
        let patch = SettingsPatch {
            history_retention_days: Some(MAX_HISTORY_RETENTION_DAYS + 1),
            ..SettingsPatch::default()
        };
        assert!(patch.validate().is_err());
    }

    #[test]
    fn existing_database_without_retention_setting_keeps_legacy_unlimited_policy() {
        let dir = std::env::temp_dir().join(format!(
            "omnivox-settings-upgrade-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("omnivox.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
                 INSERT INTO settings (key, value) VALUES ('theme', 'light');",
            )
            .unwrap();
        }

        let db = Database::init(&path).unwrap();
        let loaded = get_settings(&db).unwrap();
        assert_eq!(loaded.history_retention_days, 0);
        assert_eq!(loaded.theme, "light");
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn existing_database_preserves_explicit_retention_setting() {
        let dir = std::env::temp_dir().join(format!(
            "omnivox-settings-retention-preserve-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("omnivox.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
                 INSERT INTO settings (key, value)
                 VALUES ('history_retention_days', '90');",
            )
            .unwrap();
        }

        let db = Database::init(&path).unwrap();
        let loaded = get_settings(&db).unwrap();
        assert_eq!(loaded.history_retention_days, 90);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
