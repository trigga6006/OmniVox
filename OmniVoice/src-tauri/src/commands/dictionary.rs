use crate::storage::types::{DictionaryEntry, Snippet};

#[tauri::command]
pub async fn add_dictionary_entry(
    phrase: String,
    replacement: String,
) -> Result<DictionaryEntry, String> {
    crate::storage::dictionary::add_entry(&phrase, &replacement).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_dictionary_entry(
    id: String,
    phrase: String,
    replacement: String,
) -> Result<(), String> {
    crate::storage::dictionary::update_entry(&id, &phrase, &replacement)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_dictionary_entry(id: String) -> Result<(), String> {
    crate::storage::dictionary::delete_entry(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_dictionary_entries() -> Result<Vec<DictionaryEntry>, String> {
    crate::storage::dictionary::list_entries().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_snippet(
    trigger: String,
    content: String,
    description: Option<String>,
) -> Result<Snippet, String> {
    crate::storage::snippets::add_snippet(
        &trigger,
        &content,
        description.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_snippet(
    id: String,
    trigger: String,
    content: String,
    description: Option<String>,
) -> Result<(), String> {
    crate::storage::snippets::update_snippet(
        &id,
        &trigger,
        &content,
        description.as_deref(),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_snippet(id: String) -> Result<(), String> {
    crate::storage::snippets::delete_snippet(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_snippets() -> Result<Vec<Snippet>, String> {
    crate::storage::snippets::list_snippets().map_err(|e| e.to_string())
}
