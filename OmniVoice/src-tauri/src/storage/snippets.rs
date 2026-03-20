use crate::error::AppResult;
use crate::storage::types::Snippet;
use chrono::Utc;
use uuid::Uuid;

/// Add a new text snippet.
pub fn add_snippet(
    trigger: &str,
    content: &str,
    description: Option<&str>,
) -> AppResult<Snippet> {
    Ok(Snippet {
        id: Uuid::new_v4(),
        trigger: trigger.to_string(),
        content: content.to_string(),
        description: description.map(|d| d.to_string()),
        is_enabled: true,
        created_at: Utc::now(),
    })
}

/// Update an existing snippet.
pub fn update_snippet(
    _id: &str,
    _trigger: &str,
    _content: &str,
    _description: Option<&str>,
) -> AppResult<()> {
    Ok(())
}

/// Delete a snippet by ID.
pub fn delete_snippet(_id: &str) -> AppResult<()> {
    Ok(())
}

/// List all snippets.
pub fn list_snippets() -> AppResult<Vec<Snippet>> {
    Ok(vec![])
}
