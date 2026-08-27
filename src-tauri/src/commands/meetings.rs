use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::meeting::{
    self,
    provider::{OpenRouterModel, ProviderStatus, SummaryEstimate},
};
use crate::state::AppState;
use crate::storage::meetings::{
    self, Meeting, MeetingActionItem, MeetingAiUsageRecord, MeetingDetail, MeetingMarker,
    MeetingProviderSettings, MeetingQuestionAnswer, MeetingRecoveryItem, MeetingSummaryVersion,
    MeetingTemplate, MeetingUsageStats,
};

#[derive(Debug, Serialize)]
pub struct MeetingExportResult {
    path: String,
    file_name: String,
}

fn require(window: &tauri::WebviewWindow, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&window.label()) {
        Ok(())
    } else {
        Err(format!(
            "Meeting command is not available to the '{}' window",
            window.label()
        ))
    }
}

#[tauri::command]
pub async fn meeting_start(
    window: tauri::WebviewWindow,
    app: AppHandle,
    title: String,
    source_app: Option<String>,
    previous_meeting_id: Option<String>,
) -> Result<Meeting, String> {
    require(
        &window,
        &[
            "main",
            "overlay",
            "meeting",
            "meeting-widget",
            "meeting-suggestion",
        ],
    )?;
    let previous_meeting_id = previous_meeting_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let meeting = meeting::start(&app, &title, source_app.as_deref(), previous_meeting_id)?;
    if let Err(error) = show_meeting_widget_impl(&app).await {
        eprintln!("Meeting widget could not be shown: {error}");
    }
    if let Some(suggestion) = app.get_webview_window("meeting-suggestion") {
        let _ = suggestion.hide();
    }
    Ok(meeting)
}

#[tauri::command]
pub async fn meeting_pause(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    require(&window, &["main", "overlay", "meeting", "meeting-widget"])?;
    meeting::pause(&app)
}
#[tauri::command]
pub async fn meeting_resume(window: tauri::WebviewWindow, app: AppHandle) -> Result<(), String> {
    require(&window, &["main", "overlay", "meeting", "meeting-widget"])?;
    meeting::resume(&app)
}
#[tauri::command]
pub async fn meeting_stop(window: tauri::WebviewWindow, app: AppHandle) -> Result<String, String> {
    require(&window, &["main", "overlay", "meeting", "meeting-widget"])?;
    let id = meeting::stop(&app)?;
    // The right-edge widget becomes the right-edge drawer: retire the widget
    // first so the drawer is the only thing that appears in its place.
    hide_meeting_widget(&app);
    if let Err(error) = open_meeting_drawer_impl(&app, Some(&id)).await {
        eprintln!("Meeting drawer could not be shown after stop: {error}");
    }
    Ok(id)
}

#[tauri::command]
pub async fn meeting_state(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<crate::meeting::MeetingStatePayload, String> {
    require(&window, &["main", "overlay", "meeting", "meeting-widget"])?;
    Ok(state.meeting.state())
}

#[tauri::command]
pub async fn test_meeting_audio(
    window: tauri::WebviewWindow,
    app: AppHandle,
) -> Result<meeting::MeetingAudioReadiness, String> {
    require(&window, &["main", "meeting"])?;
    tokio::task::spawn_blocking(move || meeting::test_audio_readiness(&app))
        .await
        .map_err(|error| format!("Meeting audio test failed: {error}"))?
}

#[tauri::command]
pub async fn list_meetings(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<Meeting>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_meetings(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_meeting_recovery_items(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingRecoveryItem>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_recovery_items(&state.db).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_meeting_templates(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingTemplate>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_templates(&state.db).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_meeting_as_template(
    window: tauri::WebviewWindow,
    meeting_id: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<MeetingTemplate, String> {
    require(&window, &["main", "meeting"])?;
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err("Template name is required".into());
    }
    if name.chars().count() > 80 {
        return Err("Template names must be 80 characters or fewer".into());
    }
    meetings::save_meeting_as_template(&state.db, &meeting_id, &name)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_meeting_template(
    window: tauri::WebviewWindow,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::delete_template(&state.db, &id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_deleted_meetings(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<Meeting>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_deleted_meetings(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_meetings(
    window: tauri::WebviewWindow,
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<Meeting>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::search_meetings(&state.db, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_deleted_meetings(
    window: tauri::WebviewWindow,
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<Meeting>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::search_deleted_meetings(&state.db, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_meeting(
    window: tauri::WebviewWindow,
    id: String,
    state: State<'_, AppState>,
) -> Result<Option<MeetingDetail>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::get_detail(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_meeting_notes(
    window: tauri::WebviewWindow,
    id: String,
    notes: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::update_notes(&state.db, &id, &notes).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_meeting_ai_options(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    model: Option<String>,
    preset: Option<String>,
    instructions: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    let model = model
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if model
        .as_ref()
        .is_some_and(|value| value.chars().count() > 200)
    {
        return Err("OpenRouter model identifiers must be 200 characters or fewer".into());
    }
    let preset = preset
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if preset
        .as_ref()
        .is_some_and(|value| !meeting::provider::valid_summary_preset(value))
    {
        return Err("Unknown meeting summary preset".into());
    }
    if instructions.chars().count() > 2_000 {
        return Err("Meeting summary instructions must be 2,000 characters or fewer".into());
    }
    meetings::update_ai_options(
        &app.state::<AppState>().db,
        &id,
        model.as_deref(),
        preset.as_deref(),
        instructions.trim(),
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &id);
    Ok(())
}

#[tauri::command]
pub async fn update_meeting_metadata(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    agenda: Option<String>,
    participants: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    require(&window, &["main", "meeting"])?;
    let agenda = agenda.map(|value| value.trim().to_string());
    if agenda
        .as_ref()
        .is_some_and(|value| value.chars().count() > 2_000)
    {
        return Err("Meeting agendas must be 2,000 characters or fewer".into());
    }
    let participants = participants.map(normalize_meeting_participants);
    meetings::update_metadata(
        &app.state::<AppState>().db,
        &id,
        agenda.as_deref(),
        participants.as_deref(),
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &id);
    Ok(participants.unwrap_or_default())
}

fn normalize_meeting_participants(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let name = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let name = name.chars().take(80).collect::<String>();
        if name.is_empty()
            || normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        normalized.push(name);
        if normalized.len() == 50 {
            break;
        }
    }
    normalized
}

#[tauri::command]
pub async fn update_meeting_title(
    window: tauri::WebviewWindow,
    id: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    if title.trim().is_empty() {
        return Err("Meeting title cannot be empty".into());
    };
    meetings::update_title(&state.db, &id, title.trim()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_meeting_favorite(
    window: tauri::WebviewWindow,
    id: String,
    favorite: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::set_favorite(&state.db, &id, favorite).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_meeting_tags(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    tags: Vec<String>,
) -> Result<Vec<String>, String> {
    require(&window, &["main", "meeting"])?;
    let normalized = normalize_meeting_tags(tags);
    meetings::set_tags(&app.state::<AppState>().db, &id, &normalized).map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &id);
    Ok(normalized)
}

fn normalize_meeting_tags(tags: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for raw in tags {
        let tag = raw
            .trim()
            .trim_start_matches('#')
            .chars()
            .take(32)
            .collect::<String>();
        if tag.is_empty()
            || normalized
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(&tag))
        {
            continue;
        }
        normalized.push(tag);
        if normalized.len() == 12 {
            break;
        }
    }
    normalized
}

#[tauri::command]
pub async fn update_meeting_segment(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
    text: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    if text.trim().is_empty() {
        return Err("Transcript text cannot be empty".into());
    }
    if text.chars().count() > 8_000 {
        return Err("Transcript segment must be 8,000 characters or fewer".into());
    }
    meetings::update_segment_text(&app.state::<AppState>().db, &id, &meeting_id, &text)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn update_meeting_segment_speaker(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
    label: String,
    apply_to_source: bool,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if label.chars().count() > 80 {
        return Err("Speaker names must be 80 characters or fewer".into());
    }
    meetings::update_segment_speaker(
        &app.state::<AppState>().db,
        &id,
        &meeting_id,
        (!label.is_empty()).then_some(label.as_str()),
        apply_to_source,
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn set_meeting_action_completed(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    task: String,
    completed: bool,
) -> Result<Vec<String>, String> {
    require(&window, &["main", "meeting"])?;
    let task = task.trim();
    if task.is_empty() || task.chars().count() > 500 {
        return Err("Action item text must be between 1 and 500 characters".into());
    }
    let completed_actions =
        meetings::set_action_completed(&app.state::<AppState>().db, &id, task, completed)
            .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &id);
    Ok(completed_actions)
}

#[tauri::command]
pub async fn list_meeting_action_items(
    window: tauri::WebviewWindow,
    meeting_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingActionItem>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_action_items(&state.db, meeting_id.as_deref()).map_err(|error| error.to_string())
}

fn normalize_action_fields(
    task: String,
    owner: Option<String>,
    due: Option<String>,
) -> Result<(String, Option<String>, Option<String>), String> {
    let task = task.split_whitespace().collect::<Vec<_>>().join(" ");
    if task.is_empty() || task.chars().count() > 500 {
        return Err("Action item text must be between 1 and 500 characters".into());
    }
    let normalize_optional = |value: Option<String>, label: &str| {
        let value = value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if value
            .as_ref()
            .is_some_and(|value| value.chars().count() > 200)
        {
            Err(format!(
                "Action item {label} must be 200 characters or fewer"
            ))
        } else {
            Ok(value)
        }
    };
    Ok((
        task,
        normalize_optional(owner, "owner")?,
        normalize_optional(due, "due date")?,
    ))
}

#[tauri::command]
pub async fn add_meeting_action_item(
    window: tauri::WebviewWindow,
    app: AppHandle,
    meeting_id: String,
    task: String,
    owner: Option<String>,
    due: Option<String>,
) -> Result<MeetingActionItem, String> {
    require(&window, &["main", "meeting"])?;
    let (task, owner, due) = normalize_action_fields(task, owner, due)?;
    let item = meetings::add_action_item(
        &app.state::<AppState>().db,
        &meeting_id,
        &task,
        owner.as_deref(),
        due.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(item)
}

#[tauri::command]
pub async fn update_meeting_action_item(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
    task: String,
    owner: Option<String>,
    due: Option<String>,
) -> Result<MeetingActionItem, String> {
    require(&window, &["main", "meeting"])?;
    let (task, owner, due) = normalize_action_fields(task, owner, due)?;
    let item = meetings::update_action_item(
        &app.state::<AppState>().db,
        &id,
        &meeting_id,
        &task,
        owner.as_deref(),
        due.as_deref(),
    )
    .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(item)
}

#[tauri::command]
pub async fn set_meeting_action_item_completed(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
    completed: bool,
) -> Result<MeetingActionItem, String> {
    require(&window, &["main", "meeting"])?;
    let item = meetings::set_action_item_completed(
        &app.state::<AppState>().db,
        &id,
        &meeting_id,
        completed,
    )
    .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(item)
}

#[tauri::command]
pub async fn delete_meeting_action_item(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::delete_action_item(&app.state::<AppState>().db, &id, &meeting_id)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn add_meeting_marker(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    at_ms: Option<u64>,
    label: Option<String>,
) -> Result<MeetingMarker, String> {
    require(&window, &["main", "meeting", "meeting-widget"])?;
    let state = app.state::<AppState>();
    let timestamp = match at_ms {
        Some(value) => value,
        None => {
            let runtime = state.meeting.state();
            if runtime.meeting_id.as_deref() != Some(&id) {
                return Err("A timestamp is required for an inactive meeting".into());
            }
            runtime.elapsed_ms
        }
    };
    let marker = meetings::add_marker(
        &state.db,
        &id,
        timestamp,
        label.as_deref().unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &id);
    Ok(marker)
}

#[tauri::command]
pub async fn update_meeting_marker(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
    label: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::update_marker(&app.state::<AppState>().db, &id, &meeting_id, &label)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn delete_meeting_marker(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::delete_marker(&app.state::<AppState>().db, &id, &meeting_id)
        .map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn update_meeting_summary(
    window: tauri::WebviewWindow,
    id: String,
    markdown: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::update_summary_markdown(&state.db, &id, &markdown).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_meeting_summary_versions(
    window: tauri::WebviewWindow,
    meeting_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingSummaryVersion>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_summary_versions(&state.db, &meeting_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn restore_meeting_summary_version(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
) -> Result<MeetingSummaryVersion, String> {
    require(&window, &["main", "meeting"])?;
    let version = meetings::restore_summary_version(&app.state::<AppState>().db, &id, &meeting_id)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(version)
}

#[tauri::command]
pub async fn delete_meeting(
    window: tauri::WebviewWindow,
    id: String,
    app: AppHandle,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    let state = app.state::<AppState>();
    if state.meeting.state().meeting_id.as_deref() == Some(&id) {
        return Err("End the meeting before deleting it".into());
    }
    meetings::trash_meeting(&state.db, &id).map_err(|e| e.to_string())?;
    hide_meeting_widget(&app);
    let _ = app.emit("meeting-updated", &id);
    Ok(())
}

#[tauri::command]
pub async fn restore_meeting(
    window: tauri::WebviewWindow,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::restore_meeting(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn permanently_delete_meeting(
    window: tauri::WebviewWindow,
    id: String,
    app: AppHandle,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    let state = app.state::<AppState>();
    if state.meeting.state().meeting_id.as_deref() == Some(&id) {
        return Err("End the meeting before deleting it".into());
    }
    let paths = meetings::delete_meeting(&state.db, &id).map_err(|e| e.to_string())?;
    let root = state.data_dir.join("meetings");
    for raw in paths {
        let path = std::path::PathBuf::from(raw);
        if path.starts_with(&root) {
            let _ = std::fs::remove_file(path);
        }
    }
    let dir = root.join(&id);
    if dir.starts_with(&root) {
        let _ = std::fs::remove_dir(dir);
    }
    hide_meeting_widget(&app);
    let _ = app.emit("meeting-updated", &id);
    Ok(())
}

#[tauri::command]
pub async fn estimate_meeting_summary(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
    id: String,
    model: Option<String>,
) -> Result<SummaryEstimate, String> {
    require(&window, &["main", "meeting"])?;
    meeting::provider::estimate(&state.db, &id, model.as_deref()).await
}

#[tauri::command]
pub async fn ask_meeting_question(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    question: String,
    model: Option<String>,
) -> Result<MeetingQuestionAnswer, String> {
    require(&window, &["main", "meeting"])?;
    let answer = meeting::provider::answer_question(
        &app.state::<AppState>().db,
        &id,
        &question,
        model.as_deref(),
    )
    .await?;
    let _ = app.emit("meeting-updated", &id);
    Ok(answer)
}

#[tauri::command]
pub async fn delete_meeting_question(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    meeting_id: String,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meetings::delete_question_answer(&app.state::<AppState>().db, &id, &meeting_id)
        .map_err(|error| error.to_string())?;
    let _ = app.emit("meeting-updated", &meeting_id);
    Ok(())
}

#[tauri::command]
pub async fn summarize_meeting(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
    model: Option<String>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meeting::summarize_and_emit(&app, &id, model.as_deref()).await
}

#[tauri::command]
pub async fn retry_meeting_transcription(
    window: tauri::WebviewWindow,
    app: AppHandle,
    id: String,
) -> Result<u64, String> {
    require(&window, &["main", "meeting"])?;
    meeting::retry_transcription(&app, &id)
}

#[tauri::command]
pub async fn get_meeting_provider_settings(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<MeetingProviderSettings, String> {
    require(&window, &["main", "meeting"])?;
    meetings::get_provider_settings(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_meeting_usage_stats(
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<MeetingUsageStats, String> {
    require(&window, &["main", "meeting"])?;
    meetings::usage_stats(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_meeting_ai_usage(
    window: tauri::WebviewWindow,
    limit: Option<u64>,
    state: State<'_, AppState>,
) -> Result<Vec<MeetingAiUsageRecord>, String> {
    require(&window, &["main", "meeting"])?;
    meetings::list_ai_usage(&state.db, limit.unwrap_or(200)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_meeting_provider_settings(
    window: tauri::WebviewWindow,
    settings: MeetingProviderSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    if settings.monthly_budget < 0.0
        || settings.per_meeting_budget < 0.0
        || settings.max_prompt_price <= 0.0
        || settings.max_completion_price <= 0.0
    {
        return Err("Provider budgets and price limits must be positive".into());
    }
    if !meeting::provider::valid_summary_preset(&settings.summary_preset) {
        return Err("Unknown meeting summary preset".into());
    }
    if !matches!(
        settings.transcription_mode.as_str(),
        "after_meeting" | "live"
    ) {
        return Err("Unknown meeting transcription mode".into());
    }
    if !matches!(
        settings.routing_preference.as_str(),
        "balanced" | "price" | "throughput" | "latency"
    ) {
        return Err("Unknown OpenRouter provider routing preference".into());
    }
    if settings.custom_instructions.chars().count() > 2_000 {
        return Err("Custom summary instructions must be 2,000 characters or fewer".into());
    }
    meetings::save_provider_settings(&state.db, &settings).map_err(|e| e.to_string())?;
    // The idle call detector reads `auto_suggest` from a cache instead of
    // SQLite; this is the only writer, so it owns the invalidation.
    meeting::invalidate_auto_suggest_cache();
    Ok(())
}

#[tauri::command]
pub async fn save_openrouter_key(
    window: tauri::WebviewWindow,
    key: String,
) -> Result<ProviderStatus, String> {
    require(&window, &["main", "meeting"])?;
    meeting::credentials::save_openrouter_key(&key)?;
    let status = meeting::provider::status().await;
    if status.is_management_key == Some(true) {
        let _ = meeting::credentials::delete_openrouter_key();
        return Err("Use a regular OpenRouter inference key. Management keys can change account-wide billing and are intentionally not stored by OmniVox.".into());
    }
    if !status.connected {
        let _ = meeting::credentials::delete_openrouter_key();
        return Err(status
            .error
            .unwrap_or_else(|| "OpenRouter rejected the API key".into()));
    }
    Ok(status)
}

#[tauri::command]
pub async fn remove_openrouter_key(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    meeting::credentials::delete_openrouter_key()
}

#[tauri::command]
pub async fn get_openrouter_status(window: tauri::WebviewWindow) -> Result<ProviderStatus, String> {
    require(&window, &["main", "meeting"])?;
    Ok(meeting::provider::status().await)
}

#[tauri::command]
pub async fn list_openrouter_models(
    window: tauri::WebviewWindow,
) -> Result<Vec<OpenRouterModel>, String> {
    require(&window, &["main", "meeting"])?;
    meeting::provider::models_with_benchmarks().await
}

#[tauri::command]
pub fn open_openrouter_credits(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    crate::actions::executor::run_open_url("https://openrouter.ai/settings/credits")
}

#[tauri::command]
pub fn open_openrouter_keys(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    crate::actions::executor::run_open_url("https://openrouter.ai/settings/keys")
}

#[tauri::command]
pub fn export_meeting(
    window: tauri::WebviewWindow,
    id: String,
    format: String,
    state: State<'_, AppState>,
) -> Result<MeetingExportResult, String> {
    require(&window, &["main", "meeting"])?;
    let meeting = meetings::get_meeting(&state.db, &id)
        .map_err(|e| e.to_string())?
        .ok_or("Meeting not found")?;
    let extension = match format.as_str() {
        "notes" | "markdown" => "md",
        "json" => "json",
        "webvtt" => "vtt",
        "srt" => "srt",
        _ => return Err("Export format must be notes, markdown, json, webvtt, or srt".into()),
    };
    let content = match format.as_str() {
        "notes" => meetings::export_polished_notes(&state.db, &id).map_err(|e| e.to_string())?,
        "markdown" => meetings::export_markdown(&state.db, &id).map_err(|e| e.to_string())?,
        "webvtt" => meetings::export_webvtt(&state.db, &id).map_err(|e| e.to_string())?,
        "srt" => meetings::export_srt(&state.db, &id).map_err(|e| e.to_string())?,
        "json" => {
            let detail = meetings::get_detail(&state.db, &id)
                .map_err(|e| e.to_string())?
                .ok_or("Meeting not found")?;
            let versions = meetings::list_summary_versions(&state.db, &id)
                .map_err(|error| error.to_string())?;
            let mut archive = serde_json::to_value(&detail).map_err(|e| e.to_string())?;
            archive
                .as_object_mut()
                .ok_or("Meeting archive could not be serialized")?
                .insert(
                    "summary_versions".into(),
                    serde_json::to_value(versions).map_err(|e| e.to_string())?,
                );
            serde_json::to_string_pretty(&archive).map_err(|e| e.to_string())? + "\n"
        }
        _ => unreachable!("export format was validated above"),
    };
    let stem = sanitize_export_name(&meeting.title);
    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let profile = if format == "notes" { "-notes" } else { "" };
    let file_name = format!("{stem}{profile}-{timestamp}.{extension}");
    let export_dir = state.data_dir.join("meeting-exports");
    std::fs::create_dir_all(&export_dir).map_err(|e| e.to_string())?;
    let path = export_dir.join(&file_name);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(MeetingExportResult {
        path: path.to_string_lossy().into_owned(),
        file_name,
    })
}

#[tauri::command]
pub fn reveal_meeting_export(
    window: tauri::WebviewWindow,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require(&window, &["main", "meeting"])?;
    let root = state.data_dir.join("meeting-exports");
    let root = std::fs::canonicalize(&root).map_err(|e| e.to_string())?;
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("That export is outside OmniVox's meeting export folder".into());
    }
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(path.parent().ok_or("Export folder is missing")?)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn sanitize_export_name(title: &str) -> String {
    let value = title
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else if character.is_whitespace() {
                '-'
            } else {
                '_'
            }
        })
        .collect::<String>();
    let value = value
        .trim_matches(['-', '_'])
        .chars()
        .take(80)
        .collect::<String>();
    if value.is_empty() {
        "meeting".into()
    } else {
        value
    }
}

#[cfg(test)]
mod export_tests {
    use super::{normalize_meeting_participants, normalize_meeting_tags, sanitize_export_name};

    #[test]
    fn export_names_are_portable_and_bounded() {
        assert_eq!(
            sanitize_export_name(" Weekly / product sync? "),
            "Weekly-_-product-sync"
        );
        assert_eq!(sanitize_export_name("///"), "meeting");
        assert!(sanitize_export_name(&"a".repeat(200)).len() <= 80);
    }

    #[test]
    fn meeting_tags_are_clean_deduplicated_and_bounded() {
        let tags = normalize_meeting_tags(vec![
            " #Roadmap ".into(),
            "roadmap".into(),
            "".into(),
            "a".repeat(50),
        ]);
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], "Roadmap");
        assert_eq!(tags[1].len(), 32);
    }

    #[test]
    fn meeting_participants_are_clean_deduplicated_and_bounded() {
        let participants = normalize_meeting_participants(vec![
            "  Alex   Morgan ".into(),
            "alex morgan".into(),
            "".into(),
            "Jordan".into(),
        ]);
        assert_eq!(participants, vec!["Alex Morgan", "Jordan"]);
    }
}

#[tauri::command]
pub async fn open_meeting_drawer(
    window: tauri::WebviewWindow,
    app: AppHandle,
    meeting_id: Option<String>,
) -> Result<(), String> {
    require(&window, &["main", "overlay", "meeting", "meeting-widget"])?;
    open_meeting_drawer_impl(&app, meeting_id.as_deref()).await
}

pub async fn open_meeting_drawer_impl(
    app: &AppHandle,
    meeting_id: Option<&str>,
) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("meeting") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        if let Some(id) = meeting_id {
            let _ = app.emit("meeting-select", id);
        }
        return Ok(());
    }
    let (w, h) = (440.0, 720.0);
    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let ax = area.position.x as f64 / scale;
        let ay = area.position.y as f64 / scale;
        let aw = area.size.width as f64 / scale;
        let ah = area.size.height as f64 / scale;
        (ax + aw - w - 14.0, ay + (ah - h).max(0.0) / 2.0)
    } else {
        (800.0, 120.0)
    };
    // The drawer picks its meeting up from the URL: the delayed event below can
    // land before a freshly created WebView has mounted its listener, and a lost
    // event leaves the drawer showing the newest meeting instead of this one.
    let drawer_url = match meeting_id {
        Some(id) => format!(
            "/meeting.html?meeting={}",
            url::form_urlencoded::byte_serialize(id.as_bytes()).collect::<String>()
        ),
        None => "/meeting.html".to_string(),
    };
    let win = WebviewWindowBuilder::new(app, "meeting", WebviewUrl::App(drawer_url.into()))
        .title("OmniVox Meeting Notes")
        .inner_size(w, h)
        .min_inner_size(360.0, 480.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .focused(true)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = handle.get_webview_window("meeting") {
                let _ = w.hide();
            }
        }
    });
    if let Some(id) = meeting_id {
        let handle = app.clone();
        let id = id.to_string();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            let _ = handle.emit("meeting-select", id);
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn close_meeting_drawer(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["meeting"])?;
    window.hide().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn dismiss_meeting_widget(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["meeting-widget"])?;
    window.hide().map_err(|e| e.to_string())
}

pub fn hide_meeting_widget(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("meeting-widget") {
        let _ = window.hide();
    }
}

#[tauri::command]
pub async fn dismiss_meeting_suggestion(window: tauri::WebviewWindow) -> Result<(), String> {
    require(&window, &["meeting-suggestion"])?;
    // Tell the detection gate this call was declined, or the banner resurfaces
    // on the next poll tick / title change for as long as the call lasts.
    crate::meeting::decline_current_suggestion();
    window.hide().map_err(|e| e.to_string())
}

pub async fn show_meeting_widget_impl(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("meeting-widget") {
        let _ = win.show();
        return Ok(());
    }
    let (w, h) = (78.0, 142.0);
    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let ax = area.position.x as f64 / scale;
        let ay = area.position.y as f64 / scale;
        let aw = area.size.width as f64 / scale;
        let ah = area.size.height as f64 / scale;
        (ax + aw - w - 10.0, ay + (ah - h) / 2.0)
    } else {
        (1000.0, 350.0)
    };
    WebviewWindowBuilder::new(
        app,
        "meeting-widget",
        WebviewUrl::App("/meeting-widget.html".into()),
    )
    .title("")
    .inner_size(w, h)
    .min_inner_size(w, h)
    .max_inner_size(w, h)
    .position(x, y)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}
