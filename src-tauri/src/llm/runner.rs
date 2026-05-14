use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    busy: Arc<AtomicBool>,
    last_used_ns: Arc<AtomicI64>,
    /// Kept alive to stop the worker cleanly on drop.
    _worker: WorkerHandle,
}

struct LlmRequest {
    text: String,
    screen_tokens: Vec<String>,
    source_app: Option<String>,
    reply_tx: oneshot::Sender<AppResult<SlotExtraction>>,
}

/// RAII: dropping the handle drops the sender, which lets the worker exit
/// after its current extraction.
struct WorkerHandle {
    _join: Mutex<Option<thread::JoinHandle<()>>>,
}

struct BusyReset(Arc<AtomicBool>);

impl Drop for BusyReset {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

impl LlmRunner {
    /// Spawn a dedicated worker thread owning `engine`.
    ///
    /// The thread gets a 256 MB stack to match the Whisper loader — llama.cpp
    /// has the same enormous debug-build stack frames that cause
    /// STATUS_STACK_BUFFER_OVERRUN on Windows without this.
    pub fn spawn<E: LlmEngine + 'static>(engine: E) -> AppResult<Self> {
        let (tx, rx) = sync_channel::<LlmRequest>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let last_used_ns = Arc::new(AtomicI64::new(now_ns()));
        let worker_busy = Arc::clone(&busy);
        let worker_last_used = Arc::clone(&last_used_ns);

        let join = thread::Builder::new()
            .name("omnivox-llm".into())
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                let engine = engine;
                while let Ok(req) = rx.recv() {
                    let _busy_reset = BusyReset(Arc::clone(&worker_busy));
                    let result = engine.extract_slots_with_context(
                        &req.text,
                        &req.screen_tokens,
                        req.source_app.as_deref(),
                    );
                    // Receiver may have been dropped by a timeout — ignore.
                    let _ = req.reply_tx.send(result);
                    worker_last_used.store(now_ns(), Ordering::Relaxed);
                }
            })
            .map_err(|e| AppError::Llm(format!("spawn LLM worker failed: {e}")))?;

        Ok(Self {
            tx,
            busy,
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
        self.extract_with_context_and_timeout(text, Vec::new(), None, timeout)
            .await
    }

    /// Submit a request with optional screen-context tokens and await the
    /// response with a timeout.  Behaviour identical to `extract_with_timeout`
    /// when `screen_tokens` is empty (Phase 2 caller gates on a setting).
    pub async fn extract_with_context_and_timeout(
        &self,
        text: String,
        screen_tokens: Vec<String>,
        source_app: Option<String>,
        timeout: Duration,
    ) -> AppResult<SlotExtraction> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::Llm("LLM busy — another extraction in flight".into()));
        }

        let req = LlmRequest {
            text,
            screen_tokens,
            source_app,
            reply_tx,
        };

        match self.tx.try_send(req) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.busy.store(false, Ordering::Release);
                return Err(AppError::Llm("LLM busy — another extraction in flight".into()));
            }
            Err(TrySendError::Disconnected(_)) => {
                self.busy.store(false, Ordering::Release);
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

    /// Unix timestamp in nanoseconds when the worker last finished a job.
    pub fn last_used_ns(&self) -> i64 {
        self.last_used_ns.load(Ordering::Relaxed)
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct SlowEngine {
        delay: Duration,
    }

    impl LlmEngine for SlowEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            std::thread::sleep(self.delay);
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn rejects_second_request_while_native_inference_is_running() {
        let runner = Arc::new(
            LlmRunner::spawn(SlowEngine {
                delay: Duration::from_millis(100),
            })
            .unwrap(),
        );

        let first = {
            let runner = Arc::clone(&runner);
            tokio::spawn(async move {
                runner
                    .extract_with_timeout("first".into(), Duration::from_secs(1))
                    .await
            })
        };

        for _ in 0..100 {
            if runner.is_busy() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert!(runner.is_busy());

        let second = runner
            .extract_with_timeout("second".into(), Duration::from_secs(1))
            .await;
        assert!(second.unwrap_err().to_string().contains("busy"));

        let first_result = first.await.unwrap().unwrap();
        assert_eq!(first_result.goal, "first");
        assert!(!runner.is_busy());
    }
}
