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

/// Writing style controls capitalization and punctuation formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WritingStyle {
    /// Sentence-start capitals + full punctuation + auto-period at end.
    #[default]
    Formal,
    /// Sentence-start capitals + artifact cleanup, but no forced trailing period.
    Casual,
    /// Lowercase everything, minimal punctuation (keep ? and !).
    VeryCasual,
}

impl WritingStyle {
    /// Parse from the settings string value. Falls back to `Formal` for unknown values.
    pub fn from_str(s: &str) -> Self {
        match s {
            "casual" => Self::Casual,
            "very_casual" => Self::VeryCasual,
            _ => Self::Formal,
        }
    }

    /// Convert to the settings string value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Formal => "formal",
            Self::Casual => "casual",
            Self::VeryCasual => "very_casual",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessorConfig {
    pub auto_capitalize: bool,
    pub auto_punctuate: bool,
    pub apply_dictionary: bool,
    /// Remove filler words (um, uh, you know, etc.) and deduplicate repeated words.
    pub apply_filler_removal: bool,
    /// Writing style — controls capitalization and punctuation behavior.
    pub writing_style: WritingStyle,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        Self {
            auto_capitalize: true,
            auto_punctuate: true,
            apply_dictionary: true,
            apply_filler_removal: true,
            writing_style: WritingStyle::default(),
        }
    }
}
