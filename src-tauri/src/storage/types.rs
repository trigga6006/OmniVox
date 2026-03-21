use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    pub id: Uuid,
    pub trigger: String,
    pub content: String,
    pub description: Option<String>,
    pub is_enabled: bool,
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
        }
    }
}
