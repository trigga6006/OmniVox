use tauri::State;
use crate::state::AppState;
use crate::storage::types::{DictionaryEntry, Snippet};

/// Reload dictionary entries from DB into the in-memory ProcessorChain
/// so replacements take effect immediately.
fn sync_dictionary(state: &AppState) {
    if let Ok(entries) = crate::storage::dictionary::list_entries(&state.db) {
        if let Ok(mut processor) = state.processor.lock() {
            processor.set_dictionary(entries);
        }
    }
}

/// Reload snippets from DB into the in-memory ProcessorChain
/// so trigger-word expansions take effect immediately.
fn sync_snippets(state: &AppState) {
    if let Ok(snippets) = crate::storage::snippets::list_snippets(&state.db) {
        if let Ok(mut processor) = state.processor.lock() {
            processor.set_snippets(snippets);
        }
    }
}

#[tauri::command]
pub async fn add_dictionary_entry(
    phrase: String,
    replacement: String,
    state: State<'_, AppState>,
) -> Result<DictionaryEntry, String> {
    let entry = crate::storage::dictionary::add_entry(&state.db, &phrase, &replacement)
        .map_err(|e| e.to_string())?;
    sync_dictionary(&state);
    Ok(entry)
}

#[tauri::command]
pub async fn update_dictionary_entry(
    id: String,
    phrase: String,
    replacement: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::dictionary::update_entry(&state.db, &id, &phrase, &replacement)
        .map_err(|e| e.to_string())?;
    sync_dictionary(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_dictionary_entry(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::dictionary::delete_entry(&state.db, &id).map_err(|e| e.to_string())?;
    sync_dictionary(&state);
    Ok(())
}

#[tauri::command]
pub async fn list_dictionary_entries(
    state: State<'_, AppState>,
) -> Result<Vec<DictionaryEntry>, String> {
    crate::storage::dictionary::list_entries(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_snippet(
    trigger: String,
    content: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> Result<Snippet, String> {
    let snippet = crate::storage::snippets::add_snippet(
        &state.db,
        &trigger,
        &content,
        description.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    sync_snippets(&state);
    Ok(snippet)
}

#[tauri::command]
pub async fn update_snippet(
    id: String,
    trigger: String,
    content: String,
    description: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::snippets::update_snippet(
        &state.db,
        &id,
        &trigger,
        &content,
        description.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    sync_snippets(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_snippet(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::snippets::delete_snippet(&state.db, &id).map_err(|e| e.to_string())?;
    sync_snippets(&state);
    Ok(())
}

#[tauri::command]
pub async fn list_snippets(
    state: State<'_, AppState>,
) -> Result<Vec<Snippet>, String> {
    crate::storage::snippets::list_snippets(&state.db).map_err(|e| e.to_string())
}
