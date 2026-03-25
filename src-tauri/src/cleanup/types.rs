use serde::{Deserialize, Serialize};

/// Unique identifier for a cleanup model.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CleanupModelId(pub String);

impl CleanupModelId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CleanupModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Cleanup mode determines the rewriting strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupMode {
    /// Light cleanup: remove filler, fix grammar, preserve wording.
    Clean,
    /// Correct technical recognition errors, normalize terminology.
    TechnicalRectify,
    /// Rewrite into clearer, more actionable instructions for AI assistants.
    AgentOptimize,
    /// Bias output toward coding-agent friendly instruction format.
    ClaudeCodeOptimize,
}

impl CleanupMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Clean => "Clean",
            Self::TechnicalRectify => "Technical Rectify",
            Self::AgentOptimize => "Agent Optimize",
            Self::ClaudeCodeOptimize => "Claude Code Optimize",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "clean" => Self::Clean,
            "technical_rectify" => Self::TechnicalRectify,
            "agent_optimize" => Self::AgentOptimize,
            "claude_code_optimize" => Self::ClaudeCodeOptimize,
            _ => Self::AgentOptimize,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::TechnicalRectify => "technical_rectify",
            Self::AgentOptimize => "agent_optimize",
            Self::ClaudeCodeOptimize => "claude_code_optimize",
        }
    }
}

/// How aggressively the cleanup model rewrites the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RewriteStrength {
    /// Very light cleanup, minimal paraphrasing, preserve user phrasing.
    Conservative,
    /// Standard recommended mode: stronger cleanup, controlled paraphrasing.
    Balanced,
    /// Stronger compression and restructuring, still preserves intent.
    Aggressive,
}

impl RewriteStrength {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Conservative => "Conservative",
            Self::Balanced => "Balanced",
            Self::Aggressive => "Aggressive",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "conservative" => Self::Conservative,
            "balanced" => Self::Balanced,
            "aggressive" => Self::Aggressive,
            _ => Self::Balanced,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Conservative => "conservative",
            Self::Balanced => "balanced",
            Self::Aggressive => "aggressive",
        }
    }
}

/// User-configurable cleanup settings, persisted across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupSettings {
    pub enabled: bool,
    pub model_id: String,
    pub mode: CleanupMode,
    pub strength: RewriteStrength,
    /// Whether to use the cleaned output by default for copy/paste.
    pub use_cleaned_by_default: bool,
}

impl Default for CleanupSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            model_id: "qwen3_5_4b".to_string(),
            mode: CleanupMode::AgentOptimize,
            strength: RewriteStrength::Balanced,
            use_cleaned_by_default: false,
        }
    }
}

/// A request to the cleanup service.
#[derive(Debug, Clone)]
pub struct CleanupRequest {
    pub raw_text: String,
    pub mode: CleanupMode,
    pub strength: RewriteStrength,
    pub model_id: CleanupModelId,
}

/// The result of a cleanup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupResult {
    pub raw_text: String,
    pub cleaned_text: String,
    pub model_id: String,
    pub mode: CleanupMode,
    pub strength: RewriteStrength,
    pub duration_ms: u64,
    pub status: CleanupStatus,
}

/// Status of a cleanup operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupStatus {
    Idle,
    Queued,
    Running,
    Success,
    Failed,
    Cancelled,
}

/// Size class for a cleanup model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSizeClass {
    Small,  // ~2B params
    Medium, // ~3-4B params
}

/// Speed class for a cleanup model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelSpeedClass {
    Fast,
    Standard,
}

/// Metadata about a supported cleanup model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_class: ModelSizeClass,
    pub speed_class: ModelSpeedClass,
    pub recommended_use: String,
    pub is_default: bool,
    /// Whether the model files are present on disk.
    pub is_installed: bool,
    /// Inference endpoint URL (for local server like llama.cpp / Ollama).
    pub endpoint: Option<String>,
    /// GGUF model filename or identifier.
    pub model_file: String,
}
