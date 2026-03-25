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
    pub llm_prompt: String,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Writing style for this mode ("formal", "casual", "very_casual").
    pub writing_style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictationStats {
    pub total_words: u64,
    pub total_transcriptions: u64,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionRecord {
    pub id: Uuid,
    pub text: String,
    pub duration_ms: u64,
    pub model_name: String,
    pub created_at: DateTime<Utc>,
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
    pub language: String,
    pub auto_start: bool,
    pub minimize_to_tray: bool,
    pub output_mode: String,
    pub sample_rate: u32,
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
    /// Automatically press Enter after transcription to send the message (TypeSimulation/Both only).
    pub ship_mode: bool,
    /// Hide the floating pill overlay (invisible but still interactive).
    pub ghost_mode: bool,
    /// Writing style controls capitalization and punctuation ("formal", "casual", "very_casual").
    pub writing_style: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            language: "en".to_string(),
            auto_start: false,
            minimize_to_tray: true,
            output_mode: "clipboard".to_string(),
            sample_rate: 16000,
            active_model_id: None,
            hotkey: Some(crate::hotkey::HotkeyConfig::default()),
            gpu_acceleration: false,
            active_context_mode_id: None,
            live_preview: false,
            noise_reduction: false,
            auto_switch_modes: true,
            voice_commands: true,
            command_send: true,
            ship_mode: false,
            ghost_mode: false,
            writing_style: "formal".to_string(),
        }
    }
}
