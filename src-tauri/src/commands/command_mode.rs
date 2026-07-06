//! Tauri commands for Command Mode confirmation.
//!
//! The capture itself is driven by the Right Ctrl hotkey (see `hotkey.rs` →
//! `pipeline::start_command_inner` / `stop_and_run_command`).  These two
//! commands let the overlay accept or dismiss a low-confidence app match that
//! the pipeline parked via a `command-confirm` event.

use tauri::State;

use crate::state::AppState;

/// Execute the command currently awaiting confirmation.  `edited_text`
/// carries the pill textarea's content when the user revised a message
/// before accepting.
#[tauri::command]
pub async fn confirm_command(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    edited_text: Option<String>,
) -> Result<(), String> {
    crate::pipeline::confirm_pending_command(&app_handle, &state, edited_text).await;
    Ok(())
}

/// Discard the command currently awaiting confirmation.
#[tauri::command]
pub async fn cancel_command(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::pipeline::cancel_pending_command(&app_handle, &state);
    Ok(())
}

/// Dry-run an utterance through the command brain (matcher → LLM) without
/// executing it — backs the "Test command" box on the Models → Command tab.
#[tauri::command]
pub async fn test_command(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    utterance: String,
) -> Result<crate::pipeline::CommandTestResult, String> {
    Ok(crate::pipeline::test_command(&app_handle, &state, &utterance).await)
}
