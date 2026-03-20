use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedText {
    pub original: String,
    pub processed: String,
    pub corrections: Vec<Correction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub original: String,
    pub replacement: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorConfig {
    pub auto_capitalize: bool,
    pub auto_punctuate: bool,
    pub apply_dictionary: bool,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            auto_capitalize: true,
            auto_punctuate: true,
            apply_dictionary: true,
        }
    }
}
