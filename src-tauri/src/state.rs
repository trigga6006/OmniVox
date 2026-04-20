use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::audio::capture::AudioCapture;
use crate::audio::types::AudioConfig;
use crate::llm::runner::LlmRunner;
use crate::llm_models::downloader::LlmModelDownloader;
use crate::llm_models::manager::LlmModelManager;
use crate::models::downloader::ModelDownloader;
use crate::models::manager::ModelManager;
use crate::output::router::OutputRouter;
use crate::output::types::OutputConfig;
use crate::postprocess::processor::ProcessorChain;
use crate::postprocess::types::{ProcessorConfig, WritingStyle};
use crate::storage::database::Database;

/// Central application state, managed by Tauri.
///
/// All mutable fields are behind `Mutex` for thread-safe access from
/// async command handlers and the global shortcut callback.
pub struct AppState {
    /// Microphone capture engine
    pub audio: Mutex<AudioCapture>,
    /// Whisper engine. None until a model is loaded.
    /// Wrapped in Arc so transcription can run on a blocking thread without
    /// holding the Mutex for the duration of CPU-bound inference.
    pub engine: Mutex<Option<Arc<crate::asr::engine::WhisperEngine>>>,
    /// Text post-processing chain (capitalization, dictionary, etc.)
    pub processor: Mutex<ProcessorChain>,
    /// Output router (clipboard / keystroke simulation)
    pub output: OutputRouter,
    /// Output mode configuration
    pub output_config: Mutex<OutputConfig>,
    /// Model catalog + download state
    pub model_manager: ModelManager,
    /// Streaming model downloader
    pub downloader: ModelDownloader,
    /// ID of the currently active model
    pub active_model_id: Mutex<Option<String>>,
    /// Local SQLite database for persistent storage
    pub db: Database,
    /// Application data directory (~/.local/share/omnivox or AppData/omnivox)
    pub data_dir: PathBuf,
    /// Directory where downloaded model files are stored
    pub models_dir: PathBuf,
    /// HWND of the window that was focused before recording started.
    /// Used to restore focus before pasting transcription text.
    pub prev_foreground: Mutex<Option<isize>>,
    /// Active context mode ID.
    pub active_context_mode_id: Mutex<Option<String>>,

    // ── Structured Mode / LLM side ────────────────────────────────────────
    /// Dedicated llama.cpp worker.  None until the first model is loaded.
    /// Wrapped in Arc so async extraction calls don't pin the mutex.
    pub llm_runner: Mutex<Option<Arc<LlmRunner>>>,
    /// ID of the currently active LLM model.
    pub active_llm_model_id: Mutex<Option<String>>,
    /// LLM model catalog.
    pub llm_model_manager: LlmModelManager,
    /// Streaming LLM downloader (sibling of `downloader` but on its own event channel).
    pub llm_downloader: LlmModelDownloader,
    /// Directory where GGUF LLM files live (sibling of `models_dir`).
    pub llm_models_dir: PathBuf,
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("omnivox");
        let models_dir = data_dir.join("models");
        let llm_models_dir = data_dir.join("llm_models");
        let db_path = data_dir.join("omnivox.db");

        // Initialize database. Create tables on first run.
        let db = Database::init(&db_path).expect("Failed to initialize database");

        // Load saved writing style so it persists across restarts.
        let writing_style = crate::storage::settings::get_settings(&db)
            .map(|s| WritingStyle::from_str(&s.writing_style))
            .unwrap_or_default();
        let processor_config = ProcessorConfig {
            writing_style,
            ..ProcessorConfig::default()
        };

        Self {
            audio: Mutex::new(AudioCapture::new(AudioConfig::default())),
            engine: Mutex::new(None),
            processor: Mutex::new(ProcessorChain::new(processor_config)),
            output: OutputRouter::new(),
            output_config: Mutex::new(OutputConfig::default()),
            model_manager: ModelManager::new(models_dir.clone()),
            downloader: ModelDownloader::new(models_dir.clone()),
            active_model_id: Mutex::new(None),
            db,
            data_dir,
            models_dir,
            prev_foreground: Mutex::new(None),
            active_context_mode_id: Mutex::new(None),
            llm_runner: Mutex::new(None),
            active_llm_model_id: Mutex::new(None),
            llm_model_manager: LlmModelManager::new(llm_models_dir.clone()),
            llm_downloader: LlmModelDownloader::new(llm_models_dir.clone()),
            llm_models_dir,
        }
    }
}
