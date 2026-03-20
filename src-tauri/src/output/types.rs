use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputMode {
    Clipboard,
    TypeSimulation,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub mode: OutputMode,
    pub typing_delay_ms: u32,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            mode: OutputMode::Clipboard,
            typing_delay_ms: 10,
        }
    }
}
