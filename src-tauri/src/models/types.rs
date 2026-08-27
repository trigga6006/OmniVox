use serde::{Deserialize, Serialize};

/// Trusted compute backend used for a recommendation. `Unknown` is deliberate:
/// recommendations must not assume that a discrete GPU is usable merely
/// because Windows reports an adapter.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackend {
    Cpu,
    Vulkan,
    Cuda,
    Metal,
    #[default]
    Unknown,
}

/// Capability tier produced by a short, local ASR benchmark.  It describes
/// observed throughput, not the marketing specification of the machine.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum AsrCapabilityTier {
    Entry,
    Balanced,
    High,
}

/// Hardware facts consumed by the model recommender.
///
/// Callers should populate `gpu_vram_mb` only from a backend that can actually
/// allocate the ASR workload (for example Vulkan). Win32's `AdapterRAM` is a
/// 32-bit field and can under-report modern GPUs, so it must not be used as a
/// trusted value here.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct HardwareProfile {
    pub physical_cpu_cores: Option<u32>,
    pub logical_cpu_cores: Option<u32>,
    pub ram_total_mb: Option<u64>,
    pub gpu_name: Option<String>,
    pub gpu_vram_mb: Option<u64>,
    pub compute_backend: ComputeBackend,
    /// Result of a versioned local benchmark when one is available. This wins
    /// over heuristics because driver quality and thermals matter in practice.
    pub measured_asr_tier: Option<AsrCapabilityTier>,
}

impl HardwareProfile {
    pub fn from_logical_cores(logical_cpu_cores: u32) -> Self {
        Self {
            logical_cpu_cores: Some(logical_cpu_cores),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelLanguageSupport {
    English,
    Multilingual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapabilityTier {
    Entry,
    Balanced,
    High,
}

/// Which inference backend a catalog entry runs on. This is stable catalog
/// metadata, exactly like `language_support`: activating an entry selects the
/// engine implementation, so it must never be guessed from an ID or filename.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelFamily {
    /// whisper.cpp GGML model, one file on disk.
    #[default]
    Whisper,
    /// sherpa-onnx offline transducer, a directory of ONNX files plus tokens.
    Parakeet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
    pub quantization: String,
    /// Stable catalog metadata; never infer this from a display name or ID.
    pub language_support: ModelLanguageSupport,
    /// Inference backend this entry loads into. See [`ModelFamily`].
    pub family: ModelFamily,
    pub capability_tier: ModelCapabilityTier,
    /// Approximate process memory budget for model weights and decode state.
    /// This is planning metadata, not a VRAM allocation guarantee.
    pub estimated_memory_mb: u64,
    pub description: String,
    pub is_downloaded: bool,
    pub path: Option<String>,
    /// True if this model ships with the installer (no download needed).
    pub bundled: bool,
    /// True if the backend recommends this model for the user's hardware.
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f32,
    pub status: DownloadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareInfo {
    pub cpu_name: String,
    pub cpu_cores: u32,
    pub ram_total_mb: u64,
    pub gpu_name: Option<String>,
    pub gpu_vram_mb: Option<u64>,
    /// Backend used by the currently loaded Whisper engine. `unknown` means no
    /// engine has completed loading yet; it must not be presented as CPU.
    pub compute_backend: ComputeBackend,
    /// Capability inferred from recent successful dictation traces when at
    /// least three comparable samples exist.
    pub measured_asr_tier: Option<AsrCapabilityTier>,
    pub recommended_model: String,
}
