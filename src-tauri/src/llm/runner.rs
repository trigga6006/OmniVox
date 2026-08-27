use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::oneshot;

use crate::actions::CommandIntent;
use crate::error::{AppError, AppResult};
use crate::llm::engine::{InferenceControl, LlmCleanupSession, LlmCommandSession, LlmEngine};
use crate::llm::profiles::{Profile, ProfileOutput};

/// Which stage a runner's worker serves.
///
/// One runner owns one model, and the two stages need different models, so the
/// role decides which session the worker warms at startup and rebuilds on
/// prewarm.  Requests for the other role's session still work (they build
/// lazily), but nothing in the app crosses roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerRole {
    /// Structured Mode extraction + Command Mode classification.
    Structured,
    /// Dictation transcript cleanup.
    Cleanup,
}

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
/// - Timed-out requests cancel cooperatively; their dropped receiver also
///   prevents a late native result from appearing as stale output.
/// - Backpressure: if a request arrives while the worker is busy, the caller
///   degrades to plain dictation instead of queuing.
pub struct LlmRunner {
    tx: SyncSender<LlmRequest>,
    busy: Arc<AtomicBool>,
    /// Set by the worker when it releases the model weights after a long idle
    /// and ends its thread.  The runner object outlives the worker, so the
    /// keyed slot lookups in `commands::llm` consult this to treat the slot as
    /// empty and reload transparently.  See [`LlmRunner::is_parked`].
    parked: Arc<AtomicBool>,
    last_used_ns: Arc<AtomicI64>,
    /// The profile the worker's session should be warmed on.  Shared state
    /// (not a queued message) so a profile switch can never be lost to a full
    /// queue — the worker reconciles against this before every extraction and
    /// prewarm.  There is only ever ONE warmed session (one KV cache); a
    /// switch drops it and rebuilds on the new profile.
    desired_profile: Arc<Mutex<&'static Profile>>,
    shutdown: Arc<AtomicBool>,
    worker: WorkerHandle,
}

enum LlmRequest {
    /// Structured Mode extraction on the active profile (uses the warmed
    /// KV-cache session).
    Extract {
        text: String,
        screen_tokens: Vec<String>,
        source_app: Option<String>,
        control: InferenceControl,
        reply_tx: oneshot::Sender<AppResult<ProfileOutput>>,
    },
    /// Command Mode free-form classification (dedicated persistent context).
    Classify {
        utterance: String,
        control: InferenceControl,
        reply_tx: oneshot::Sender<AppResult<Vec<CommandIntent>>>,
    },
    /// Grammar-free transcript cleanup (dedicated persistent context).
    Cleanup {
        text: String,
        styling: String,
        control: InferenceControl,
        reply_tx: oneshot::Sender<AppResult<String>>,
    },
    /// Opportunistic session rebuild (no reply). Sent when a recording starts
    /// so an idle-dropped KV cache is re-warmed while the user is speaking
    /// instead of on the extraction's critical path.
    Prewarm,
    Shutdown,
}

/// RAII: dropping the handle drops the sender, which lets the worker exit
/// after its current extraction.
struct WorkerHandle {
    join: Mutex<Option<thread::JoinHandle<()>>>,
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

/// The worker's two-tier idle-release schedule, evaluated on the recv timeout
/// tick.  There is no separate timer thread: every threshold below is checked
/// only when a `recv_timeout(tick)` expires with no work waiting.
///
/// Parameterized purely so tests can compress the (multi-minute) real
/// thresholds; production always uses [`IdlePolicy::DEFAULT`].
#[derive(Clone, Copy)]
struct IdlePolicy {
    /// How long the worker blocks for a request before doing housekeeping.
    tick: Duration,
    /// Idle time after which the warmed session's KV cache is released.
    session_idle_ns: i64,
    /// Idle time after which the model weights are released by ending the
    /// worker thread and parking the runner.
    weights_idle_ns: i64,
}

impl IdlePolicy {
    const DEFAULT: Self = Self {
        tick: Duration::from_secs(60),
        // Drop the session and release its KV cache after this much idle time;
        // it is rebuilt (system prompt re-warmed) on the next request.
        session_idle_ns: 5 * 60 * 1_000_000_000,
        // Second tier: release the model WEIGHTS after this much idle time.
        // The KV cache is hundreds of MB; the weights are on the order of a
        // gigabyte and, until now, stayed resident for the whole app session
        // even if the user never dictated again.  The next use pays one cold
        // load — the accepted trade.
        weights_idle_ns: 45 * 60 * 1_000_000_000,
    };
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
        Self::spawn_with_role(engine, initial_profile, RunnerRole::Structured)
    }

    /// Spawn a worker for the cleanup stage.  Startup warms the cleanup
    /// session's constant system-turn prefix instead of a Structured Mode
    /// profile — the profile below is inert for this role.
    pub fn spawn_cleanup<E: LlmEngine + 'static>(engine: E) -> AppResult<Self> {
        Self::spawn_with_role(
            engine,
            crate::llm::profiles::get(crate::llm::profiles::DEFAULT_PROFILE_ID),
            RunnerRole::Cleanup,
        )
    }

    fn spawn_with_role<E: LlmEngine + 'static>(
        engine: E,
        initial_profile: &'static Profile,
        role: RunnerRole,
    ) -> AppResult<Self> {
        Self::spawn_with_idle_policy(engine, initial_profile, role, IdlePolicy::DEFAULT)
    }

    fn spawn_with_idle_policy<E: LlmEngine + 'static>(
        engine: E,
        initial_profile: &'static Profile,
        role: RunnerRole,
        idle: IdlePolicy,
    ) -> AppResult<Self> {
        let (tx, rx) = sync_channel::<LlmRequest>(1);
        let busy = Arc::new(AtomicBool::new(false));
        let parked = Arc::new(AtomicBool::new(false));
        let last_used_ns = Arc::new(AtomicI64::new(now_ns()));
        let desired_profile = Arc::new(Mutex::new(initial_profile));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (startup_tx, startup_rx) = sync_channel::<Result<(), String>>(1);
        let worker_busy = Arc::clone(&busy);
        let worker_parked = Arc::clone(&parked);
        let worker_last_used = Arc::clone(&last_used_ns);
        let worker_desired = Arc::clone(&desired_profile);
        let worker_shutdown = Arc::clone(&shutdown);

        let join = thread::Builder::new()
            .name("omnivox-llm".into())
            .stack_size(256 * 1024 * 1024)
            .spawn(move || {
                use std::sync::mpsc::RecvTimeoutError;

                let engine = engine;
                // Persistent extraction session — keeps the active profile's
                // system prompt KV cached across requests so each extraction
                // only prefills the user's words.  Warmed here, in the
                // background, right after model activation.  On failure the
                // per-request rebuild below retries.
                let mut profile: &'static Profile = *lock_profile(&worker_desired);
                // Wrap the initial session build in `catch_unwind` too (B2-9): a
                // PANIC here (not just an Err) would otherwise unwind the whole
                // worker thread, leaving a dead runner installed with a
                // disconnected channel and no self-heal.  On panic/err we start
                // with no session; the per-request rebuild (also guarded) retries.
                let startup_control = InferenceControl::shutdown_only(Arc::clone(&worker_shutdown));
                let mut session: Option<Box<dyn crate::llm::engine::LlmSession + '_>> = None;
                let mut command_session: Option<Box<dyn LlmCommandSession + '_>> = None;
                let mut cleanup_session: Option<Box<dyn LlmCleanupSession + '_>> = None;
                let startup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    match role {
                        RunnerRole::Structured => {
                            session =
                                Some(engine.new_session_for_controlled(profile, &startup_control)?);
                        }
                        RunnerRole::Cleanup => {
                            cleanup_session =
                                Some(engine.new_cleanup_session_controlled(&startup_control)?);
                        }
                    }
                    Ok::<(), AppError>(())
                }));
                match startup {
                    Ok(Ok(())) => {
                        let _ = startup_tx.send(Ok(()));
                    }
                    Ok(Err(e)) => {
                        let _ = startup_tx.send(Err(e.to_string()));
                        return;
                    }
                    Err(_) => {
                        let _ = startup_tx.send(Err("LLM session initialization panicked".into()));
                        return;
                    }
                }

                loop {
                    let req = match rx.recv_timeout(idle.tick) {
                        Ok(req) => req,
                        Err(RecvTimeoutError::Timeout) => {
                            let idle_ns =
                                now_ns().saturating_sub(worker_last_used.load(Ordering::Relaxed));
                            // Idle housekeeping: release the session's KV
                            // cache (hundreds of MB at n_ctx 4096) when no
                            // dictation has needed it for a while.
                            if (session.is_some()
                                || command_session.is_some()
                                || cleanup_session.is_some())
                                && idle_ns > idle.session_idle_ns
                            {
                                session = None;
                                command_session = None;
                                cleanup_session = None;
                                crate::llm::diaglog::log(
                                    "runner: idle — released session KV cache",
                                );
                            }
                            // Second tier: give the model weights back too.
                            // `engine` is owned by this thread and borrowed by
                            // the sessions above, so ending the thread is the
                            // only way to drop it — the runner is marked parked
                            // first, which makes the keyed slot lookups in
                            // `commands::llm` treat it as absent and reload
                            // through the existing `ensure_*` machinery.
                            //
                            // Claiming the busy slot is what makes the handoff
                            // race-free: a `submit` that already claimed it
                            // makes this compare_exchange fail, so the worker
                            // stays alive and serves that request instead of
                            // parking out from under it.  Parked is published
                            // BEFORE the slot is released, so a `submit` that
                            // arrives afterwards is rejected up front rather
                            // than enqueued onto a dying worker.
                            if idle_ns > idle.weights_idle_ns
                                && worker_busy
                                    .compare_exchange(
                                        false,
                                        true,
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_ok()
                            {
                                worker_parked.store(true, Ordering::Release);
                                worker_busy.store(false, Ordering::Release);
                                crate::llm::diaglog::log(
                                    "runner: idle — parked worker and released model weights",
                                );
                                break;
                            }
                            continue;
                        }
                        Err(RecvTimeoutError::Disconnected) => break,
                    };

                    if worker_shutdown.load(Ordering::Acquire)
                        || matches!(&req, LlmRequest::Shutdown)
                    {
                        worker_busy.store(false, Ordering::Release);
                        break;
                    }

                    // Held only for requests that actually acquired the busy
                    // slot in `submit`. `Prewarm` bypasses `submit` (it never
                    // sets `busy`), so it must NOT create a guard — otherwise
                    // its drop at end-of-iteration would clear a *concurrent*
                    // extraction's busy flag and defeat single-flight
                    // backpressure (a second request could then queue behind
                    // the in-flight native decode).
                    let _busy_reset = match &req {
                        LlmRequest::Prewarm | LlmRequest::Shutdown => None,
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

                    // Self-heal: run each request's work under `catch_unwind`
                    // so a Rust-side panic (a binding assertion, a postprocess
                    // bug, a bad decode) unwinds into an error for the waiting
                    // caller instead of tearing down the worker thread.  Once
                    // the thread dies the channel disconnects and every future
                    // `submit` returns "LLM worker has stopped" forever.
                    match req {
                        LlmRequest::Extract {
                            text,
                            screen_tokens,
                            source_app,
                            control,
                            reply_tx,
                        } => {
                            let outcome =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    // Rebuild the session if it was idle-dropped,
                                    // profile-switched, or failed at init.  The
                                    // request then pays one full warm-up.
                                    if session.is_none() {
                                        session = Some(
                                            engine.new_session_for_controlled(profile, &control)?,
                                        );
                                    }
                                    match session.as_mut() {
                                        Some(s) => s
                                            .generate_raw_controlled(
                                                &text,
                                                &screen_tokens,
                                                source_app.as_deref(),
                                                &control,
                                            )
                                            .and_then(|raw| (profile.postprocess)(&raw, &text)),
                                        None => {
                                            Err(AppError::Llm("LLM session unavailable".into()))
                                        }
                                    }
                                }));
                            // Release the busy slot BEFORE delivering the
                            // reply: the native decode is done, and a caller
                            // that observes the result must never race a
                            // still-set busy flag (spurious rejections).
                            drop(_busy_reset);
                            let result = outcome.unwrap_or_else(|_| {
                                // A panic leaves llama.cpp state suspect — drop
                                // the session so the next request rebuilds it
                                // fresh, and keep the worker alive.
                                session = None;
                                crate::llm::diaglog::log(
                                    "runner: extraction panicked — session reset, worker alive",
                                );
                                Err(AppError::Llm("LLM worker recovered from a panic".into()))
                            });
                            // Receiver may have been dropped by a timeout — ignore.
                            let _ = reply_tx.send(result);
                        }
                        LlmRequest::Classify {
                            utterance,
                            control,
                            reply_tx,
                        } => {
                            let outcome =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if command_session.is_none() {
                                        command_session =
                                            Some(engine.new_command_session_controlled(&control)?);
                                    }
                                    match command_session.as_mut() {
                                        Some(s) => {
                                            s.classify_command_controlled(&utterance, &control)
                                        }
                                        None => Err(AppError::Llm(
                                            "LLM command session unavailable".into(),
                                        )),
                                    }
                                }));
                            drop(_busy_reset);
                            let result = outcome.unwrap_or_else(|_| {
                                command_session = None;
                                crate::llm::diaglog::log(
                                    "runner: classify panicked — worker alive",
                                );
                                Err(AppError::Llm("LLM worker recovered from a panic".into()))
                            });
                            let _ = reply_tx.send(result);
                        }
                        LlmRequest::Cleanup {
                            text,
                            styling,
                            control,
                            reply_tx,
                        } => {
                            let outcome =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    if cleanup_session.is_none() {
                                        cleanup_session =
                                            Some(engine.new_cleanup_session_controlled(&control)?);
                                    }
                                    match cleanup_session.as_mut() {
                                        Some(s) => s.cleanup_controlled(&text, &styling, &control),
                                        None => Err(AppError::Llm(
                                            "LLM cleanup session unavailable".into(),
                                        )),
                                    }
                                }));
                            drop(_busy_reset);
                            let result = outcome.unwrap_or_else(|_| {
                                cleanup_session = None;
                                crate::llm::diaglog::log("runner: cleanup panicked — worker alive");
                                Err(AppError::Llm("LLM worker recovered from a panic".into()))
                            });
                            let _ = reply_tx.send(result);
                        }
                        LlmRequest::Prewarm => {
                            let control =
                                InferenceControl::shutdown_only(Arc::clone(&worker_shutdown));
                            match role {
                                RunnerRole::Structured if session.is_none() => {
                                    session = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            engine
                                                .new_session_for_controlled(profile, &control)
                                                .ok()
                                        }),
                                    )
                                    .unwrap_or_else(|_| {
                                        crate::llm::diaglog::log(
                                            "runner: prewarm panicked — worker alive",
                                        );
                                        None
                                    });
                                    crate::llm::diaglog::log(&format!(
                                        "runner: prewarmed session ({})",
                                        profile.id
                                    ));
                                }
                                RunnerRole::Cleanup if cleanup_session.is_none() => {
                                    cleanup_session = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| {
                                            engine.new_cleanup_session_controlled(&control).ok()
                                        }),
                                    )
                                    .unwrap_or_else(|_| {
                                        crate::llm::diaglog::log(
                                            "runner: cleanup prewarm panicked — worker alive",
                                        );
                                        None
                                    });
                                    crate::llm::diaglog::log("runner: prewarmed cleanup session");
                                }
                                _ => {}
                            }
                        }
                        LlmRequest::Shutdown => break,
                    }
                    worker_last_used.store(now_ns(), Ordering::Relaxed);
                }
            })
            .map_err(|e| AppError::Llm(format!("spawn LLM worker failed: {e}")))?;

        match startup_rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = join.join();
                return Err(AppError::Llm(format!(
                    "LLM worker startup warm failed: {e}"
                )));
            }
            Err(_) => {
                let _ = join.join();
                return Err(AppError::Llm(
                    "LLM worker stopped before startup warm completed".into(),
                ));
            }
        }

        Ok(Self {
            tx,
            busy,
            parked,
            last_used_ns,
            desired_profile,
            shutdown,
            worker: WorkerHandle {
                join: Mutex::new(Some(join)),
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
        control: InferenceControl,
        op: &str,
    ) -> AppResult<T> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(AppError::Llm("LLM worker is shutting down".into()));
        }
        if self.parked.load(Ordering::Acquire) {
            return Err(AppError::Llm(
                "LLM model was released after a long idle — reload needed".into(),
            ));
        }
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
            Err(_) => {
                control.cancel();
                Err(AppError::Llm(format!(
                    "LLM {op} timed out after {timeout:?}"
                )))
            }
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
        let control = InferenceControl::with_timeout(timeout, Arc::clone(&self.shutdown));
        let req = LlmRequest::Extract {
            text,
            screen_tokens,
            source_app,
            control: control.clone(),
            reply_tx,
        };
        self.submit(req, reply_rx, timeout, control, "extraction")
            .await
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
        let control = InferenceControl::with_timeout(timeout, Arc::clone(&self.shutdown));
        let req = LlmRequest::Classify {
            utterance,
            control: control.clone(),
            reply_tx,
        };
        self.submit(req, reply_rx, timeout, control, "classify")
            .await
    }

    /// Rewrite one raw transcript as clean written text.  Same busy-guard +
    /// timeout discipline as extraction, so a cleanup pass can never queue
    /// behind another one — the caller degrades to the un-cleaned transcript.
    pub async fn cleanup_with_timeout(
        &self,
        text: String,
        styling: String,
        timeout: Duration,
    ) -> AppResult<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let control = InferenceControl::with_timeout(timeout, Arc::clone(&self.shutdown));
        let req = LlmRequest::Cleanup {
            text,
            styling,
            control: control.clone(),
            reply_tx,
        };
        self.submit(req, reply_rx, timeout, control, "cleanup")
            .await
    }

    /// Fire-and-forget: rebuild the warmed extraction session if it was
    /// idle-dropped.  Silently ignored when the worker is busy or the queue
    /// is full — this is an opportunistic head start, never load-bearing.
    /// Does not take the busy slot: an extraction submitted while the
    /// prewarm runs simply queues behind it and then benefits from the
    /// freshly warmed session.
    pub fn prewarm(&self) {
        if self.shutdown.load(Ordering::Acquire) || self.parked.load(Ordering::Acquire) {
            return;
        }
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

    /// True once the worker has released the model weights after a long idle.
    ///
    /// A parked runner can never serve another request: callers must treat it
    /// as "no model loaded" and go through `ensure_runner_loaded_at_epoch` /
    /// `ensure_cleanup_runner_loaded` to get a fresh one.
    pub fn is_parked(&self) -> bool {
        self.parked.load(Ordering::Acquire)
    }

    /// Stop accepting work, cooperatively cancel any active decode, and join
    /// the worker. Idempotent so activation can release native model memory
    /// before loading a replacement even while other `Arc` handles exist.
    pub fn shutdown_and_join(&self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.tx.try_send(LlmRequest::Shutdown);
        let join = self
            .worker
            .join
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(join) = join {
            if join.thread().id() != thread::current().id() {
                let _ = join.join();
            }
        }
        self.busy.store(false, Ordering::Release);
    }
}

impl Drop for LlmRunner {
    fn drop(&mut self) {
        self.shutdown_and_join();
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

    /// Engine whose first extraction panics, then succeeds — exercises the
    /// worker's per-request `catch_unwind` self-heal.
    struct PanicOnceEngine {
        panicked: Arc<AtomicBool>,
    }

    impl LlmEngine for PanicOnceEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            if !self.panicked.swap(true, Ordering::SeqCst) {
                panic!("simulated decode panic");
            }
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn worker_survives_a_panicking_request() {
        let runner = LlmRunner::spawn(
            PanicOnceEngine {
                panicked: Arc::new(AtomicBool::new(false)),
            },
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .unwrap();

        // First extraction panics inside the worker; it must come back as an
        // error, not a dropped reply or a dead worker.
        let first = runner
            .extract_with_timeout("first".into(), Duration::from_secs(2))
            .await;
        assert!(first.is_err(), "panicking request should surface an error");

        // The worker must still be alive: a second request succeeds and the
        // busy slot was released after the panic.
        assert!(!runner.is_busy());
        let second = runner
            .extract_with_timeout("second".into(), Duration::from_secs(2))
            .await
            .expect("worker should survive the earlier panic");
        assert_eq!(second.slots["goal"], "second");
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

        // Spawn's ready handshake guarantees the retained session is already
        // built before the runner can be installed or reported ready.
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

    struct FailingStartupEngine;

    impl LlmEngine for FailingStartupEngine {
        fn extract_slots(&self, _user_text: &str) -> AppResult<SlotExtraction> {
            unreachable!()
        }

        fn new_session_for(
            &self,
            _profile: &'static Profile,
        ) -> AppResult<Box<dyn crate::llm::engine::LlmSession + '_>> {
            Err(AppError::Llm("simulated warm failure".into()))
        }
    }

    #[test]
    fn startup_handshake_rejects_runner_without_retained_session() {
        let error = LlmRunner::spawn(
            FailingStartupEngine,
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .err()
        .expect("startup should fail");
        assert!(error.to_string().contains("startup warm failed"));
    }

    struct CountingCommandEngine {
        command_sessions: Arc<std::sync::atomic::AtomicUsize>,
        command_calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct CountingCommandSession {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl crate::llm::engine::LlmCommandSession for CountingCommandSession {
        fn classify_command(&mut self, _utterance: &str) -> AppResult<Vec<CommandIntent>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    impl LlmEngine for CountingCommandEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }

        fn new_command_session(
            &self,
        ) -> AppResult<Box<dyn crate::llm::engine::LlmCommandSession + '_>> {
            self.command_sessions.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(CountingCommandSession {
                calls: Arc::clone(&self.command_calls),
            }))
        }
    }

    #[tokio::test]
    async fn command_classification_reuses_one_lazy_session() {
        let command_sessions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let command_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runner = LlmRunner::spawn(
            CountingCommandEngine {
                command_sessions: Arc::clone(&command_sessions),
                command_calls: Arc::clone(&command_calls),
            },
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .unwrap();

        assert_eq!(command_sessions.load(Ordering::SeqCst), 0);
        runner
            .classify_command_with_timeout("first".into(), Duration::from_secs(1))
            .await
            .unwrap();
        runner
            .classify_command_with_timeout("second".into(), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(command_sessions.load(Ordering::SeqCst), 1);
        assert_eq!(command_calls.load(Ordering::SeqCst), 2);
    }

    struct CleanupEngine {
        sessions: Arc<std::sync::atomic::AtomicUsize>,
    }

    struct CleanupSession {
        calls: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl crate::llm::engine::LlmCleanupSession for CleanupSession {
        fn cleanup(&mut self, transcript: &str, styling: &str) -> AppResult<String> {
            self.calls
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((transcript.to_string(), styling.to_string()));
            Ok(format!("cleaned:{transcript}"))
        }
    }

    impl LlmEngine for CleanupEngine {
        fn extract_slots(&self, _user_text: &str) -> AppResult<SlotExtraction> {
            unreachable!("cleanup runner never extracts")
        }

        fn new_session_for(
            &self,
            _profile: &'static Profile,
        ) -> AppResult<Box<dyn crate::llm::engine::LlmSession + '_>> {
            panic!("cleanup runner must not warm a structured session");
        }

        fn new_cleanup_session(&self) -> AppResult<Box<dyn LlmCleanupSession + '_>> {
            self.sessions.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(CleanupSession {
                calls: Arc::new(Mutex::new(Vec::new())),
            }))
        }
    }

    #[tokio::test]
    async fn cleanup_runner_warms_only_the_cleanup_session_and_reuses_it() {
        let sessions = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let runner = LlmRunner::spawn_cleanup(CleanupEngine {
            sessions: Arc::clone(&sessions),
        })
        .unwrap();

        // Startup warms exactly one cleanup session (and no structured one —
        // `new_session_for` would panic).
        assert_eq!(sessions.load(Ordering::SeqCst), 1);

        let out = runner
            .cleanup_with_timeout(
                "um hello".into(),
                "semi-formal".into(),
                Duration::from_secs(1),
            )
            .await
            .unwrap();
        assert_eq!(out, "cleaned:um hello");

        // A second pass reuses the same warmed session.
        runner
            .cleanup_with_timeout("again".into(), "formal".into(), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(sessions.load(Ordering::SeqCst), 1);
        assert!(!runner.is_busy());
    }

    struct CooperativeEngine;
    struct CooperativeSession;

    impl LlmEngine for CooperativeEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }

        fn new_session_for(
            &self,
            _profile: &'static Profile,
        ) -> AppResult<Box<dyn crate::llm::engine::LlmSession + '_>> {
            Ok(Box::new(CooperativeSession))
        }
    }

    impl crate::llm::engine::LlmSession for CooperativeSession {
        fn generate_raw(
            &mut self,
            user_text: &str,
            _screen_tokens: &[String],
            _source_app: Option<&str>,
        ) -> AppResult<String> {
            Ok(format!(r#"{{"goal":"{user_text}"}}"#))
        }

        fn generate_raw_controlled(
            &mut self,
            user_text: &str,
            _screen_tokens: &[String],
            _source_app: Option<&str>,
            control: &InferenceControl,
        ) -> AppResult<String> {
            if user_text != "fast" {
                loop {
                    control.check()?;
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            self.generate_raw(user_text, &[], None)
        }
    }

    #[tokio::test]
    async fn timeout_cooperatively_cancels_and_releases_busy_slot() {
        let runner = LlmRunner::spawn(
            CooperativeEngine,
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .unwrap();

        let timed_out = runner
            .extract_with_timeout("slow".into(), Duration::from_millis(20))
            .await;
        let timeout_message = timed_out.unwrap_err().to_string();
        assert!(
            timeout_message.contains("timed out") || timeout_message.contains("deadline"),
            "unexpected timeout error: {timeout_message}"
        );
        for _ in 0..100 {
            if !runner.is_busy() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(!runner.is_busy());
        let output = runner
            .extract_with_timeout("fast".into(), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(output.slots["goal"], "fast");
    }

    struct DropFlagEngine(Arc<AtomicBool>);

    impl Drop for DropFlagEngine {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    impl LlmEngine for DropFlagEngine {
        fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
            Ok(SlotExtraction {
                goal: user_text.to_string(),
                ..Default::default()
            })
        }
    }

    #[test]
    fn shutdown_joins_worker_and_drops_engine() {
        let dropped = Arc::new(AtomicBool::new(false));
        let runner = LlmRunner::spawn(
            DropFlagEngine(Arc::clone(&dropped)),
            profiles::get(profiles::DEFAULT_PROFILE_ID),
        )
        .unwrap();
        runner.shutdown_and_join();
        assert!(dropped.load(Ordering::SeqCst));
    }

    /// Compressed schedule so the two idle tiers are reachable in a test: the
    /// real thresholds are 5 and 45 minutes on a 60s tick.  `tick` still gates
    /// how soon the first housekeeping pass can run.
    fn fast_idle_policy(tick: Duration) -> IdlePolicy {
        IdlePolicy {
            tick,
            session_idle_ns: 0,
            weights_idle_ns: 0,
        }
    }

    async fn wait_until_parked(runner: &LlmRunner) {
        for _ in 0..400 {
            if runner.is_parked() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("worker never parked");
    }

    /// Tier two of the idle schedule: past the weights threshold the worker
    /// releases the engine (and with it the model weights), marks itself
    /// parked, and ends — with no extra polling thread, purely on the existing
    /// recv-timeout tick.
    #[tokio::test]
    async fn idle_past_the_weights_threshold_frees_the_engine_and_signals_reload() {
        let dropped = Arc::new(AtomicBool::new(false));
        let runner = LlmRunner::spawn_with_idle_policy(
            DropFlagEngine(Arc::clone(&dropped)),
            profiles::get(profiles::DEFAULT_PROFILE_ID),
            RunnerRole::Structured,
            fast_idle_policy(Duration::from_millis(5)),
        )
        .unwrap();

        wait_until_parked(&runner).await;
        assert!(
            dropped.load(Ordering::SeqCst),
            "parking must drop the engine and free the model weights"
        );

        // A caller holding an Arc from before the park is rejected up front
        // rather than enqueued onto a worker that will never answer.
        let error = runner
            .extract_with_timeout("after park".into(), Duration::from_secs(1))
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("reload needed"), "unexpected error: {error}");
        // Prewarm is a no-op: a parked worker must never be nudged back to life.
        runner.prewarm();
        assert!(runner.is_parked());

        // The reload path calls this while holding LLM_LOAD_LOCK before it
        // installs the replacement, so on an already-exited worker it must
        // return instead of blocking on a join that can never complete.
        runner.shutdown_and_join();
        assert!(!runner.is_busy());
    }

    /// Both roles share the worker loop, so the cleanup runner parks on the
    /// same schedule — it is the one most likely to sit idle for an hour.
    #[tokio::test]
    async fn cleanup_runner_also_parks_and_frees_its_engine_when_idle() {
        let dropped = Arc::new(AtomicBool::new(false));
        let runner = LlmRunner::spawn_with_idle_policy(
            DropFlagEngine(Arc::clone(&dropped)),
            profiles::get(profiles::DEFAULT_PROFILE_ID),
            RunnerRole::Cleanup,
            fast_idle_policy(Duration::from_millis(5)),
        )
        .unwrap();

        wait_until_parked(&runner).await;
        assert!(dropped.load(Ordering::SeqCst));
        let error = runner
            .cleanup_with_timeout(
                "um hello".into(),
                "semi-formal".into(),
                Duration::from_secs(1),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("reload needed"), "unexpected error: {error}");
        runner.shutdown_and_join();
    }

    /// Work already queued when the idle threshold is reached must still be
    /// served: the park claims the busy slot first, so a request that got there
    /// first keeps the worker alive rather than losing its reply to the park.
    #[tokio::test]
    async fn a_pending_request_is_served_rather_than_lost_to_the_idle_park() {
        let runner = LlmRunner::spawn_with_idle_policy(
            SlowEngine {
                delay: Duration::from_millis(50),
            },
            profiles::get(profiles::DEFAULT_PROFILE_ID),
            RunnerRole::Structured,
            // Long enough that the request below is submitted before the first
            // housekeeping tick can fire.
            fast_idle_policy(Duration::from_secs(2)),
        )
        .unwrap();

        let output = runner
            .extract_with_timeout("in flight".into(), Duration::from_secs(5))
            .await
            .expect("a pending request must not be lost to the idle park");
        assert_eq!(output.slots["goal"], "in flight");
        assert!(!runner.is_parked());
    }
}
