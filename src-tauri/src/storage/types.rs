use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMode {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub color: String,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Writing style for this mode ("formal", "casual", "very_casual").
    pub writing_style: String,
    /// Structured Mode profile id for this mode ("agent-prompt", "email",
    /// "notes-outline").  Empty or unknown resolves to the default
    /// agent-prompt profile (`llm::profiles::get`).
    pub structured_profile: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationStats {
    pub total_words: u64,
    pub total_transcriptions: u64,
    pub total_duration_ms: u64,
}

/// A single transcription distilled to the fields the analytics page needs.
///
/// Word/character counts are computed in Rust so the (potentially large) full
/// `text` of every record never has to cross the IPC boundary — the frontend
/// only receives these lean rows and derives sessions, streaks, heatmaps, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsRecord {
    pub created_at: DateTime<Utc>,
    pub word_count: u64,
    pub char_count: u64,
    pub duration_ms: u64,
    pub model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionRecord {
    pub id: Uuid,
    pub text: String,
    pub duration_ms: u64,
    pub model_name: String,
    pub created_at: DateTime<Utc>,
    /// Original dictation before Structured Mode post-processing.
    ///
    /// `None` for pre-migration rows and for plain dictations (where `text`
    /// and the raw transcript are the same).  The "View raw" disclosure in
    /// the Structured panel surfaces this so the user can always recover the
    /// words they actually spoke.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_transcript: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: Uuid,
    pub phrase: String,
    pub replacement: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: Uuid,
    pub trigger: String,
    pub content: String,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VocabularyEntry {
    pub id: Uuid,
    pub word: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub title: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppBinding {
    pub id: Uuid,
    pub mode_id: String,
    pub process_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    /// Launch OmniVox automatically when the OS starts (Windows registry
    /// Run key via tauri-plugin-autostart).  Applied in
    /// `commands::settings::update_settings` and reconciled at startup.
    pub auto_start: bool,
    pub output_mode: String,
    pub active_model_id: Option<String>,
    pub hotkey: Option<crate::hotkey::HotkeyConfig>,
    /// Enable GPU acceleration for Whisper inference (requires Vulkan).
    pub gpu_acceleration: bool,
    /// Active context mode ID — determines which prompt/dictionary/snippets are used.
    pub active_context_mode_id: Option<String>,
    /// Show live transcription preview in the floating pill while recording.
    pub live_preview: bool,
    /// Pre-process audio with RNNoise to remove background noise before Whisper.
    pub noise_reduction: bool,
    /// Automatically switch context mode based on the foreground application.
    pub auto_switch_modes: bool,
    /// Recognize spoken voice commands ("new line", "new paragraph", "delete last word").
    pub voice_commands: bool,
    /// Enable the "send" voice command independently — say "send" at the end to press Enter.
    pub command_send: bool,
    /// Command Mode: a dedicated push-to-talk hotkey whose entire utterance is a
    /// command (launch app, key chord, media key, window action) instead of
    /// dictated text.  Disabled by default.
    pub command_mode: bool,
    /// Gate for voice commands that launch an application (Command Mode
    /// "open <app>" and the inline dictation `LaunchApp`).  When false, the
    /// backend refuses to start a process from a voice command.  Default ON —
    /// the identity-bound command path makes launches safe, and the owner
    /// prefers features full-on.
    pub launch_app_voice_commands_enabled: bool,
    /// Automatically press Enter after transcription to send the message (TypeSimulation/Both only).
    pub ship_mode: bool,
    /// Hide the floating pill overlay (invisible but still interactive).
    pub ghost_mode: bool,
    /// Writing style controls capitalization and punctuation ("formal", "casual", "very_casual").
    pub writing_style: String,
    /// Remove filler words (um, uh, "you know", stray "basically") and
    /// deduplicate stutter repeats during post-processing.  Off = transcribe
    /// verbatim.
    pub filler_removal: bool,
    /// Lower system volume while recording to reduce background noise pickup.
    pub audio_ducking: bool,
    /// How much to reduce volume (0 = no reduction, 100 = full mute). Default 70.
    pub ducking_amount: u32,
    /// Structured Mode: run dictation through a local LLM and output a slot-
    /// filled Markdown prompt instead of plain prose.
    pub structured_mode: bool,
    /// ID of the LLM catalog entry to use for slot extraction.
    pub active_llm_model_id: Option<String>,
    /// Cleanup Mode: run the raw transcript through a local text-normalizer
    /// LLM (fillers, self-corrections, punctuation, spoken numbers/dates) before
    /// anything downstream sees it.  Independent of Structured Mode — when both
    /// are on, the cleaned text is what gets structured.
    pub cleanup_mode: bool,
    /// ID of the LLM catalog entry (purpose `cleanup`) used for that stage.
    /// Separate from `active_llm_model_id`: the two stages run different models.
    pub active_cleanup_model_id: Option<String>,
    /// Hard timeout (seconds) for a single LLM extraction.  On timeout, the
    /// pipeline falls back to plain-text output and emits `structured-mode-degraded`.
    /// Also bounds one cleanup pass.
    pub llm_timeout_secs: u32,
    /// Transcripts shorter than this skip Structured Mode entirely — too
    /// little content to structure meaningfully.
    pub structured_min_chars: u32,
    /// When true, Structured Mode requires the user to end their dictation
    /// with the trigger word "Voxify" before the LLM runs.  If the word is
    /// absent the transcription is output plain, even with `structured_mode`
    /// on.  Mirrors how `command_send` gates Ship Mode behind the "send"
    /// voice command — lets the user opt-in per utterance instead of
    /// structuring every single one.
    pub structured_voice_command: bool,
    /// Read visible text from the foreground window (Windows UIA) and feed
    /// the technical tokens it contains into Whisper as `initial_prompt`,
    /// so file paths, identifiers, and CLI flags transcribe verbatim.  Local
    /// only — captured text never leaves the device.
    pub use_screen_context: bool,
    /// When true, Structured Mode also receives the screen-context tokens so
    /// Qwen can swap phonetic guesses for verbatim matches.  Independent of
    /// `use_screen_context` (Phase 1 alone covers most cases; this adds the
    /// reconciliation layer for multi-token strings).
    pub structured_use_screen_context: bool,
    /// Persist completed dictations in the local history database. Disabling
    /// this is fail-closed on the save path and securely purges existing rows.
    pub history_enabled: bool,
    /// Number of days to retain transcript history. Zero means keep forever;
    /// nonzero values are capped to ten years to reject malformed IPC input.
    pub history_retention_days: u32,
}

pub const MAX_HISTORY_RETENTION_DAYS: u32 = 3_650;
pub const DEFAULT_NEW_INSTALL_HISTORY_RETENTION_DAYS: u32 = 30;

impl AppSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.history_retention_days > MAX_HISTORY_RETENTION_DAYS {
            return Err(format!(
                "History retention cannot exceed {MAX_HISTORY_RETENTION_DAYS} days"
            ));
        }
        Ok(())
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            auto_start: false,
            output_mode: "clipboard".to_string(),
            active_model_id: None,
            hotkey: Some(crate::hotkey::HotkeyConfig::default()),
            // Default ON when the binary was compiled with a GPU backend, so a
            // GPU build uses the GPU out of the box instead of running Whisper +
            // the LLM entirely on CPU until the user finds the Settings toggle.
            // CPU-only builds default OFF. (Only the DEFAULT changes — a user who
            // has explicitly saved gpu_acceleration keeps their choice.)
            gpu_acceleration: cfg!(any(feature = "vulkan", feature = "cuda")),
            active_context_mode_id: None,
            live_preview: false,
            noise_reduction: false,
            auto_switch_modes: true,
            voice_commands: true,
            command_send: true,
            command_mode: false,
            launch_app_voice_commands_enabled: true,
            ship_mode: false,
            ghost_mode: false,
            writing_style: "formal".to_string(),
            filler_removal: true,
            audio_ducking: true,
            ducking_amount: 70,
            structured_mode: false,
            active_llm_model_id: None,
            cleanup_mode: false,
            active_cleanup_model_id: None,
            llm_timeout_secs: 8,
            structured_min_chars: 40,
            structured_voice_command: false,
            // Screen contents can include sensitive text. New installs must
            // opt in explicitly; persisted settings retain their saved value.
            use_screen_context: false,
            structured_use_screen_context: false,
            // Database initialization explicitly seeds existing installations
            // to their legacy unlimited policy before this default is read.
            history_enabled: true,
            history_retention_days: DEFAULT_NEW_INSTALL_HISTORY_RETENTION_DAYS,
        }
    }
}

/// A typed, field-level settings update.  WebViews must send this instead of
/// re-sending an entire (possibly stale) `AppSettings` object.
///
/// `Option` means "leave unchanged".  The two optional model ids deliberately
/// use an empty string to clear a selection; the UI never clears either today,
/// and that representation keeps absent JSON fields distinct from `null`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsPatch {
    pub theme: Option<String>,
    pub auto_start: Option<bool>,
    pub output_mode: Option<String>,
    pub active_model_id: Option<String>,
    pub hotkey: Option<crate::hotkey::HotkeyConfig>,
    pub gpu_acceleration: Option<bool>,
    pub active_context_mode_id: Option<String>,
    pub live_preview: Option<bool>,
    pub noise_reduction: Option<bool>,
    pub auto_switch_modes: Option<bool>,
    pub voice_commands: Option<bool>,
    pub command_send: Option<bool>,
    pub command_mode: Option<bool>,
    pub launch_app_voice_commands_enabled: Option<bool>,
    pub ship_mode: Option<bool>,
    pub ghost_mode: Option<bool>,
    pub writing_style: Option<String>,
    pub filler_removal: Option<bool>,
    pub audio_ducking: Option<bool>,
    pub ducking_amount: Option<u32>,
    pub structured_mode: Option<bool>,
    pub active_llm_model_id: Option<String>,
    pub cleanup_mode: Option<bool>,
    pub active_cleanup_model_id: Option<String>,
    pub llm_timeout_secs: Option<u32>,
    pub structured_min_chars: Option<u32>,
    pub structured_voice_command: Option<bool>,
    pub use_screen_context: Option<bool>,
    pub structured_use_screen_context: Option<bool>,
    pub history_enabled: Option<bool>,
    pub history_retention_days: Option<u32>,
}

impl SettingsPatch {
    pub fn validate(&self) -> Result<(), String> {
        if self
            .history_retention_days
            .is_some_and(|days| days > MAX_HISTORY_RETENTION_DAYS)
        {
            return Err(format!(
                "History retention cannot exceed {MAX_HISTORY_RETENTION_DAYS} days"
            ));
        }
        Ok(())
    }

    /// The overlay pill intentionally exposes a small set of live toggles.
    /// Keep every destructive, persistence-wide, model, hotkey, and OS setting
    /// main-window-only even though both windows share the patch command.
    ///
    /// This exhaustive destructure is deliberate: adding a new settings field
    /// must fail compilation here until its overlay policy is classified.
    pub fn validate_overlay_scope(&self) -> Result<(), String> {
        let Self {
            theme,
            auto_start,
            output_mode,
            active_model_id,
            hotkey,
            gpu_acceleration,
            active_context_mode_id,
            live_preview: _,
            noise_reduction: _,
            auto_switch_modes: _,
            voice_commands,
            command_send: _,
            command_mode,
            launch_app_voice_commands_enabled,
            ship_mode: _,
            ghost_mode: _,
            writing_style,
            filler_removal,
            audio_ducking,
            ducking_amount,
            structured_mode: _,
            active_llm_model_id,
            cleanup_mode,
            active_cleanup_model_id,
            llm_timeout_secs,
            structured_min_chars,
            structured_voice_command: _,
            use_screen_context,
            structured_use_screen_context,
            history_enabled,
            history_retention_days,
        } = self;
        let contains_main_only_field = theme.is_some()
            || auto_start.is_some()
            || output_mode.is_some()
            || active_model_id.is_some()
            || hotkey.is_some()
            || gpu_acceleration.is_some()
            || active_context_mode_id.is_some()
            || voice_commands.is_some()
            || command_mode.is_some()
            || launch_app_voice_commands_enabled.is_some()
            || writing_style.is_some()
            || filler_removal.is_some()
            || audio_ducking.is_some()
            || ducking_amount.is_some()
            || active_llm_model_id.is_some()
            || cleanup_mode.is_some()
            || active_cleanup_model_id.is_some()
            || llm_timeout_secs.is_some()
            || structured_min_chars.is_some()
            || use_screen_context.is_some()
            || structured_use_screen_context.is_some()
            || history_enabled.is_some()
            || history_retention_days.is_some();
        if contains_main_only_field {
            Err("The overlay may only change its documented live toggles".into())
        } else {
            Ok(())
        }
    }

    /// Apply the supplied fields and return whether anything actually changed.
    /// This makes side effects idempotent: a duplicate event cannot restart a
    /// hotkey hook, reload a model, or touch OS autostart.
    pub fn apply_to(self, settings: &mut AppSettings) -> bool {
        let mut changed = false;
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    if settings.$field != value {
                        settings.$field = value;
                        changed = true;
                    }
                }
            };
        }

        apply!(theme);
        apply!(auto_start);
        apply!(output_mode);
        if let Some(value) = self.active_model_id {
            let value = (!value.is_empty()).then_some(value);
            if settings.active_model_id != value {
                settings.active_model_id = value;
                changed = true;
            }
        }
        if let Some(value) = self.hotkey {
            let value = Some(value);
            if settings.hotkey != value {
                settings.hotkey = value;
                changed = true;
            }
        }
        apply!(gpu_acceleration);
        if let Some(value) = self.active_context_mode_id {
            let value = (!value.is_empty()).then_some(value);
            if settings.active_context_mode_id != value {
                settings.active_context_mode_id = value;
                changed = true;
            }
        }
        apply!(live_preview);
        apply!(noise_reduction);
        apply!(auto_switch_modes);
        apply!(voice_commands);
        apply!(command_send);
        apply!(command_mode);
        apply!(launch_app_voice_commands_enabled);
        apply!(ship_mode);
        apply!(ghost_mode);
        apply!(writing_style);
        apply!(filler_removal);
        apply!(audio_ducking);
        apply!(ducking_amount);
        apply!(structured_mode);
        if let Some(value) = self.active_llm_model_id {
            let value = (!value.is_empty()).then_some(value);
            if settings.active_llm_model_id != value {
                settings.active_llm_model_id = value;
                changed = true;
            }
        }
        apply!(cleanup_mode);
        if let Some(value) = self.active_cleanup_model_id {
            let value = (!value.is_empty()).then_some(value);
            if settings.active_cleanup_model_id != value {
                settings.active_cleanup_model_id = value;
                changed = true;
            }
        }
        apply!(llm_timeout_secs);
        apply!(structured_min_chars);
        apply!(structured_voice_command);
        apply!(use_screen_context);
        apply!(structured_use_screen_context);
        apply!(history_enabled);
        apply!(history_retention_days);
        changed
    }
}

#[cfg(test)]
mod settings_patch_scope_tests {
    use super::SettingsPatch;

    #[test]
    fn overlay_live_toggle_set_is_allowed() {
        let patch = SettingsPatch {
            live_preview: Some(true),
            noise_reduction: Some(true),
            auto_switch_modes: Some(false),
            command_send: Some(false),
            ship_mode: Some(true),
            ghost_mode: Some(true),
            structured_mode: Some(true),
            structured_voice_command: Some(true),
            ..SettingsPatch::default()
        };
        assert!(patch.validate_overlay_scope().is_ok());
    }

    #[test]
    fn overlay_cannot_mutate_destructive_or_main_only_settings() {
        let denied = [
            SettingsPatch {
                history_enabled: Some(false),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                active_llm_model_id: Some("model-id".into()),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                hotkey: Some(crate::hotkey::HotkeyConfig::default()),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                auto_start: Some(true),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                cleanup_mode: Some(true),
                ..SettingsPatch::default()
            },
            SettingsPatch {
                active_cleanup_model_id: Some("s1-mini-0.6b-q4".into()),
                ..SettingsPatch::default()
            },
        ];
        for patch in denied {
            assert!(patch.validate_overlay_scope().is_err());
        }
    }
}

/// A user-editable voice command row (built-in or custom).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomVoiceCommand {
    pub id: Uuid,
    /// The spoken trigger, stored lowercased.
    pub phrase: String,
    /// Encoded action: a built-in variant name ("NewLine") or a KeyCombo spec
    /// ("key:ctrl+shift+k").
    pub action: String,
    /// "anywhere" | "end_of_utterance".
    pub trigger_scope: String,
    pub enabled: bool,
    pub built_in: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
}
