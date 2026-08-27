//! Exact WebView caller authorization for custom Tauri commands.
//!
//! Tauri capability files constrain core APIs, but application commands in the
//! invoke handler still need server-side authorization. Policies are kept
//! small and explicit so adding a window never silently grants it access.

#[derive(Debug, Clone, Copy)]
pub(super) enum WindowPolicy {
    Main,
    Overlay,
    MainOverlay,
    MainScratchpad,
    AllAppWindows,
}

pub(super) fn require_caller(
    caller: &tauri::WebviewWindow,
    policy: WindowPolicy,
) -> Result<(), String> {
    require_label(caller.label(), policy)
}

fn require_label(label: &str, policy: WindowPolicy) -> Result<(), String> {
    let allowed = match policy {
        WindowPolicy::Main => label == "main",
        WindowPolicy::Overlay => label == "overlay",
        WindowPolicy::MainOverlay => matches!(label, "main" | "overlay"),
        WindowPolicy::MainScratchpad => matches!(label, "main" | "scratchpad"),
        WindowPolicy::AllAppWindows => matches!(label, "main" | "overlay" | "scratchpad"),
    };
    if allowed {
        Ok(())
    } else {
        Err(format!(
            "This command is not available to the '{label}' window"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policies_allow_only_exact_documented_labels() {
        let cases = [
            (WindowPolicy::Main, vec!["main"]),
            (WindowPolicy::Overlay, vec!["overlay"]),
            (WindowPolicy::MainOverlay, vec!["main", "overlay"]),
            (WindowPolicy::MainScratchpad, vec!["main", "scratchpad"]),
            (
                WindowPolicy::AllAppWindows,
                vec!["main", "overlay", "scratchpad"],
            ),
        ];
        for (policy, allowed) in cases {
            for label in [
                "main",
                "overlay",
                "scratchpad",
                "main-extra",
                "overlay-extra",
                "scratchpad-extra",
                "unknown",
            ] {
                assert_eq!(
                    require_label(label, policy).is_ok(),
                    allowed.contains(&label),
                    "unexpected policy result for {policy:?} / {label:?}"
                );
            }
        }
    }
}
