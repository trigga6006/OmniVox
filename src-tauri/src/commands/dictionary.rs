use tauri::State;
use crate::state::AppState;
use crate::storage::types::{DictionaryEntry, Snippet, VocabularyEntry};

/// Reload both global and active-mode entries into the in-memory
/// ProcessorChain so replacements/snippets take effect immediately.
pub(crate) fn sync_processor(state: &AppState) {
    let active_mode_id = state.active_context_mode_id.lock().unwrap().clone();

    let mut entries = crate::storage::dictionary::list_entries(&state.db).unwrap_or_default();
    let mut snippets = crate::storage::snippets::list_snippets(&state.db).unwrap_or_default();

    if let Some(ref mode_id) = active_mode_id {
        entries.extend(
            crate::storage::dictionary::list_entries_for_mode(&state.db, mode_id)
                .unwrap_or_default(),
        );
        snippets.extend(
            crate::storage::snippets::list_snippets_for_mode(&state.db, mode_id)
                .unwrap_or_default(),
        );
    }

    if let Ok(mut processor) = state.processor.lock() {
        processor.set_dictionary(entries);
        processor.set_snippets(snippets);
    }
}

/// Rebuild the Whisper initial prompt from all sources (static vocabulary,
/// dictionary replacements, custom vocabulary) and hot-swap it into the
/// running engine. Takes effect on the next transcription call.
pub(crate) fn sync_whisper_prompt(state: &AppState) {
    let engine_guard = match state.engine.lock().ok() {
        Some(g) => g,
        None => return,
    };
    let engine = match engine_guard.as_ref() {
        Some(e) => e,
        None => return,
    };

    let is_multilingual = state
        .active_model_id
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|id| super::models::is_model_multilingual(id)))
        .unwrap_or(false);

    let prompt = super::models::build_whisper_vocab_prompt(state, is_multilingual);
    engine.set_initial_prompt(prompt);
}

// ── Global entry commands (Dictionary / Snippets pages) ──

#[tauri::command]
pub async fn add_dictionary_entry(
    phrase: String,
    replacement: String,
    state: State<'_, AppState>,
) -> Result<DictionaryEntry, String> {
    let entry = crate::storage::dictionary::add_entry(&state.db, &phrase, &replacement, None)
        .map_err(|e| e.to_string())?;
    sync_processor(&state);
    sync_whisper_prompt(&state);
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
    sync_processor(&state);
    sync_whisper_prompt(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_dictionary_entry(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::dictionary::delete_entry(&state.db, &id).map_err(|e| e.to_string())?;
    sync_processor(&state);
    sync_whisper_prompt(&state);
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
        None,
    )
    .map_err(|e| e.to_string())?;
    sync_processor(&state);
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
    sync_processor(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_snippet(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::snippets::delete_snippet(&state.db, &id).map_err(|e| e.to_string())?;
    sync_processor(&state);
    Ok(())
}

#[tauri::command]
pub async fn list_snippets(
    state: State<'_, AppState>,
) -> Result<Vec<Snippet>, String> {
    crate::storage::snippets::list_snippets(&state.db).map_err(|e| e.to_string())
}

// ── Mode-scoped entry commands (Profile editor) ──

#[tauri::command]
pub async fn list_mode_dictionary_entries(
    mode_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<DictionaryEntry>, String> {
    crate::storage::dictionary::list_entries_for_mode(&state.db, &mode_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_mode_dictionary_entry(
    mode_id: String,
    phrase: String,
    replacement: String,
    state: State<'_, AppState>,
) -> Result<DictionaryEntry, String> {
    let entry = crate::storage::dictionary::add_entry(
        &state.db,
        &phrase,
        &replacement,
        Some(&mode_id),
    )
    .map_err(|e| e.to_string())?;
    sync_processor(&state);
    sync_whisper_prompt(&state);
    Ok(entry)
}

#[tauri::command]
pub async fn delete_mode_dictionary_entry(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::dictionary::delete_entry(&state.db, &id).map_err(|e| e.to_string())?;
    sync_processor(&state);
    sync_whisper_prompt(&state);
    Ok(())
}

#[tauri::command]
pub async fn list_mode_snippets(
    mode_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<Snippet>, String> {
    crate::storage::snippets::list_snippets_for_mode(&state.db, &mode_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_mode_snippet(
    mode_id: String,
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
        Some(&mode_id),
    )
    .map_err(|e| e.to_string())?;
    sync_processor(&state);
    Ok(snippet)
}

#[tauri::command]
pub async fn delete_mode_snippet(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::snippets::delete_snippet(&state.db, &id).map_err(|e| e.to_string())?;
    sync_processor(&state);
    Ok(())
}

// ── Vocabulary commands ──

#[tauri::command]
pub async fn add_vocabulary_entry(
    word: String,
    state: State<'_, AppState>,
) -> Result<VocabularyEntry, String> {
    let entry = crate::storage::vocabulary::add_entry(&state.db, &word, None)
        .map_err(|e| e.to_string())?;
    sync_whisper_prompt(&state);
    Ok(entry)
}

#[tauri::command]
pub async fn update_vocabulary_entry(
    id: String,
    word: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::vocabulary::update_entry(&state.db, &id, &word)
        .map_err(|e| e.to_string())?;
    sync_whisper_prompt(&state);
    Ok(())
}

#[tauri::command]
pub async fn delete_vocabulary_entry(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::vocabulary::delete_entry(&state.db, &id).map_err(|e| e.to_string())?;
    sync_whisper_prompt(&state);
    Ok(())
}

#[tauri::command]
pub async fn list_vocabulary_entries(
    state: State<'_, AppState>,
) -> Result<Vec<VocabularyEntry>, String> {
    crate::storage::vocabulary::list_entries(&state.db).map_err(|e| e.to_string())
}

// ── Mode-scoped vocabulary commands ──

#[tauri::command]
pub async fn list_mode_vocabulary_entries(
    mode_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<VocabularyEntry>, String> {
    crate::storage::vocabulary::list_entries_for_mode(&state.db, &mode_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_mode_vocabulary_entry(
    mode_id: String,
    word: String,
    state: State<'_, AppState>,
) -> Result<VocabularyEntry, String> {
    let entry = crate::storage::vocabulary::add_entry(&state.db, &word, Some(&mode_id))
        .map_err(|e| e.to_string())?;
    sync_whisper_prompt(&state);
    Ok(entry)
}

#[tauri::command]
pub async fn delete_mode_vocabulary_entry(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::vocabulary::delete_entry(&state.db, &id).map_err(|e| e.to_string())?;
    sync_whisper_prompt(&state);
    Ok(())
}
