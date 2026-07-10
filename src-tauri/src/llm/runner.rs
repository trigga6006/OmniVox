use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::oneshot;

use crate::actions::CommandIntent;
use crate::error::{AppError, AppResult};
use crate::llm::engine::LlmEngine;
use crate::llm::profiles::{Profile, ProfileOutput};

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
    /// The profile the worker's session should be warmed on.  Shared state
    /// (not a queued message) so a profile switch can never be lost to a full
    /// queue — the worker reconciles against this before every extraction and
    /// prewarm.  There is only ever ONE warmed session (one KV cache); a
    /// switch drops it and rebuilds on the new profile.
    desired_profile: Arc<Mutex<&'static Profile>>,
    /// Kept alive to stop the worker cleanly on drop.
    _worker: WorkerHandle,
}

enum LlmRequest {
    /// Structured Mode extraction on the active profile (uses the warmed
    /// KV-cache session).
    Extract {
        text: String,
        screen_tokens: Vec<String>,
        source_app: Option<String>,
        reply_tx: oneshot::Sender<AppResult<ProfileOutput>>,
    },
    /// Command Mode free-form classification (one-off throwaway context).
    Classify {
        utterance: String,
        reply_tx: oneshot::Sender<AppResult<Vec<CommandIntent>>>,
    },
    /// Opportunistic session rebuild (no reply). Sent when a recording starts
    /// so an idle-dropped KV cache is re-warmed while the user is speaking
    /// instead of on the extraction's critical path.
    Prewarm,
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

/// Poison-tolerant lock on the shared desired-profile cell.  The value is a
/// `&'static Profile` (plain pointer), so a panicked writer can't have left
/// it half-updated — recovering the inner value is always safe.
fn lock_profile<'a>(
    cell: &'a Mutex<&'static Profile>,
) -> std::sync::MutexGuard<'a, &'static Profile> {
    cell.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl LlmRunner {
    /// Spawn a dedicated worker thread owning `engine`, warmed on
    /// `initial_profile`.
    ///
    /// The thread gets a 256 MB stack to match the Whisper loader — llama.cpp
    /// has the same enormous debug-build stack frames that cause
    /// STATUS_STACK_BUFFER_OVERRUN on Windows without this.
    pub fn spawn<E: LlmEngine + 'static>(
        engine: E,
        initial_profile: &'static Profile,
    ) -> AppResult<Self> {
        let (tx, rx) = sync_channel::<LlmRequest>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let last_used_ns = Arc::new(AtomicI64::new(now_ns()));
        let desired_profile = Arc::new(Mutex::new(initial_profile));
        let worker_busy = Arc::clone(&busy);
        let worker_last_used = Arc::clone(&last_used_ns);
        let worker_desired = Arc::clone(&desired_profile);

        let join = thread::Builder::new()
            .name("omnivox-llm".into())
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                use std::sync::mpsc::RecvTimeoutError;

                /// Drop the session and release its KV cache after this much
                /// idle time; it is rebuilt (system prompt re-warmed) on the
                /// next request.
                const SESSION_IDLE_SECS: i64 = 5 * 60;

                let engine = engine;
                // Persistent extraction session — keeps the active profile's
                // system prompt KV cached across requests so each extraction
                // only prefills the user's words.  Warmed here, in the
                // background, right after model activation.  On failure the
                // per-request rebuild below retries.
                let mut profile: &'static Profile = *lock_profile(&worker_desired);
                let mut session = match engine.new_session_for(profile) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        crate::llm::diaglog::log(&format!(
                            "runner: session init failed (will retry per-request): {e}"
                        ));
                        None
                    }
                };

                loop {
                    let req = match rx.recv_timeout(Duration::from_secs(60)) {
                        Ok(req) => req,
                        Err(RecvTimeoutError::Timeout) => {
                            // Idle housekeeping: release the session's KV
                            // cache (hundreds of MB at n_ctx 3072) when no
                            // dictation has needed it for a while.
                            if session.is_some() {
                                let idle_ns =
                                    now_ns().saturating_sub(worker_last_used.load(Ordering::Relaxed));
                                if idle_ns > SESSION_IDLE_SECS * 1_000_000_000 {
                                    session = None;
                                    crate::llm::diaglog::log(
                                        "runner: idle — released session KV cache",
                                    );
                                }
                            }
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    };

                    // Held only for requests that actually acquired the busy
                    // slot in `submit`. `Prewarm` bypasses `submit` (it never
                    // sets `busy`), so it must NOT create a guard — otherwise
                    // its drop at end-of-iteration would clear a *concurrent*
                    // extraction's busy flag and defeat single-flight
                    // backpressure (a second request could then queue behind
                    // the in-flight native decode).
                    let _busy_reset = match &req {
                        LlmRequest::Prewarm => None,
                        _ => Some(BusyReset(Arc::clone(&worker_busy))),
                    };

                    // Reconcile with the desired profile before any session
                    // use.  A switch drops the old session (its KV prefix is
                    // useless for the new prompt) — never two sessions alive.
                    let want: &'static Profile = *lock_profile(&worker_desired);
                    if want.id != profile.id {
                        session = None;
                        profile = want;
                        crate::llm::diaglog::log(&format!(
                            "runner: switching to profile '{}'",
                            profile.id
                        ));
                    }

                    match req {
                        LlmRequest::Extract {
                            text,
                            screen_tokens,
                            source_app,
                            reply_tx,
                        } => {
                            // Rebuild the session if it was idle-dropped,
                            // profile-switched, or failed at init.  The
                            // request then pays one full warm-up.
                            if session.is_none() {
                                session = engine.new_session_for(profile).ok();
                            }
                            let result = match session.as_mut() {
                                Some(s) => s
                                    .generate_raw(
                                        &text,
                                        &screen_tokens,
                                        source_app.as_deref(),
                                    )
                                    .and_then(|raw| (profile.postprocess)(&raw, &text)),
                                None => Err(AppError::Llm(
                                    "LLM session unavailable".into(),
                                )),
                            };
                            // Release the busy slot BEFORE delivering the
                            // reply: the native decode is done, and a caller
                            // that observes the result must never race a
                            // still-set busy flag (spurious rejections).
                            drop(_busy_reset);
                            // Receiver may have been dropped by a timeout — ignore.
                            let _ = reply_tx.send(result);
                        }
                        LlmRequest::Classify { utterance, reply_tx } => {
                            // Runs on a throwaway context inside `classify_command`
                            // so the warmed extraction session is left intact.
                            let result = engine.classify_command(&utterance);
                            drop(_busy_reset);
                            let _ = reply_tx.send(result);
                        }
                        LlmRequest::Prewarm => {
                            if session.is_none() {
                                session = engine.new_session_for(profile).ok();
                                crate::llm::diaglog::log(&format!(
                                    "runner: prewarmed session ({})",
                                    profile.id
                                ));
                            }
                        }
                    }
                    worker_last_used.store(now_ns(), Ordering::Relaxed);
                }
            })
            .map_err(|e| AppError::Llm(format!("spawn LLM worker failed: {e}")))?;

        Ok(Self {
            tx,
            busy,
            last_used_ns,
            desired_profile,
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
    ) -> AppResult<ProfileOutput> {
        self.extract_with_context_and_timeout(text, Vec::new(), None, timeout)
            .await
    }

    /// Shared dispatch protocol for every request kind: acquire the single
    /// busy slot, enqueue, and await the reply with a timeout.  On a send
    /// failure the busy slot is released here; on success the worker's
    /// `BusyReset` releases it (so a timed-out request still can't pile work
    /// up — the slot stays held until the native decode finishes).  `op` only
    /// flavors the error text ("extraction" / "classify").
    async fn submit<T>(
        &self,
        req: LlmRequest,
        reply_rx: oneshot::Receiver<AppResult<T>>,
        timeout: Duration,
        op: &str,
    ) -> AppResult<T> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(AppError::Llm(format!("LLM busy — another {op} in flight")));
        }

        match self.tx.try_send(req) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.busy.store(false, Ordering::Release);
                return Err(AppError::Llm(format!("LLM busy — another {op} in flight")));
            }
            Err(TrySendError::Disconnected(_)) => {
                self.busy.store(false, Ordering::Release);
                return Err(AppError::Llm("LLM worker has stopped".into()));
            }
        }

        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::Llm("LLM worker dropped reply".into())),
            Err(_) => Err(AppError::Llm(format!("LLM {op} timed out after {timeout:?}"))),
        }
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
    ) -> AppResult<ProfileOutput> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = LlmRequest::Extract {
            text,
            screen_tokens,
            source_app,
            reply_tx,
        };
        self.submit(req, reply_rx, timeout, "extraction").await
    }

    /// Classify a free-form Command-Mode utterance into an ordered sequence of
    /// `CommandIntent`s (multi-step chains supported).  Same busy-guard + timeout
    /// discipline as extraction; returns an empty Vec when the model decides the
    /// input isn't a recognizable command.
    pub async fn classify_command_with_timeout(
        &self,
        utterance: String,
        timeout: Duration,
    ) -> AppResult<Vec<CommandIntent>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let req = LlmRequest::Classify { utterance, reply_tx };
        self.submit(req, reply_rx, timeout, "classify").await
    }

    /// Fire-and-forget: rebuild the warmed extraction session if it was
    /// idle-dropped.  Silently ignored when the worker is busy or the queue
    /// is full — this is an opportunistic head start, never load-bearing.
    /// Does not take the busy slot: an extraction submitted while the
    /// prewarm runs simply queues behind it and then benefits from the
    /// freshly warmed session.
    pub fn prewarm(&self) {
        let _ = self.tx.try_send(LlmRequest::Prewarm);
    }

    /// Switch the active Structured Mode profile.
    ///
    /// Updates the shared desired-profile cell (can't be lost, even when the
    /// worker is busy) and nudges a background re-warm so the KV session is
    /// rebuilt on the new system prompt while the user isn't dictating.  If
    /// the nudge is dropped (queue full), the next extraction reconciles and
    /// pays the one-time warm cost itself — slow once, never wrong.
    pub fn set_profile(&self, profile: &'static Profile) {
        {
            let mut desired = lock_profile(&self.desired_profile);
            if desired.id == profile.id {
                return;
            }
            *desired = profile;
        }
        let _ = self.tx.try_send(LlmRequest::Prewarm);
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
    use crate::llm::profiles;
    use crate::llm::schema::SlotExtraction;
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
            LlmRunner::spawn(
                SlowEngine {
                    delay: Duration::from_millis(100),
                },
                profiles::get(profiles::DEFAULT_PROFILE_ID),
            )
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
        assert_eq!(first_result.slots["goal"], "first");
        assert!(first_result.markdown.contains("first"));
        assert!(!runner.is_busy());
    }

    /// Engine that records which profile each session was built for, so the
    /// test can observe the switch-triggered rebuild.
    struct RecordingEngine {
        sessions: Arc<Mutex<Vec<&'static str>>>,
    }

    impl LlmEngine for RecordingEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }

        fn new_session_for(
            &self,
            profile: &'static Profile,
        ) -> AppResult<Box<dyn crate::llm::engine::LlmSession + '_>> {
            self.sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(profile.id);
            Ok(Box::new(crate::llm::engine::StatelessSession(self)))
        }
    }

    #[tokio::test]
    async fn set_profile_rebuilds_session_on_new_profile_in_background() {
        let sessions = Arc::new(Mutex::new(Vec::new()));
        let runner = LlmRunner::spawn(
            RecordingEngine {
                sessions: Arc::clone(&sessions),
            },
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .unwrap();

        // Initial session is built (in the worker) on the default profile.
        for _ in 0..200 {
            if !sessions.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(*sessions.lock().unwrap(), vec!["agent-prompt"]);

        // Switching kicks a background re-warm on the new profile — exactly
        // one new session, no extraction needed.
        runner.set_profile(profiles::get("email"));
        for _ in 0..200 {
            if sessions.lock().unwrap().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        assert_eq!(*sessions.lock().unwrap(), vec!["agent-prompt", "email"]);

        // Setting the same profile again is a no-op (no session churn).
        runner.set_profile(profiles::get("email"));
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(sessions.lock().unwrap().len(), 2);
    }
}
