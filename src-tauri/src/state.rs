use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::oneshot;

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

/// Which capture is currently active.
///
/// Dictation and Command Mode share the one microphone + Whisper engine, so a
/// stray hotkey release must not run the wrong pipeline.  The stop paths key off
/// this to decide whether a capture is theirs to finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMode {
    Idle,
    Dictation,
    Command,
}

/// A Command-Mode action awaiting user confirmation (Enter/Esc in the pill).
/// Used for actions we won't fire blind: a low-confidence "open app" match (we
/// never guess-launch the wrong app) and consequential ones like closing a
/// window.
#[derive(Debug, Clone)]
pub enum PendingCommand {
    /// Low-confidence app match — launch this AppsFolder entry on confirm.
    OpenApp { app_id: String, name: String },
    /// Close the captured foreground window on confirm.
    CloseWindow { hwnd: isize, title: String },
    /// An intent sequence containing a consequential step (sending a typed
    /// message with Enter) — run the whole chain on confirm. May be a single
    /// intent; the chain runner handles that fine.
    Chain {
        intents: Vec<crate::actions::CommandIntent>,
    },
}

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
    /// Local SQLite database for persistent storage.
    /// Arc so heavy read commands (analytics, export, search) can move a
    /// handle onto a blocking thread instead of stalling the async runtime.
    pub db: Arc<Database>,
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

    // ── Screen-context side ───────────────────────────────────────────────
    /// Receiver for the screen-context capture spawned at recording start.
    /// The pipeline drains this just before transcription so capture cost
    /// (~50–250 ms) is hidden under the user's speaking time.  `None` when
    /// the feature is disabled or no capture has been spawned yet.
    pub screen_context_rx: Mutex<Option<oneshot::Receiver<crate::screen_context::ScreenContext>>>,
    /// Signals when the live-preview Whisper worker has dropped its decode
    /// state. Stop waits briefly on this before final transcription so large
    /// models don't double-allocate preview + final decode buffers.
    pub preview_done_rx: Mutex<Option<oneshot::Receiver<()>>>,

    // ── Command Mode ──────────────────────────────────────────────────────
    /// Which capture (dictation vs command) is currently active. This is the
    /// ownership gate: a start claims it (Idle → mode), a stop releases it.
    pub capture_mode: Mutex<CaptureMode>,
    /// True once `audio.start()` has succeeded for the active capture. Lets the
    /// stop path distinguish "still starting" from "live" via a plain atomic,
    /// avoiding a capture_mode↔audio lock-order inversion.
    pub capture_live: std::sync::atomic::AtomicBool,
    /// Set when a stop arrives before the capture is live (a quick push-to-talk
    /// tap). The start path consumes it once audio is up and stops itself, so a
    /// fast tap can never leave a capture stuck "recording" forever.
    pub pending_stop: std::sync::atomic::AtomicBool,
    /// A resolved command awaiting confirmation (low-confidence app match).
    pub pending_command: Mutex<Option<PendingCommand>>,
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
        let db = Arc::new(Database::init(&db_path).expect("Failed to initialize database"));

        // Load saved writing style + filler removal so they persist across restarts.
        let saved = crate::storage::settings::get_settings(&db).ok();
        let writing_style = saved
            .as_ref()
            .map(|s| WritingStyle::from_str(&s.writing_style))
            .unwrap_or_default();
        let filler_removal = saved.as_ref().map(|s| s.filler_removal).unwrap_or(true);
        let processor_config = ProcessorConfig {
            writing_style,
            apply_filler_removal: filler_removal,
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
            screen_context_rx: Mutex::new(None),
            preview_done_rx: Mutex::new(None),
            capture_mode: Mutex::new(CaptureMode::Idle),
            capture_live: std::sync::atomic::AtomicBool::new(false),
            pending_stop: std::sync::atomic::AtomicBool::new(false),
            pending_command: Mutex::new(None),
        }
    }
}
