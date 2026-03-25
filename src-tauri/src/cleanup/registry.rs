use crate::cleanup::types::{CleanupModelInfo, ModelSizeClass, ModelSpeedClass};

/// Return the list of all supported cleanup models.
/// This registry drives the settings dropdown and model selection UI.
pub fn supported_models() -> Vec<CleanupModelInfo> {
    vec![
        CleanupModelInfo {
            id: "qwen3_5_4b".to_string(),
            name: "Qwen 3.5 4B".to_string(),
            description: "Primary recommended model. Good balance of quality and speed for prompt cleanup.".to_string(),
            size_class: ModelSizeClass::Medium,
            speed_class: ModelSpeedClass::Standard,
            recommended_use: "Best overall quality for prompt cleanup and technical rectification.".to_string(),
            is_default: true,
            is_installed: false,
            endpoint: None,
            model_file: "qwen3.5-4b".to_string(),
        },
        CleanupModelInfo {
            id: "phi4_mini_instruct".to_string(),
            name: "Phi-4 Mini Instruct".to_string(),
            description: "Microsoft's compact instruct model. Strong reasoning for its size.".to_string(),
            size_class: ModelSizeClass::Medium,
            speed_class: ModelSpeedClass::Standard,
            recommended_use: "Strong instruction following and technical accuracy.".to_string(),
            is_default: false,
            is_installed: false,
            endpoint: None,
            model_file: "phi-4-mini-instruct".to_string(),
        },
        CleanupModelInfo {
            id: "smollm3_3b".to_string(),
            name: "SmolLM3 3B".to_string(),
            description: "Lightweight and fast. Good for quick cleanup tasks.".to_string(),
            size_class: ModelSizeClass::Medium,
            speed_class: ModelSpeedClass::Fast,
            recommended_use: "Fast cleanup when speed is prioritized over maximum quality.".to_string(),
            is_default: false,
            is_installed: false,
            endpoint: None,
            model_file: "smollm3-3b".to_string(),
        },
        CleanupModelInfo {
            id: "granite_3_3_2b_instruct".to_string(),
            name: "Granite 3.3 2B Instruct".to_string(),
            description: "IBM's ultra-compact instruct model. Fastest option, minimal resource usage.".to_string(),
            size_class: ModelSizeClass::Small,
            speed_class: ModelSpeedClass::Fast,
            recommended_use: "Ultra-fast fallback when resources are constrained.".to_string(),
            is_default: false,
            is_installed: false,
            endpoint: None,
            model_file: "granite-3.3-2b-instruct".to_string(),
        },
    ]
}

/// Look up a model by ID.
pub fn get_model(id: &str) -> Option<CleanupModelInfo> {
    supported_models().into_iter().find(|m| m.id == id)
}

/// Get the default model ID.
pub fn default_model_id() -> String {
    "qwen3_5_4b".to_string()
}

/// Validate that a model ID is in the registry.
pub fn is_valid_model_id(id: &str) -> bool {
    supported_models().iter().any(|m| m.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_at_least_three_models() {
        assert!(supported_models().len() >= 3);
    }

    #[test]
    fn default_model_exists_in_registry() {
        let id = default_model_id();
        assert!(is_valid_model_id(&id), "Default model '{id}' not in registry");
    }

    #[test]
    fn exactly_one_default_model() {
        let defaults: Vec<_> = supported_models().into_iter().filter(|m| m.is_default).collect();
        assert_eq!(defaults.len(), 1, "Expected exactly one default model");
    }

    #[test]
    fn get_model_returns_correct_entry() {
        let model = get_model("phi4_mini_instruct");
        assert!(model.is_some());
        assert_eq!(model.unwrap().name, "Phi-4 Mini Instruct");
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(get_model("nonexistent_model").is_none());
        assert!(!is_valid_model_id("nonexistent_model"));
    }
}
