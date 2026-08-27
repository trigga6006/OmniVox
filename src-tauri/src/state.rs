use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureStopIntent {
    Finish,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePhase {
    Starting { pending: Option<CaptureStopIntent> },
    Live,
    Stopping,
}

/// All mutable ownership for one microphone capture generation.
///
/// The receivers and liveness token live here so a worker from an old capture
/// can never observe or overwrite a newer capture's state.
pub struct CaptureSession {
    pub generation: u64,
    pub mode: CaptureMode,
    pub phase: CapturePhase,
    /// Monotonic timestamps used only to derive content-free latency metrics.
    pub claimed_at: Instant,
    pub mic_live_at: Option<Instant>,
    pub stop_requested_at: Option<Instant>,
    pub live: Arc<AtomicBool>,
    pub target: Option<crate::focus::WindowTarget>,
    /// True only when this capture was explicitly routed into the mounted
    /// StructuredPanel editor. Snapshotted per generation so panel lifecycle
    /// changes during transcription cannot accidentally enable the LLM.
    pub structured_panel_edit: bool,
    pub settings: crate::storage::types::AppSettings,
    pub output_config: OutputConfig,
    pub structured_profile: &'static crate::llm::profiles::Profile,
    pub screen_context_rx: Option<oneshot::Receiver<crate::screen_context::ScreenContext>>,
    pub preview_done_rx: Option<oneshot::Receiver<()>>,
    pub command_context: Option<CommandContext>,
}

/// Immutable per-command binding, created at capture (`start_command_inner`) and
/// carried through classify → pending → execute → undo.  It fixes the command's
/// target window ONCE, so execution never re-reads the shared `prev_foreground`
/// slot (which a concurrent dictation can overwrite — the H1 redirect).  The
/// `id` is a monotonic token used for confirm-id matching and cancellation
/// gating.
#[derive(Debug, Clone, Copy)]
pub struct CommandContext {
    /// Capture generation that produced this command, used for event ordering.
    pub generation: u64,
    /// Monotonic command id (see [`AppState::next_command_id`]).
    pub id: u64,
    /// The window this command was aimed at, captured at command start.
    pub target_hwnd: Option<isize>,
    /// PID that owned `target_hwnd` at capture — verified against the live
    /// foreground/window before every side-effecting primitive so a recycled
    /// HWND can't redirect the action.
    pub target_pid: Option<u32>,
    /// When the command was captured — bounds how long a parked confirm stays
    /// executable.
    pub captured_at: Instant,
}

impl CommandContext {
    /// The bound target as a [`crate::focus::WindowTarget`], if one was captured.
    pub fn target(&self) -> Option<crate::focus::WindowTarget> {
        self.target_hwnd.map(|hwnd| crate::focus::WindowTarget {
            hwnd,
            pid: self.target_pid,
        })
    }
}

/// A Command-Mode action awaiting user confirmation (Enter/Esc in the pill).
/// Used for actions we won't fire blind: a low-confidence "open app" match (we
/// never guess-launch the wrong app) and consequential ones like closing a
/// window.  Parked together with the [`CommandContext`] that produced it so the
/// confirm executes against the bound target, not the live foreground.
#[derive(Debug, Clone)]
pub enum PendingCommand {
    /// Low-confidence app match — launch this AppsFolder entry on confirm.
    OpenApp { app_id: String, name: String },
    /// Close the captured foreground window on confirm.  `pid` is the process
    /// that owned `hwnd` at capture — re-verified before the WM_CLOSE so a
    /// recycled handle can't close an unrelated window.
    CloseWindow {
        hwnd: isize,
        pid: Option<u32>,
        title: String,
    },
    /// An intent sequence containing a consequential step (sending a typed
    /// message with Enter) — run the whole chain on confirm. May be a single
    /// intent; the chain runner handles that fine.
    Chain {
        intents: Vec<crate::actions::CommandIntent>,
    },
    /// Wipe the scratchpad's saved cards + note on confirm (destructive).
    ClearScratchpad,
}

/// What the most recent executed command can undo (Phase A minimum-viable
/// undo).  One slot, overwritten per undoable action — "undo that" always
/// refers to the last thing the assistant did.  Submitted messages are
/// deliberately NOT undoable; the confirm gate is their protection.
#[derive(Debug, Clone)]
pub enum LastAction {
    /// A launched app's verified foreground window — undo closes it
    /// gracefully (WM_CLOSE, so the app runs its own save prompt).  Carries the
    /// window identity so undo re-verifies before closing.
    LaunchedApp {
        target: crate::focus::WindowTarget,
        name: String,
    },
    /// A minimized window — undo restores it.
    Minimized { target: crate::focus::WindowTarget },
    /// Show-desktop — undo toggles it back (Win+D is a toggle).
    ShowDesktop,
    /// Non-submitting typed text — undo sends Ctrl+Z at the target window.
    TypedText {
        target: Option<crate::focus::WindowTarget>,
    },
}

/// Central application state, managed by Tauri.
///
/// All mutable fields are behind `Mutex` for thread-safe access from
/// async command handlers and the global shortcut callback.
pub struct AppState {
    /// Microphone capture engine
    pub audio: Mutex<AudioCapture>,
    /// Active ASR engine (Whisper or Parakeet). None until a model is loaded.
    /// Each variant wraps an Arc, so transcription can snapshot the engine and
    /// run on a blocking thread without holding the Mutex for the duration of
    /// CPU-bound inference.
    pub engine: Mutex<Option<crate::asr::LoadedAsrEngine>>,
    /// Serializes final-ASR engine snapshot/enqueue with model replacement.
    /// A switch holds this across worker reset and native load, so a request
    /// either enters the old worker before reset or observes the new engine.
    pub asr_model_transition: Mutex<()>,
    /// Text post-processing chain (capitalization, dictionary, etc.)
    pub processor: Mutex<ProcessorChain>,
    /// Output router (clipboard / keystroke simulation)
    pub output: OutputRouter,
    /// Output mode configuration
    pub output_config: Mutex<OutputConfig>,
    /// In-memory settings authority for latency-sensitive capture paths.
    pub settings: RwLock<crate::storage::settings::RuntimeSettings>,
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
    /// Compatibility mirror for code paths being migrated to `capture.target`.
    /// Immutable dictation target (hwnd + owning pid) captured at recording
    /// START, so an inline consequential voice command (Send/Enter, mouse, key
    /// combo) can re-verify it is still firing into the SAME window — binding
    /// the pid at capture (not at output time) defeats an HWND recycled to a
    /// different process between start and output (B2-3).
    /// Active context mode ID.
    pub active_context_mode_id: Mutex<Option<String>>,

    // ── Structured Mode / LLM side ────────────────────────────────────────
    /// Dedicated llama.cpp worker paired with the canonical id of the model it
    /// was loaded for.  None until the first model is loaded.  Storing the id
    /// alongside the runner under ONE mutex means a read returns a consistent
    /// (model, runner) pair — a concurrent model switch can never hand back one
    /// model's runner keyed to another (B2-16).  Arc so async extraction calls
    /// don't pin the mutex.
    pub llm_runner: Mutex<Option<(String, Arc<LlmRunner>)>>,
    /// Dedicated cleanup-stage worker, paired with the canonical id of the
    /// model it was loaded for.  Separate from `llm_runner` because the two
    /// stages run different models: Structured Mode's extractor stays warm
    /// while the normalizer runs, and neither evicts the other's KV cache.
    pub cleanup_runner: Mutex<Option<(String, Arc<LlmRunner>)>>,
    /// Active Structured Mode profile (from the active context mode's
    /// `structured_profile`).  Read when the runner spawns so it warms the
    /// right system prompt; mode activation keeps it and the runner in sync.
    pub active_structured_profile: Mutex<&'static crate::llm::profiles::Profile>,
    /// ID of the currently active LLM model.
    pub active_llm_model_id: Mutex<Option<String>>,
    /// Lifecycle epoch captured by every LLM load before native initialization.
    /// Disabling both LLM-backed modes advances it so an older in-flight load
    /// cannot install its runner after the disable has reclaimed model memory.
    pub llm_load_epoch: AtomicU64,
    /// LLM model catalog.
    pub llm_model_manager: LlmModelManager,
    /// Streaming LLM downloader (sibling of `downloader` but on its own event channel).
    pub llm_downloader: LlmModelDownloader,
    /// Directory where GGUF LLM files live (sibling of `models_dir`).
    pub llm_models_dir: PathBuf,
    /// One-time, generation- and target-bound capabilities for Structured
    /// Mode's editable preview panel.
    pub structured_outputs: crate::structured_output::StructuredOutputRegistry,
    /// Set only by the exact overlay window while StructuredPanel is mounted.
    /// This distinguishes clicking the ordinary pill (where the real target is
    /// the app behind it) from dictating into the panel's own editor.
    pub structured_panel_active: AtomicBool,
    /// Coalesces privacy cleanup so long-running sessions enforce retention
    /// without putting SQLite/WAL work on the transcription critical path.
    pub history_purge_in_progress: AtomicBool,
    pub history_purge_requested: AtomicBool,
    pub history_last_purge_epoch: AtomicU64,

    /// Compatibility slots. New capture workers own these receivers inside
    /// `CaptureSession`; they remain during the orchestration migration.

    // ── Screen-context side ───────────────────────────────────────────────
    /// Receiver for the screen-context capture spawned at recording start.
    /// The pipeline drains this just before transcription so capture cost
    /// (~50–250 ms) is hidden under the user's speaking time.  `None` when
    /// the feature is disabled or no capture has been spawned yet.
    /// Signals when the live-preview Whisper worker has dropped its decode
    /// state. Stop waits briefly on this before final transcription so large
    /// models don't double-allocate preview + final decode buffers.

    // ── Command Mode ──────────────────────────────────────────────────────
    /// Which capture (dictation vs command) is currently active. This is the
    /// ownership gate: a start claims it (Idle → mode), a stop releases it.
    pub capture: Mutex<Option<CaptureSession>>,
    /// Monotonic capture generation. Zero is reserved for "no capture".
    pub capture_generation: AtomicU64,
    /// True once `audio.start()` has succeeded for the active capture. Lets the
    /// stop path distinguish "still starting" from "live" via a plain atomic,
    /// avoiding a capture_mode↔audio lock-order inversion.
    /// Set when a stop arrives before the capture is live (a quick push-to-talk
    /// tap). The start path consumes it once audio is up and stops itself, so a
    /// fast tap can never leave a capture stuck "recording" forever.
    /// A resolved command awaiting confirmation, parked with the
    /// [`CommandContext`] that produced it so the confirm executes against the
    /// bound target + id, not the live foreground.
    pub pending_command: Mutex<Option<(CommandContext, PendingCommand)>>,
    /// The current command's immutable binding, handed from the capture start
    /// (`start_command_inner`) to the stop/classify path.  Snapshotted into a
    /// local there so a concurrent capture overwriting this slot can't redirect
    /// an in-flight command.
    /// Monotonic command-id generator.  Each command capture claims a fresh id
    /// (see [`AppState::next_command_id`]); ids order commands for cancellation
    /// gating and confirm-id matching.
    pub command_id_gen: AtomicU64,
    /// Cancellation floor (H6).  A spoken "stop"/"cancel" raises this to the
    /// current command id; any command whose id is `<=` the floor is cancelled.
    /// Monotonic — never cleared, so a stop during classification still prevents
    /// execution, and later commands (higher ids) proceed normally.  Arc so a
    /// `spawn_blocking` command closure can hold a live handle and re-check the
    /// floor immediately before firing its primitive (B2-13).
    pub command_cancel_floor: Arc<AtomicU64>,
    /// The most recent undoable command action ("undo that").
    pub last_action: Mutex<Option<LastAction>>,

    /// When true AND the scratchpad window is visible, the global dictation
    /// hotkey routes into the scratchpad regardless of which app is focused — so
    /// you can read in one window and dictate answers into the pad. Default OFF
    /// so dictation lands where you're focused; the pad's Crosshair toggle opts
    /// in. Gated on window visibility, so it can never hijack normal dictation
    /// while the pad is closed. Dictating with the pad *itself* focused still
    /// routes to the pad (foreground match), independent of this flag.
    pub scratchpad_capture: std::sync::atomic::AtomicBool,
    /// Independent, bounded long-running meeting capture and transcription.
    pub meeting: crate::meeting::MeetingManager,
}

impl AppState {
    /// Claim the next monotonic command id.  1-based, so `0` is never a valid
    /// command id (it doubles as the "no command" floor sentinel).
    pub fn next_command_id(&self) -> u64 {
        self.command_id_gen.fetch_add(1, Ordering::SeqCst) + 1
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
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
        let runtime_settings =
            crate::storage::settings::RuntimeSettings::load(&db).unwrap_or_default();
        let saved = Some(runtime_settings.values().clone());
        let writing_style = saved
            .as_ref()
            .map(|s| WritingStyle::parse(&s.writing_style))
            .unwrap_or_default();
        let filler_removal = saved.as_ref().map(|s| s.filler_removal).unwrap_or(true);
        let processor_config = ProcessorConfig {
            writing_style,
            apply_filler_removal: filler_removal,
            ..ProcessorConfig::default()
        };
        let audio_config = AudioConfig {
            device_id: crate::storage::settings::get_audio_device_id(&db).unwrap_or_default(),
            ..AudioConfig::default()
        };

        Self {
            audio: Mutex::new(AudioCapture::new(audio_config)),
            engine: Mutex::new(None),
            asr_model_transition: Mutex::new(()),
            processor: Mutex::new(ProcessorChain::new(processor_config)),
            output: OutputRouter::new(),
            output_config: Mutex::new(OutputConfig::default()),
            settings: RwLock::new(runtime_settings),
            model_manager: ModelManager::new(models_dir.clone()),
            downloader: ModelDownloader::new(models_dir.clone()),
            active_model_id: Mutex::new(None),
            db,
            data_dir,
            models_dir,
            prev_foreground: Mutex::new(None),
            active_context_mode_id: Mutex::new(None),
            llm_runner: Mutex::new(None),
            cleanup_runner: Mutex::new(None),
            active_structured_profile: Mutex::new(crate::llm::profiles::get(
                crate::llm::profiles::DEFAULT_PROFILE_ID,
            )),
            active_llm_model_id: Mutex::new(None),
            llm_load_epoch: AtomicU64::new(0),
            llm_model_manager: LlmModelManager::new(llm_models_dir.clone()),
            llm_downloader: LlmModelDownloader::new(llm_models_dir.clone()),
            llm_models_dir,
            structured_outputs: crate::structured_output::StructuredOutputRegistry::default(),
            structured_panel_active: AtomicBool::new(false),
            history_purge_in_progress: AtomicBool::new(false),
            history_purge_requested: AtomicBool::new(false),
            history_last_purge_epoch: AtomicU64::new(0),
            capture: Mutex::new(None),
            capture_generation: AtomicU64::new(0),
            pending_command: Mutex::new(None),
            command_id_gen: AtomicU64::new(0),
            command_cancel_floor: Arc::new(AtomicU64::new(0)),
            last_action: Mutex::new(None),
            scratchpad_capture: std::sync::atomic::AtomicBool::new(false),
            meeting: crate::meeting::MeetingManager::new(),
        }
    }
}
