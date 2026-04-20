use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

use crate::error::{AppError, AppResult};
use crate::llm::engine::LlmEngine;
use crate::llm::schema::SlotExtraction;

/// Dedicated-worker runner.
///
/// Owns exactly one llama.cpp engine behind a worker thread, with a bounded
/// capacity-1 request queue.  The pipeline uses this instead of a raw
/// `spawn_blocking` + `tokio::time::timeout` combo because timing out the
/// future does NOT cancel the native llama.cpp decode loop — stacked timeouts
/// would pile work up in the blocking pool.
///
/// Guarantees:
/// - At most one extraction in flight at any time.
/// - Timed-out requests drop their receiver; results silently die when the
///   worker finally finishes — they cannot appear later as stale output.
/// - Backpressure: if a request arrives while the worker is busy, the caller
///   degrades to plain dictation instead of queuing.
pub struct LlmRunner {
    tx: SyncSender<LlmRequest>,
    last_used_ns: Arc<AtomicI64>,
    /// Kept alive to stop the worker cleanly on drop.
    _worker: WorkerHandle,
}

struct LlmRequest {
    text: String,
    reply_tx: oneshot::Sender<AppResult<SlotExtraction>>,
}

/// RAII: dropping the handle drops the sender, which lets the worker exit
/// after its current extraction.
struct WorkerHandle {
    _join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl LlmRunner {
    /// Spawn a dedicated worker thread owning `engine`.
    ///
    /// The thread gets a 256 MB stack to match the Whisper loader — llama.cpp
    /// has the same enormous debug-build stack frames that cause
    /// STATUS_STACK_BUFFER_OVERRUN on Windows without this.
    pub fn spawn<E: LlmEngine + 'static>(engine: E) -> AppResult<Self> {
        let (tx, rx) = sync_channel::<LlmRequest>(1);
        let last_used_ns = Arc::new(AtomicI64::new(Instant::now().elapsed().as_nanos() as i64));
        let worker_last_used = Arc::clone(&last_used_ns);

        let join = thread::Builder::new()
            .name("omnivox-llm".into())
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                let engine = engine;
                while let Ok(req) = rx.recv() {
                    let result = engine.extract_slots(&req.text);
                    // Receiver may have been dropped by a timeout — ignore.
                    let _ = req.reply_tx.send(result);
                    worker_last_used.store(
                        Instant::now().elapsed().as_nanos() as i64,
                        Ordering::Relaxed,
                    );
                }
            })
            .map_err(|e| AppError::Llm(format!("spawn LLM worker failed: {e}")))?;

        Ok(Self {
            tx,
            last_used_ns,
            _worker: WorkerHandle {
                _join: Mutex::new(Some(join)),
            },
        })
    }

    /// Submit a request and await the response with a timeout.
    ///
    /// Returns `Err` when:
    /// - The worker queue is full (another extraction in flight).
    /// - The timeout elapses.
    /// - The worker panicked (channel closed).
    /// - The engine itself failed.
    pub async fn extract_with_timeout(
        &self,
        text: String,
        timeout: Duration,
    ) -> AppResult<SlotExtraction> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = LlmRequest {
            text,
            reply_tx,
        };

        match self.tx.try_send(req) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err(AppError::Llm("LLM busy — another extraction in flight".into()));
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err(AppError::Llm("LLM worker has stopped".into()));
            }
        }

        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::Llm("LLM worker dropped reply".into())),
            Err(_) => Err(AppError::Llm(format!(
                "LLM extraction timed out after {:?}",
                timeout
            ))),
        }
    }

    /// Nanoseconds since process start when the worker last finished a job.
    /// Used by the idle-unload timer in AppState.
    pub fn last_used_ns(&self) -> i64 {
        self.last_used_ns.load(Ordering::Relaxed)
    }
}
