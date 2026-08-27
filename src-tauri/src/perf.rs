use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::models::types::ModelFamily;

const TRACE_CAPACITY: usize = 100;

#[derive(Clone, Copy)]
struct DeliveryObservation {
    generation: u64,
    at: Instant,
    kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct PipelineTrace {
    pub generation: u64,
    pub mode: &'static str,
    pub model: Option<String>,
    pub backend: Option<&'static str>,
    /// Inference engine that produced this trace. RTF-based tiering is
    /// calibrated against whisper.cpp decode cost, so consumers must filter
    /// on this before treating `asr_started`/`asr_done` as comparable across
    /// traces. Defaults to `Whisper` for traces that never call
    /// `describe_model` (aborted/incomplete captures, command traces).
    pub family: ModelFamily,
    pub audio_duration_ms: u64,
    /// Backend capture claim to successful microphone start. OS hotkey
    /// dispatch before the claim is outside this measurement.
    pub claim_to_mic_live_ms: Option<u64>,
    pub stop_received: Option<u64>,
    pub audio_stopped: Option<u64>,
    pub preview_drained: Option<u64>,
    pub preprocess_done: Option<u64>,
    pub asr_started: Option<u64>,
    pub asr_done: Option<u64>,
    pub llm_started: Option<u64>,
    pub llm_done: Option<u64>,
    pub output_started: Option<u64>,
    pub output_done: Option<u64>,
    /// Stop request to the first successful user-visible delivery boundary.
    /// For WebView-owned destinations this means the backend event was emitted;
    /// it does not claim that the browser painted the next frame.
    pub stop_to_visible_delivery_ms: Option<u64>,
    pub visible_delivery_kind: Option<&'static str>,
    pub completed: Option<u64>,
    pub outcome: &'static str,
}

pub struct PipelineTraceGuard {
    started: Instant,
    trace: Option<PipelineTrace>,
}

impl PipelineTraceGuard {
    pub fn new(generation: u64, mode: &'static str) -> Self {
        Self::new_with_capture_timing(generation, mode, None, None, None)
    }

    pub fn new_with_capture_timing(
        generation: u64,
        mode: &'static str,
        claimed_at: Option<Instant>,
        mic_live_at: Option<Instant>,
        stop_requested_at: Option<Instant>,
    ) -> Self {
        let started = stop_requested_at.unwrap_or_else(Instant::now);
        let claim_to_mic_live_ms = claimed_at
            .zip(mic_live_at)
            .and_then(|(claimed, live)| live.checked_duration_since(claimed))
            .map(duration_ms);
        Self {
            started,
            trace: Some(PipelineTrace {
                generation,
                mode,
                model: None,
                backend: None,
                family: ModelFamily::default(),
                audio_duration_ms: 0,
                claim_to_mic_live_ms,
                stop_received: Some(0),
                audio_stopped: None,
                preview_drained: None,
                preprocess_done: None,
                asr_started: None,
                asr_done: None,
                llm_started: None,
                llm_done: None,
                output_started: None,
                output_done: None,
                stop_to_visible_delivery_ms: None,
                visible_delivery_kind: None,
                completed: None,
                outcome: "incomplete",
            }),
        }
    }

    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().min(u64::MAX as u128) as u64
    }

    pub fn mark(&mut self, stage: &'static str) {
        let at = self.elapsed_ms();
        let Some(trace) = self.trace.as_mut() else {
            return;
        };
        match stage {
            "audio_stopped" => trace.audio_stopped = Some(at),
            "preview_drained" => trace.preview_drained = Some(at),
            "preprocess_done" => trace.preprocess_done = Some(at),
            "asr_started" => trace.asr_started = Some(at),
            "asr_done" => trace.asr_done = Some(at),
            "llm_started" => trace.llm_started = Some(at),
            "llm_done" => trace.llm_done = Some(at),
            "output_started" => trace.output_started = Some(at),
            "output_done" => trace.output_done = Some(at),
            _ => {}
        }
    }

    pub fn describe_audio(&mut self, samples: usize) {
        if let Some(trace) = self.trace.as_mut() {
            trace.audio_duration_ms =
                ((samples as u128 * 1000) / 16_000).min(u64::MAX as u128) as u64;
        }
    }

    pub fn describe_model(
        &mut self,
        model: impl Into<String>,
        uses_gpu: bool,
        family: ModelFamily,
    ) {
        if let Some(trace) = self.trace.as_mut() {
            trace.model = Some(model.into());
            trace.backend = Some(if uses_gpu { "gpu" } else { "cpu" });
            trace.family = family;
        }
    }

    /// Record the first successful delivery boundary. Later UI notifications
    /// must not overwrite the latency of an earlier OS paste or in-app insert.
    pub fn mark_visible_delivery(&mut self, kind: &'static str) {
        let at = self.elapsed_ms();
        if let Some(trace) = self.trace.as_mut() {
            if trace.stop_to_visible_delivery_ms.is_none() {
                trace.stop_to_visible_delivery_ms = Some(at);
                trace.visible_delivery_kind = Some(kind);
            }
        }
    }

    pub fn finish(mut self, outcome: &'static str) {
        self.record(outcome);
    }

    fn record(&mut self, outcome: &'static str) {
        let completed = self.elapsed_ms();
        if let Some(mut trace) = self.trace.take() {
            if let Some(observation) = take_visible_delivery(trace.generation) {
                if trace.stop_to_visible_delivery_ms.is_none() {
                    trace.stop_to_visible_delivery_ms = observation
                        .at
                        .checked_duration_since(self.started)
                        .map(duration_ms);
                    trace.visible_delivery_kind =
                        trace.stop_to_visible_delivery_ms.map(|_| observation.kind);
                }
            }
            trace.completed = Some(completed);
            trace.outcome = outcome;
            let mut traces = storage().lock().unwrap_or_else(|p| p.into_inner());
            if traces.len() == TRACE_CAPACITY {
                traces.pop_front();
            }
            traces.push_back(trace);
        }
    }
}

fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

impl Drop for PipelineTraceGuard {
    fn drop(&mut self) {
        if self.trace.is_some() {
            self.record("aborted");
        }
    }
}

fn storage() -> &'static Mutex<VecDeque<PipelineTrace>> {
    static TRACES: OnceLock<Mutex<VecDeque<PipelineTrace>>> = OnceLock::new();
    TRACES.get_or_init(|| Mutex::new(VecDeque::with_capacity(TRACE_CAPACITY)))
}

fn deliveries() -> &'static Mutex<VecDeque<DeliveryObservation>> {
    static DELIVERIES: OnceLock<Mutex<VecDeque<DeliveryObservation>>> = OnceLock::new();
    DELIVERIES.get_or_init(|| Mutex::new(VecDeque::with_capacity(TRACE_CAPACITY)))
}

/// Note a successful backend delivery from code that does not own the trace
/// guard (notably nested Command Mode executors). Only timing and route kind are
/// retained; payload content never enters telemetry.
pub fn observe_visible_delivery(generation: u64, kind: &'static str) {
    let mut observations = deliveries().lock().unwrap_or_else(|p| p.into_inner());
    if observations
        .iter()
        .any(|observation| observation.generation == generation)
    {
        return;
    }
    if observations.len() == TRACE_CAPACITY {
        observations.pop_front();
    }
    observations.push_back(DeliveryObservation {
        generation,
        at: Instant::now(),
        kind,
    });
}

fn take_visible_delivery(generation: u64) -> Option<DeliveryObservation> {
    let mut observations = deliveries().lock().unwrap_or_else(|p| p.into_inner());
    let index = observations
        .iter()
        .position(|observation| observation.generation == generation)?;
    observations.remove(index)
}

pub fn recent() -> Vec<PipelineTrace> {
    storage()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_are_monotonic_and_trace_has_no_content_field() {
        // Use a generation outside the range populated by `ring_is_bounded`.
        // Rust tests run concurrently, and sharing generation 42 allowed this
        // lookup to observe that test's intentionally incomplete trace.
        const TEST_GENERATION: u64 = u64::MAX;
        let now = Instant::now();
        let claimed = now.checked_sub(Duration::from_millis(30)).unwrap();
        let mic_live = claimed + Duration::from_millis(7);
        let stop = now.checked_sub(Duration::from_millis(10)).unwrap();
        let mut guard = PipelineTraceGuard::new_with_capture_timing(
            TEST_GENERATION,
            "dictation",
            Some(claimed),
            Some(mic_live),
            Some(stop),
        );
        guard.mark("audio_stopped");
        guard.mark("preview_drained");
        guard.mark_visible_delivery("in_app_event_emitted");
        guard.finish("ok");
        let trace = recent()
            .into_iter()
            .find(|trace| trace.generation == TEST_GENERATION)
            .expect("trace recorded");
        assert!(trace.stop_received <= trace.audio_stopped);
        assert!(trace.audio_stopped <= trace.preview_drained);
        assert_eq!(trace.claim_to_mic_live_ms, Some(7));
        assert!(trace.stop_to_visible_delivery_ms.unwrap() >= 10);
        assert_eq!(trace.visible_delivery_kind, Some("in_app_event_emitted"));
        let json = serde_json::to_value(trace).unwrap();
        assert!(json.get("text").is_none());
        assert!(json.get("transcript").is_none());
    }

    #[test]
    fn ring_is_bounded() {
        for generation in 0..(TRACE_CAPACITY as u64 + 10) {
            PipelineTraceGuard::new(generation, "command").finish("ok");
        }
        assert_eq!(recent().len(), TRACE_CAPACITY);
    }

    #[test]
    fn nested_delivery_observation_is_joined_without_content() {
        const TEST_GENERATION: u64 = u64::MAX - 1;
        let stop = Instant::now()
            .checked_sub(Duration::from_millis(3))
            .unwrap();
        let guard = PipelineTraceGuard::new_with_capture_timing(
            TEST_GENERATION,
            "command",
            None,
            None,
            Some(stop),
        );
        observe_visible_delivery(TEST_GENERATION, "command_result_event_emitted");
        guard.finish("ok");

        let trace = recent()
            .into_iter()
            .find(|trace| trace.generation == TEST_GENERATION)
            .expect("trace recorded");
        assert!(trace.stop_to_visible_delivery_ms.unwrap() >= 3);
        assert_eq!(
            trace.visible_delivery_kind,
            Some("command_result_event_emitted")
        );
        let json = serde_json::to_string(&trace).unwrap();
        assert!(!json.contains("summary"));
        assert!(!json.contains("utterance"));
    }
}
