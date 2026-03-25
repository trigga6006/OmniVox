#[cfg(test)]
mod tests {
    use crate::cleanup::types::*;

    #[test]
    fn cleanup_mode_roundtrip() {
        let modes = ["clean", "technical_rectify", "agent_optimize", "claude_code_optimize"];
        for mode_str in &modes {
            let mode = CleanupMode::from_str(mode_str);
            assert_eq!(mode.as_str(), *mode_str, "Roundtrip failed for {mode_str}");
        }
    }

    #[test]
    fn cleanup_mode_unknown_defaults_to_agent_optimize() {
        let mode = CleanupMode::from_str("nonexistent");
        assert_eq!(mode, CleanupMode::AgentOptimize);
    }

    #[test]
    fn rewrite_strength_roundtrip() {
        let strengths = ["conservative", "balanced", "aggressive"];
        for s in &strengths {
            let strength = RewriteStrength::from_str(s);
            assert_eq!(strength.as_str(), *s, "Roundtrip failed for {s}");
        }
    }

    #[test]
    fn rewrite_strength_unknown_defaults_to_balanced() {
        let strength = RewriteStrength::from_str("nonexistent");
        assert_eq!(strength, RewriteStrength::Balanced);
    }

    #[test]
    fn default_cleanup_settings() {
        let settings = CleanupSettings::default();
        assert!(!settings.enabled);
        assert_eq!(settings.model_id, "qwen3_5_4b");
        assert_eq!(settings.mode, CleanupMode::AgentOptimize);
        assert_eq!(settings.strength, RewriteStrength::Balanced);
        assert!(!settings.use_cleaned_by_default);
    }

    #[test]
    fn cleanup_model_id_display() {
        let id = CleanupModelId("test_model".to_string());
        assert_eq!(id.to_string(), "test_model");
        assert_eq!(id.as_str(), "test_model");
    }

    #[test]
    fn cleanup_mode_labels() {
        assert_eq!(CleanupMode::Clean.label(), "Clean");
        assert_eq!(CleanupMode::TechnicalRectify.label(), "Technical Rectify");
        assert_eq!(CleanupMode::AgentOptimize.label(), "Agent Optimize");
        assert_eq!(CleanupMode::ClaudeCodeOptimize.label(), "Claude Code Optimize");
    }

    #[test]
    fn rewrite_strength_labels() {
        assert_eq!(RewriteStrength::Conservative.label(), "Conservative");
        assert_eq!(RewriteStrength::Balanced.label(), "Balanced");
        assert_eq!(RewriteStrength::Aggressive.label(), "Aggressive");
    }

    #[test]
    fn cleanup_status_serialization() {
        let status = CleanupStatus::Running;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"running\"");

        let deserialized: CleanupStatus = serde_json::from_str("\"success\"").unwrap();
        assert_eq!(deserialized, CleanupStatus::Success);
    }

    #[test]
    fn cleanup_result_serialization() {
        let result = CleanupResult {
            raw_text: "test raw".to_string(),
            cleaned_text: "test cleaned".to_string(),
            model_id: "qwen3_5_4b".to_string(),
            mode: CleanupMode::AgentOptimize,
            strength: RewriteStrength::Balanced,
            duration_ms: 1234,
            status: CleanupStatus::Success,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"raw_text\":\"test raw\""));
        assert!(json.contains("\"cleaned_text\":\"test cleaned\""));
        assert!(json.contains("\"agent_optimize\""));
        assert!(json.contains("\"balanced\""));
    }
}
