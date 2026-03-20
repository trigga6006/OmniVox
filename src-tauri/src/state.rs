use std::path::PathBuf;
use std::sync::Mutex;

use crate::audio::capture::AudioCapture;
use crate::audio::types::AudioConfig;
use crate::models::downloader::ModelDownloader;
use crate::models::manager::ModelManager;
use crate::output::router::OutputRouter;
use crate::output::types::OutputConfig;
use crate::postprocess::processor::ProcessorChain;
use crate::postprocess::types::ProcessorConfig;
use crate::storage::database::Database;

/// Central application state, managed by Tauri.
///
/// All mutable fields are behind `Mutex` for thread-safe access from
/// async command handlers and the global shortcut callback.
pub struct AppState {
    /// Microphone capture engine
    pub audio: Mutex<AudioCapture>,
    /// Whisper engine — None until a model is loaded
    pub engine: Mutex<Option<crate::asr::engine::WhisperEngine>>,
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
}

impl AppState {
    pub fn new() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("omnivox");
        let models_dir = data_dir.join("models");
        let db_path = data_dir.join("omnivox.db");

        // Initialize database — create tables on first run
        let db = Database::init(&db_path)
            .expect("Failed to initialize database");

        Self {
            audio: Mutex::new(AudioCapture::new(AudioConfig::default())),
            engine: Mutex::new(None),
            processor: Mutex::new(ProcessorChain::new(ProcessorConfig::default())),
            output: OutputRouter::new(),
            output_config: Mutex::new(OutputConfig::default()),
            model_manager: ModelManager::new(models_dir.clone()),
            downloader: ModelDownloader::new(models_dir.clone()),
            active_model_id: Mutex::new(None),
            db,
            data_dir,
            models_dir,
        }
    }
}
