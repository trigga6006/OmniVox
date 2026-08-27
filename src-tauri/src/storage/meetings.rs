use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppResult;
use crate::storage::database::Database;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub status: String,
    pub source_app: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub user_notes: String,
    pub summary_markdown: String,
    pub summary_json: Option<String>,
    pub summary_provider: Option<String>,
    pub summary_model: Option<String>,
    pub summary_cost: Option<f64>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub is_favorite: bool,
    pub tags: Vec<String>,
    pub completed_actions: Vec<String>,
    pub summary_stale: bool,
    pub error: Option<String>,
    pub deleted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub ai_model_override: Option<String>,
    pub summary_preset_override: Option<String>,
    pub summary_instructions: String,
    pub agenda: String,
    pub participants: Vec<String>,
    pub previous_meeting_id: Option<String>,
    pub actions_materialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingSegment {
    pub id: String,
    pub meeting_id: String,
    pub source: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub speaker_label: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingMarker {
    pub id: String,
    pub meeting_id: String,
    pub at_ms: u64,
    pub label: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingActionItem {
    pub id: String,
    pub meeting_id: String,
    pub task: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    pub segment_refs: Vec<String>,
    pub origin: String,
    pub completed: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingQuestionAnswer {
    pub id: String,
    pub meeting_id: String,
    pub question: String,
    pub answer: String,
    pub segment_refs: Vec<String>,
    pub provider: String,
    pub model: String,
    pub cost: f64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub context_segments: u64,
    pub total_segments: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingSummaryVersion {
    pub id: String,
    pub meeting_id: String,
    pub markdown: String,
    pub summary_json: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub cost: Option<f64>,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub origin: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetingRecoveryItem {
    pub meeting_id: String,
    pub title: String,
    pub status: String,
    pub pending_chunks: u64,
    pub processing_chunks: u64,
    pub failed_chunks: u64,
    pub recoverable_failed_chunks: u64,
    pub completed_chunks: u64,
    pub has_summary: bool,
    pub error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingDetail {
    #[serde(flatten)]
    pub meeting: Meeting,
    pub segments: Vec<MeetingSegment>,
    pub markers: Vec<MeetingMarker>,
    pub action_items: Vec<MeetingActionItem>,
    pub questions: Vec<MeetingQuestionAnswer>,
    pub transcription: MeetingTranscriptionProgress,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeetingTranscriptionProgress {
    pub total: u64,
    pub completed: u64,
    pub pending: u64,
    pub processing: u64,
    pub failed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingChunk {
    pub id: String,
    pub meeting_id: String,
    pub source: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub audio_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingProviderSettings {
    pub provider: String,
    pub model: String,
    pub monthly_budget: f64,
    pub per_meeting_budget: f64,
    pub max_prompt_price: f64,
    pub max_completion_price: f64,
    pub zdr_only: bool,
    pub deny_data_collection: bool,
    pub auto_suggest: bool,
    pub auto_summarize: bool,
    pub summary_preset: String,
    pub custom_instructions: String,
    pub transcription_mode: String,
    pub routing_preference: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingUsageStats {
    pub month_spend: f64,
    pub request_count: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingAiUsageRecord {
    pub id: String,
    pub meeting_id: Option<String>,
    pub meeting_title: Option<String>,
    pub request_kind: String,
    pub provider: String,
    pub model: String,
    pub cost: f64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetingTemplate {
    pub id: String,
    pub name: String,
    pub title: String,
    pub agenda: String,
    pub participants: Vec<String>,
    pub ai_model_override: Option<String>,
    pub summary_preset_override: Option<String>,
    pub summary_instructions: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Default for MeetingProviderSettings {
    fn default() -> Self {
        Self {
            provider: "openrouter".into(),
            model: "deepseek/deepseek-v4-flash".into(),
            monthly_budget: 2.0,
            per_meeting_budget: 0.05,
            max_prompt_price: 0.5,
            max_completion_price: 3.0,
            zdr_only: true,
            deny_data_collection: true,
            auto_suggest: true,
            auto_summarize: false,
            summary_preset: "general".into(),
            custom_instructions: String::new(),
            transcription_mode: "after_meeting".into(),
            routing_preference: "price".into(),
        }
    }
}

fn row_to_meeting(row: &rusqlite::Row<'_>) -> rusqlite::Result<Meeting> {
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        status: row.get(2)?,
        source_app: row.get(3)?,
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        user_notes: row.get(6)?,
        summary_markdown: row.get(7)?,
        summary_json: row.get(8)?,
        summary_provider: row.get(9)?,
        summary_model: row.get(10)?,
        summary_cost: row.get(11)?,
        prompt_tokens: row.get::<_, Option<i64>>(12)?.map(|v| v.max(0) as u64),
        completion_tokens: row.get::<_, Option<i64>>(13)?.map(|v| v.max(0) as u64),
        is_favorite: row.get::<_, i64>(14)? != 0,
        tags: serde_json::from_str(&row.get::<_, String>(15)?).unwrap_or_default(),
        completed_actions: serde_json::from_str(&row.get::<_, String>(16)?).unwrap_or_default(),
        summary_stale: row.get::<_, i64>(17)? != 0,
        error: row.get(18)?,
        deleted_at: row.get(19)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
        ai_model_override: row.get(22)?,
        summary_preset_override: row.get(23)?,
        summary_instructions: row.get(24)?,
        agenda: row.get(25)?,
        participants: serde_json::from_str(&row.get::<_, String>(26)?).unwrap_or_default(),
        previous_meeting_id: row.get(27)?,
        actions_materialized: row.get::<_, i64>(28)? != 0,
    })
}

fn row_to_meeting_template(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingTemplate> {
    Ok(MeetingTemplate {
        id: row.get(0)?,
        name: row.get(1)?,
        title: row.get(2)?,
        agenda: row.get(3)?,
        participants: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
        ai_model_override: row.get(5)?,
        summary_preset_override: row.get(6)?,
        summary_instructions: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn row_to_action_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingActionItem> {
    Ok(MeetingActionItem {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        task: row.get(2)?,
        owner: row.get(3)?,
        due: row.get(4)?,
        segment_refs: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or_default(),
        origin: row.get(6)?,
        completed: row.get::<_, i64>(7)? != 0,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn row_to_summary_version(row: &rusqlite::Row<'_>) -> rusqlite::Result<MeetingSummaryVersion> {
    Ok(MeetingSummaryVersion {
        id: row.get(0)?,
        meeting_id: row.get(1)?,
        markdown: row.get(2)?,
        summary_json: row.get(3)?,
        provider: row.get(4)?,
        model: row.get(5)?,
        cost: row.get(6)?,
        prompt_tokens: row
            .get::<_, Option<i64>>(7)?
            .map(|value| value.max(0) as u64),
        completion_tokens: row
            .get::<_, Option<i64>>(8)?
            .map(|value| value.max(0) as u64),
        origin: row.get(9)?,
        created_at: row.get(10)?,
    })
}

const ACTION_ITEM_COLUMNS: &str =
    "id,meeting_id,task,owner,due,segment_refs_json,origin,completed,created_at,updated_at";
const QUALIFIED_ACTION_ITEM_COLUMNS: &str = "a.id,a.meeting_id,a.task,a.owner,a.due,a.segment_refs_json,a.origin,a.completed,a.created_at,a.updated_at";
const SUMMARY_VERSION_COLUMNS: &str = "id,meeting_id,markdown,summary_json,provider,model,cost,prompt_tokens,completion_tokens,origin,created_at";

const MEETING_TEMPLATE_COLUMNS: &str = "id,name,title,agenda,participants_json,ai_model_override,summary_preset_override,summary_instructions,created_at,updated_at";

const MEETING_COLUMNS: &str = "id, title, status, source_app, started_at, ended_at, user_notes, summary_markdown, summary_json, summary_provider, summary_model, summary_cost, prompt_tokens, completion_tokens, is_favorite, tags_json, completed_actions_json, summary_stale, error, deleted_at, created_at, updated_at, ai_model_override, summary_preset_override, summary_instructions, agenda, participants_json, previous_meeting_id, actions_materialized";

pub fn create_meeting(db: &Database, title: &str, source_app: Option<&str>) -> AppResult<Meeting> {
    create_meeting_with_previous(db, title, source_app, None)
}

pub fn create_meeting_with_previous(
    db: &Database,
    title: &str,
    source_app: Option<&str>,
    previous_meeting_id: Option<&str>,
) -> AppResult<Meeting> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let conn = db.conn()?;
    if let Some(previous_id) = previous_meeting_id {
        let exists = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1 AND deleted_at IS NULL)",
            [previous_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(crate::error::AppError::Storage(
                "Previous meeting not found".into(),
            ));
        }
    }
    conn.execute(
        "INSERT INTO meetings (id,title,status,source_app,started_at,created_at,updated_at,previous_meeting_id) VALUES (?1,?2,'recording',?3,?4,?4,?4,?5)",
        params![id, title, source_app, now, previous_meeting_id],
    )?;
    drop(conn);
    get_meeting(db, &id)?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting was not created".into()))
}

pub fn get_meeting(db: &Database, id: &str) -> AppResult<Option<Meeting>> {
    let conn = db.conn()?;
    let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings WHERE id=?1");
    Ok(conn.query_row(&sql, [id], row_to_meeting).optional()?)
}

pub fn list_templates(db: &Database) -> AppResult<Vec<MeetingTemplate>> {
    let conn = db.conn()?;
    let sql = format!(
        "SELECT {MEETING_TEMPLATE_COLUMNS} FROM meeting_templates ORDER BY name COLLATE NOCASE"
    );
    let mut stmt = conn.prepare(&sql)?;
    let templates = stmt
        .query_map([], row_to_meeting_template)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(templates)
}

pub fn save_meeting_as_template(
    db: &Database,
    meeting_id: &str,
    name: &str,
) -> AppResult<MeetingTemplate> {
    let meeting = get_meeting(db, meeting_id)?
        .filter(|meeting| meeting.deleted_at.is_none())
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    let participants = serde_json::to_string(&meeting.participants)
        .map_err(|error| crate::error::AppError::Storage(error.to_string()))?;
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO meeting_templates (id,name,title,agenda,participants_json,ai_model_override,summary_preset_override,summary_instructions,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9) ON CONFLICT(name) DO UPDATE SET name=excluded.name,title=excluded.title,agenda=excluded.agenda,participants_json=excluded.participants_json,ai_model_override=excluded.ai_model_override,summary_preset_override=excluded.summary_preset_override,summary_instructions=excluded.summary_instructions,updated_at=excluded.updated_at",
        params![id, name, meeting.title, meeting.agenda, participants, meeting.ai_model_override, meeting.summary_preset_override, meeting.summary_instructions, now],
    )?;
    let sql = format!(
        "SELECT {MEETING_TEMPLATE_COLUMNS} FROM meeting_templates WHERE name=?1 COLLATE NOCASE"
    );
    Ok(conn.query_row(&sql, [name], row_to_meeting_template)?)
}

pub fn delete_template(db: &Database, id: &str) -> AppResult<()> {
    let deleted = db
        .conn()?
        .execute("DELETE FROM meeting_templates WHERE id=?1", [id])?;
    if deleted == 0 {
        return Err(crate::error::AppError::Storage(
            "Meeting template not found".into(),
        ));
    }
    Ok(())
}

pub fn list_meetings(db: &Database) -> AppResult<Vec<Meeting>> {
    let conn = db.conn()?;
    let sql = format!(
        "SELECT {MEETING_COLUMNS} FROM meetings WHERE deleted_at IS NULL ORDER BY is_favorite DESC, started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let meetings = stmt
        .query_map([], row_to_meeting)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(meetings)
}

pub fn search_meetings(db: &Database, query: &str) -> AppResult<Vec<Meeting>> {
    search_meetings_by_deleted_state(db, query, false)
}

pub fn list_deleted_meetings(db: &Database) -> AppResult<Vec<Meeting>> {
    let conn = db.conn()?;
    let sql = format!(
        "SELECT {MEETING_COLUMNS} FROM meetings WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let meetings = stmt
        .query_map([], row_to_meeting)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(meetings)
}

pub fn search_deleted_meetings(db: &Database, query: &str) -> AppResult<Vec<Meeting>> {
    search_meetings_by_deleted_state(db, query, true)
}

fn search_meetings_by_deleted_state(
    db: &Database,
    query: &str,
    deleted: bool,
) -> AppResult<Vec<Meeting>> {
    let normalized = query.trim();
    if normalized.is_empty() {
        return if deleted {
            list_deleted_meetings(db)
        } else {
            list_meetings(db)
        };
    }
    let conn = db.conn()?;
    let deleted_clause = if deleted {
        "m.deleted_at IS NOT NULL"
    } else {
        "m.deleted_at IS NULL"
    };
    let sql = format!(
        "SELECT {MEETING_COLUMNS} FROM meetings m
         WHERE {deleted_clause} AND (
            m.title LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR COALESCE(m.source_app,'') LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.user_notes LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.summary_markdown LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.tags_json LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR COALESCE(m.ai_model_override,'') LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.summary_instructions LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.agenda LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR m.participants_json LIKE '%' || ?1 || '%' COLLATE NOCASE
            OR EXISTS (SELECT 1 FROM meeting_segments s WHERE s.meeting_id=m.id AND (s.text LIKE '%' || ?1 || '%' COLLATE NOCASE OR COALESCE(s.speaker_label,'') LIKE '%' || ?1 || '%' COLLATE NOCASE))
            OR EXISTS (SELECT 1 FROM meeting_markers k WHERE k.meeting_id=m.id AND k.label LIKE '%' || ?1 || '%' COLLATE NOCASE)
            OR EXISTS (SELECT 1 FROM meeting_questions q WHERE q.meeting_id=m.id AND (q.question LIKE '%' || ?1 || '%' COLLATE NOCASE OR q.answer LIKE '%' || ?1 || '%' COLLATE NOCASE))
            OR EXISTS (SELECT 1 FROM meeting_action_items a WHERE a.meeting_id=m.id AND a.dismissed_at IS NULL AND (a.task LIKE '%' || ?1 || '%' COLLATE NOCASE OR COALESCE(a.owner,'') LIKE '%' || ?1 || '%' COLLATE NOCASE OR COALESCE(a.due,'') LIKE '%' || ?1 || '%' COLLATE NOCASE))
         )
         ORDER BY m.is_favorite DESC, m.started_at DESC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let meetings = stmt
        .query_map([normalized], row_to_meeting)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(meetings)
}

pub fn get_detail(db: &Database, id: &str) -> AppResult<Option<MeetingDetail>> {
    let Some(meeting) = get_meeting(db, id)? else {
        return Ok(None);
    };
    let conn = db.conn()?;
    let mut stmt = conn.prepare("SELECT id,meeting_id,source,start_ms,end_ms,text,speaker_label,created_at FROM meeting_segments WHERE meeting_id=?1 ORDER BY start_ms,source,id")?;
    let segments = stmt
        .query_map([id], |row| {
            Ok(MeetingSegment {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                source: row.get(2)?,
                start_ms: row.get::<_, i64>(3)?.max(0) as u64,
                end_ms: row.get::<_, i64>(4)?.max(0) as u64,
                text: row.get(5)?,
                speaker_label: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut marker_stmt = conn.prepare(
        "SELECT id,meeting_id,at_ms,label,created_at FROM meeting_markers WHERE meeting_id=?1 ORDER BY at_ms,id",
    )?;
    let markers = marker_stmt
        .query_map([id], |row| {
            Ok(MeetingMarker {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                at_ms: row.get::<_, i64>(2)?.max(0) as u64,
                label: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let action_sql = format!(
        "SELECT {ACTION_ITEM_COLUMNS} FROM meeting_action_items WHERE meeting_id=?1 AND dismissed_at IS NULL ORDER BY completed,created_at,id"
    );
    let mut action_stmt = conn.prepare(&action_sql)?;
    let action_items = action_stmt
        .query_map([id], row_to_action_item)?
        .collect::<Result<Vec<_>, _>>()?;
    let mut question_stmt = conn.prepare("SELECT id,meeting_id,question,answer,segment_refs_json,provider,model,cost,prompt_tokens,completion_tokens,context_segments,total_segments,created_at FROM meeting_questions WHERE meeting_id=?1 ORDER BY created_at,id")?;
    let questions = question_stmt
        .query_map([id], |row| {
            Ok(MeetingQuestionAnswer {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                question: row.get(2)?,
                answer: row.get(3)?,
                segment_refs: serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                provider: row.get(5)?,
                model: row.get(6)?,
                cost: row.get(7)?,
                prompt_tokens: row.get::<_, i64>(8)?.max(0) as u64,
                completion_tokens: row.get::<_, i64>(9)?.max(0) as u64,
                context_segments: row.get::<_, i64>(10)?.max(0) as u64,
                total_segments: row.get::<_, i64>(11)?.max(0) as u64,
                created_at: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let transcription = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(CASE WHEN status='done' THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN status='processing' THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN status='failed' THEN 1 ELSE 0 END),0) FROM meeting_chunks WHERE meeting_id=?1",
        [id],
        |row| {
            Ok(MeetingTranscriptionProgress {
                total: row.get::<_, i64>(0)?.max(0) as u64,
                completed: row.get::<_, i64>(1)?.max(0) as u64,
                pending: row.get::<_, i64>(2)?.max(0) as u64,
                processing: row.get::<_, i64>(3)?.max(0) as u64,
                failed: row.get::<_, i64>(4)?.max(0) as u64,
            })
        },
    )?;
    Ok(Some(MeetingDetail {
        meeting,
        segments,
        markers,
        action_items,
        questions,
        transcription,
    }))
}

pub fn update_notes(db: &Database, id: &str, notes: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meetings SET user_notes=?1,summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?2 WHERE id=?3",
        params![notes, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn update_ai_options(
    db: &Database,
    id: &str,
    model: Option<&str>,
    preset: Option<&str>,
    instructions: &str,
) -> AppResult<()> {
    let changed = db.conn()?.execute(
        "UPDATE meetings SET ai_model_override=?1,summary_preset_override=?2,summary_instructions=?3,summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?4 WHERE id=?5 AND deleted_at IS NULL",
        params![model, preset, instructions, Utc::now().to_rfc3339(), id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    Ok(())
}

pub fn update_metadata(
    db: &Database,
    id: &str,
    agenda: Option<&str>,
    participants: Option<&[String]>,
) -> AppResult<()> {
    let conn = db.conn()?;
    let now = Utc::now().to_rfc3339();
    let changed = match (agenda, participants) {
        (Some(agenda), Some(participants)) => conn.execute(
            "UPDATE meetings SET agenda=?1,participants_json=?2,summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?3 WHERE id=?4 AND deleted_at IS NULL",
            params![agenda, serde_json::to_string(participants).map_err(|error| crate::error::AppError::Storage(error.to_string()))?, now, id],
        )?,
        (Some(agenda), None) => conn.execute(
            "UPDATE meetings SET agenda=?1,summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
            params![agenda, now, id],
        )?,
        (None, Some(participants)) => conn.execute(
            "UPDATE meetings SET participants_json=?1,summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?2 WHERE id=?3 AND deleted_at IS NULL",
            params![serde_json::to_string(participants).map_err(|error| crate::error::AppError::Storage(error.to_string()))?, now, id],
        )?,
        (None, None) => return Ok(()),
    };
    if changed == 0 {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    Ok(())
}

pub fn update_title(db: &Database, id: &str, title: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meetings SET title=?1,updated_at=?2 WHERE id=?3",
        params![title, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn set_favorite(db: &Database, id: &str, favorite: bool) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meetings SET is_favorite=?1,updated_at=?2 WHERE id=?3",
        params![favorite, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn set_tags(db: &Database, id: &str, tags: &[String]) -> AppResult<()> {
    let tags = serde_json::to_string(tags)
        .map_err(|error| crate::error::AppError::Storage(error.to_string()))?;
    db.conn()?.execute(
        "UPDATE meetings SET tags_json=?1,updated_at=?2 WHERE id=?3",
        params![tags, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn set_action_completed(
    db: &Database,
    id: &str,
    task: &str,
    completed: bool,
) -> AppResult<Vec<String>> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let encoded = tx
        .query_row(
            "SELECT completed_actions_json FROM meetings WHERE id=?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let mut completed_actions: Vec<String> = serde_json::from_str(&encoded).unwrap_or_default();
    completed_actions.retain(|item| item != task);
    if completed {
        if completed_actions.len() >= 200 {
            return Err(crate::error::AppError::Storage(
                "Too many completed meeting actions".into(),
            ));
        }
        completed_actions.push(task.to_string());
    }
    let encoded = serde_json::to_string(&completed_actions)
        .map_err(|error| crate::error::AppError::Storage(error.to_string()))?;
    tx.execute(
        "UPDATE meetings SET completed_actions_json=?1,updated_at=?2 WHERE id=?3",
        params![encoded, Utc::now().to_rfc3339(), id],
    )?;
    tx.execute(
        "UPDATE meeting_action_items SET completed=?1,updated_at=?2 WHERE meeting_id=?3 AND task=?4 COLLATE NOCASE AND dismissed_at IS NULL",
        params![completed, Utc::now().to_rfc3339(), id, task],
    )?;
    tx.commit()?;
    Ok(completed_actions)
}

pub fn list_action_items(
    db: &Database,
    meeting_id: Option<&str>,
) -> AppResult<Vec<MeetingActionItem>> {
    let conn = db.conn()?;
    let (sql, parameter) = match meeting_id {
        Some(id) => (
            format!("SELECT {QUALIFIED_ACTION_ITEM_COLUMNS} FROM meeting_action_items a JOIN meetings m ON m.id=a.meeting_id WHERE a.meeting_id=?1 AND a.dismissed_at IS NULL AND m.deleted_at IS NULL ORDER BY a.completed,a.created_at,a.id"),
            Some(id),
        ),
        None => (
            format!("SELECT {QUALIFIED_ACTION_ITEM_COLUMNS} FROM meeting_action_items a JOIN meetings m ON m.id=a.meeting_id WHERE a.dismissed_at IS NULL AND m.deleted_at IS NULL ORDER BY a.completed,m.started_at DESC,a.created_at,a.id"),
            None,
        ),
    };
    let mut stmt = conn.prepare(&sql)?;
    let rows = match parameter {
        Some(value) => stmt
            .query_map([value], row_to_action_item)?
            .collect::<Result<Vec<_>, _>>()?,
        None => stmt
            .query_map([], row_to_action_item)?
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(rows)
}

pub fn add_action_item(
    db: &Database,
    meeting_id: &str,
    task: &str,
    owner: Option<&str>,
    due: Option<&str>,
) -> AppResult<MeetingActionItem> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let exists = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1 AND deleted_at IS NULL)",
        [meeting_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let count = tx.query_row(
        "SELECT COUNT(*) FROM meeting_action_items WHERE meeting_id=?1 AND dismissed_at IS NULL",
        [meeting_id],
        |row| row.get::<_, i64>(0),
    )?;
    if count >= 500 {
        return Err(crate::error::AppError::Storage(
            "Too many meeting action items".into(),
        ));
    }
    let duplicate = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM meeting_action_items WHERE meeting_id=?1 AND task=?2 COLLATE NOCASE AND dismissed_at IS NULL)",
        params![meeting_id, task],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(crate::error::AppError::Storage(
            "That meeting action item already exists".into(),
        ));
    }
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO meeting_action_items (id,meeting_id,task,owner,due,segment_refs_json,origin,source_task,completed,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,'[]','manual',NULL,0,?6,?6)",
        params![id, meeting_id, task, owner, due, now],
    )?;
    let sql = format!("SELECT {ACTION_ITEM_COLUMNS} FROM meeting_action_items WHERE id=?1");
    let item = tx.query_row(&sql, [&id], row_to_action_item)?;
    tx.commit()?;
    Ok(item)
}

pub fn update_action_item(
    db: &Database,
    id: &str,
    meeting_id: &str,
    task: &str,
    owner: Option<&str>,
    due: Option<&str>,
) -> AppResult<MeetingActionItem> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    let (old_task, completed) = tx
        .query_row(
            "SELECT task,completed FROM meeting_action_items WHERE id=?1 AND meeting_id=?2 AND dismissed_at IS NULL",
            params![id, meeting_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0)),
        )
        .optional()?
        .ok_or_else(|| {
            crate::error::AppError::Storage("Meeting action item not found".into())
        })?;
    let duplicate = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM meeting_action_items WHERE meeting_id=?1 AND id<>?2 AND task=?3 COLLATE NOCASE AND dismissed_at IS NULL)",
        params![meeting_id, id, task],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(crate::error::AppError::Storage(
            "That meeting action item already exists".into(),
        ));
    }
    let changed = tx.execute(
        "UPDATE meeting_action_items SET task=?1,owner=?2,due=?3,origin='manual',dismissed_at=NULL,updated_at=?4 WHERE id=?5 AND meeting_id=?6 AND dismissed_at IS NULL",
        params![task, owner, due, now, id, meeting_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage(
            "Meeting action item not found".into(),
        ));
    }
    if completed && !old_task.eq_ignore_ascii_case(task) {
        update_legacy_completed_actions(&tx, meeting_id, &old_task, false)?;
        update_legacy_completed_actions(&tx, meeting_id, task, true)?;
    }
    let sql = format!("SELECT {ACTION_ITEM_COLUMNS} FROM meeting_action_items WHERE id=?1");
    let item = tx.query_row(&sql, [id], row_to_action_item)?;
    tx.commit()?;
    Ok(item)
}

pub fn set_action_item_completed(
    db: &Database,
    id: &str,
    meeting_id: &str,
    completed: bool,
) -> AppResult<MeetingActionItem> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    let task = tx
        .query_row(
            "SELECT task FROM meeting_action_items WHERE id=?1 AND meeting_id=?2 AND dismissed_at IS NULL",
            params![id, meeting_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            crate::error::AppError::Storage("Meeting action item not found".into())
        })?;
    tx.execute(
        "UPDATE meeting_action_items SET completed=?1,updated_at=?2 WHERE id=?3 AND meeting_id=?4",
        params![completed, now, id, meeting_id],
    )?;
    update_legacy_completed_actions(&tx, meeting_id, &task, completed)?;
    let sql = format!("SELECT {ACTION_ITEM_COLUMNS} FROM meeting_action_items WHERE id=?1");
    let item = tx.query_row(&sql, [id], row_to_action_item)?;
    tx.commit()?;
    Ok(item)
}

pub fn delete_action_item(db: &Database, id: &str, meeting_id: &str) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let (source_task, task) = tx
        .query_row(
            "SELECT source_task,task FROM meeting_action_items WHERE id=?1 AND meeting_id=?2 AND dismissed_at IS NULL",
            params![id, meeting_id],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            crate::error::AppError::Storage("Meeting action item not found".into())
        })?;
    if source_task.is_some() {
        tx.execute(
            "UPDATE meeting_action_items SET dismissed_at=?1,updated_at=?1 WHERE id=?2 AND meeting_id=?3",
            params![Utc::now().to_rfc3339(), id, meeting_id],
        )?;
    } else {
        tx.execute(
            "DELETE FROM meeting_action_items WHERE id=?1 AND meeting_id=?2",
            params![id, meeting_id],
        )?;
    }
    update_legacy_completed_actions(&tx, meeting_id, &task, false)?;
    tx.commit()?;
    Ok(())
}

fn update_legacy_completed_actions(
    tx: &Transaction<'_>,
    meeting_id: &str,
    task: &str,
    completed: bool,
) -> AppResult<()> {
    let encoded = tx.query_row(
        "SELECT completed_actions_json FROM meetings WHERE id=?1",
        [meeting_id],
        |row| row.get::<_, String>(0),
    )?;
    let mut actions: Vec<String> = serde_json::from_str(&encoded).unwrap_or_default();
    actions.retain(|value| !value.eq_ignore_ascii_case(task));
    if completed {
        actions.push(task.to_string());
    }
    tx.execute(
        "UPDATE meetings SET completed_actions_json=?1,updated_at=?2 WHERE id=?3",
        params![
            serde_json::to_string(&actions)
                .map_err(|error| crate::error::AppError::Storage(error.to_string()))?,
            Utc::now().to_rfc3339(),
            meeting_id
        ],
    )?;
    Ok(())
}

pub fn update_segment_text(db: &Database, id: &str, meeting_id: &str, text: &str) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let changed = tx.execute(
        "UPDATE meeting_segments SET text=?1 WHERE id=?2 AND meeting_id=?3",
        params![text.trim(), id, meeting_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage(
            "Meeting transcript segment not found".into(),
        ));
    }
    tx.execute(
        "UPDATE meetings SET summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), meeting_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn update_segment_speaker(
    db: &Database,
    id: &str,
    meeting_id: &str,
    label: Option<&str>,
    apply_to_source: bool,
) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let source = tx
        .query_row(
            "SELECT source FROM meeting_segments WHERE id=?1 AND meeting_id=?2",
            params![id, meeting_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| crate::error::AppError::Storage("Transcript segment not found".into()))?;
    let label = label.map(str::trim).filter(|value| !value.is_empty());
    if apply_to_source {
        tx.execute(
            "UPDATE meeting_segments SET speaker_label=?1 WHERE meeting_id=?2 AND source=?3",
            params![label, meeting_id, source],
        )?;
    } else {
        tx.execute(
            "UPDATE meeting_segments SET speaker_label=?1 WHERE id=?2 AND meeting_id=?3",
            params![label, id, meeting_id],
        )?;
    }
    tx.execute(
        "UPDATE meetings SET summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), meeting_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn add_marker(
    db: &Database,
    meeting_id: &str,
    at_ms: u64,
    label: &str,
) -> AppResult<MeetingMarker> {
    let label = label.trim();
    let marker = MeetingMarker {
        id: Uuid::new_v4().to_string(),
        meeting_id: meeting_id.into(),
        at_ms,
        label: if label.is_empty() {
            "Important moment".into()
        } else {
            label.chars().take(160).collect()
        },
        created_at: Utc::now().to_rfc3339(),
    };
    db.conn()?.execute(
        "INSERT INTO meeting_markers (id,meeting_id,at_ms,label,created_at) VALUES (?1,?2,?3,?4,?5)",
        params![marker.id, marker.meeting_id, marker.at_ms as i64, marker.label, marker.created_at],
    )?;
    db.conn()?.execute(
        "UPDATE meetings SET summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), meeting_id],
    )?;
    Ok(marker)
}

pub fn update_marker(db: &Database, id: &str, meeting_id: &str, label: &str) -> AppResult<()> {
    let label = label.trim().chars().take(160).collect::<String>();
    let changed = db.conn()?.execute(
        "UPDATE meeting_markers SET label=?1 WHERE id=?2 AND meeting_id=?3",
        params![label, id, meeting_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage(
            "Meeting marker not found".into(),
        ));
    }
    db.conn()?.execute(
        "UPDATE meetings SET summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), meeting_id],
    )?;
    Ok(())
}

pub fn delete_marker(db: &Database, id: &str, meeting_id: &str) -> AppResult<()> {
    let changed = db.conn()?.execute(
        "DELETE FROM meeting_markers WHERE id=?1 AND meeting_id=?2",
        params![id, meeting_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage(
            "Meeting marker not found".into(),
        ));
    }
    db.conn()?.execute(
        "UPDATE meetings SET summary_stale=CASE WHEN summary_markdown<>'' THEN 1 ELSE summary_stale END,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), meeting_id],
    )?;
    Ok(())
}

const MAX_SUMMARY_VERSIONS_PER_MEETING: i64 = 50;

fn insert_summary_version(
    tx: &Transaction<'_>,
    meeting_id: &str,
    markdown: &str,
    summary_json: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    cost: Option<f64>,
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    origin: &str,
    created_at: &str,
) -> AppResult<MeetingSummaryVersion> {
    let version = MeetingSummaryVersion {
        id: Uuid::new_v4().to_string(),
        meeting_id: meeting_id.to_string(),
        markdown: markdown.to_string(),
        summary_json: summary_json.map(str::to_string),
        provider: provider.map(str::to_string),
        model: model.map(str::to_string),
        cost,
        prompt_tokens,
        completion_tokens,
        origin: origin.to_string(),
        created_at: created_at.to_string(),
    };
    tx.execute(
        "INSERT INTO meeting_summary_versions (id,meeting_id,markdown,summary_json,provider,model,cost,prompt_tokens,completion_tokens,origin,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            version.id,
            version.meeting_id,
            version.markdown,
            version.summary_json,
            version.provider,
            version.model,
            version.cost,
            version.prompt_tokens.map(|value| value as i64),
            version.completion_tokens.map(|value| value as i64),
            version.origin,
            version.created_at,
        ],
    )?;
    tx.execute(
        "DELETE FROM meeting_summary_versions WHERE meeting_id=?1 AND id NOT IN (SELECT id FROM meeting_summary_versions WHERE meeting_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT ?2)",
        params![meeting_id, MAX_SUMMARY_VERSIONS_PER_MEETING],
    )?;
    Ok(version)
}

fn snapshot_current_summary_if_changed(
    tx: &Transaction<'_>,
    meeting_id: &str,
    created_at: &str,
) -> AppResult<()> {
    type SummarySnapshot = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<i64>,
        Option<i64>,
    );
    let current: SummarySnapshot = tx
        .query_row(
            "SELECT summary_markdown,summary_json,summary_provider,summary_model,summary_cost,prompt_tokens,completion_tokens FROM meetings WHERE id=?1 AND deleted_at IS NULL",
            [meeting_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<i64>>(5)?, row.get::<_, Option<i64>>(6)?)),
        )
        .optional()?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    if current.0.trim().is_empty() {
        return Ok(());
    }
    let latest: Option<SummarySnapshot> = tx
        .query_row(
            "SELECT markdown,summary_json,provider,model,cost,prompt_tokens,completion_tokens FROM meeting_summary_versions WHERE meeting_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT 1",
            [meeting_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<f64>>(4)?, row.get::<_, Option<i64>>(5)?, row.get::<_, Option<i64>>(6)?)),
        )
        .optional()?;
    if latest.as_ref() == Some(&current) {
        return Ok(());
    }
    let origin = if current.1.is_some() { "ai" } else { "manual" };
    insert_summary_version(
        tx,
        meeting_id,
        &current.0,
        current.1.as_deref(),
        current.2.as_deref(),
        current.3.as_deref(),
        current.4,
        current.5.map(|value| value.max(0) as u64),
        current.6.map(|value| value.max(0) as u64),
        origin,
        created_at,
    )?;
    Ok(())
}

pub fn list_summary_versions(
    db: &Database,
    meeting_id: &str,
) -> AppResult<Vec<MeetingSummaryVersion>> {
    let conn = db.conn()?;
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1 AND deleted_at IS NULL)",
        [meeting_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let sql = format!(
        "SELECT {SUMMARY_VERSION_COLUMNS} FROM meeting_summary_versions WHERE meeting_id=?1 ORDER BY created_at DESC,rowid DESC LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let versions = stmt
        .query_map(
            params![meeting_id, MAX_SUMMARY_VERSIONS_PER_MEETING],
            row_to_summary_version,
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(versions)
}

pub fn list_recovery_items(db: &Database) -> AppResult<Vec<MeetingRecoveryItem>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT m.id,m.title,m.status,
            COALESCE(SUM(CASE WHEN c.status='pending' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN c.status='processing' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN c.status='failed' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN c.status='failed' AND c.audio_path<>'' THEN 1 ELSE 0 END),0),
            COALESCE(SUM(CASE WHEN c.status='done' THEN 1 ELSE 0 END),0),
            CASE WHEN m.summary_markdown<>'' THEN 1 ELSE 0 END,
            m.error,m.updated_at
         FROM meetings m
         LEFT JOIN meeting_chunks c ON c.meeting_id=m.id
         WHERE m.deleted_at IS NULL
           AND (m.status IN ('transcribing','interrupted','failed','error') OR m.error IS NOT NULL)
         GROUP BY m.id
         ORDER BY CASE m.status WHEN 'error' THEN 0 WHEN 'failed' THEN 1 WHEN 'interrupted' THEN 2 WHEN 'transcribing' THEN 3 ELSE 4 END,m.updated_at DESC",
    )?;
    let items = stmt
        .query_map([], |row| {
            Ok(MeetingRecoveryItem {
                meeting_id: row.get(0)?,
                title: row.get(1)?,
                status: row.get(2)?,
                pending_chunks: row.get::<_, i64>(3)?.max(0) as u64,
                processing_chunks: row.get::<_, i64>(4)?.max(0) as u64,
                failed_chunks: row.get::<_, i64>(5)?.max(0) as u64,
                recoverable_failed_chunks: row.get::<_, i64>(6)?.max(0) as u64,
                completed_chunks: row.get::<_, i64>(7)?.max(0) as u64,
                has_summary: row.get::<_, i64>(8)? != 0,
                error: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

pub fn restore_summary_version(
    db: &Database,
    id: &str,
    meeting_id: &str,
) -> AppResult<MeetingSummaryVersion> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let sql = format!(
        "SELECT {SUMMARY_VERSION_COLUMNS} FROM meeting_summary_versions WHERE id=?1 AND meeting_id=?2"
    );
    let selected = tx
        .query_row(&sql, params![id, meeting_id], row_to_summary_version)
        .optional()?
        .ok_or_else(|| crate::error::AppError::Storage("Summary version not found".into()))?;
    let now = Utc::now().to_rfc3339();
    snapshot_current_summary_if_changed(&tx, meeting_id, &now)?;
    let changed = tx.execute(
        "UPDATE meetings SET status='ready',summary_markdown=?1,summary_json=?2,summary_provider=?3,summary_model=?4,summary_cost=?5,prompt_tokens=?6,completion_tokens=?7,summary_stale=1,error=NULL,actions_materialized=1,updated_at=?8 WHERE id=?9 AND deleted_at IS NULL",
        params![selected.markdown, selected.summary_json, selected.provider, selected.model, selected.cost, selected.prompt_tokens.map(|value| value as i64), selected.completion_tokens.map(|value| value as i64), now, meeting_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let restored = insert_summary_version(
        &tx,
        meeting_id,
        &selected.markdown,
        selected.summary_json.as_deref(),
        selected.provider.as_deref(),
        selected.model.as_deref(),
        selected.cost,
        selected.prompt_tokens,
        selected.completion_tokens,
        "restored",
        &now,
    )?;
    tx.commit()?;
    Ok(restored)
}

pub fn update_summary_markdown(db: &Database, id: &str, markdown: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meetings SET summary_markdown=?1,summary_json=NULL,actions_materialized=1,updated_at=?2 WHERE id=?3",
        params![markdown, Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn set_status(
    db: &Database,
    id: &str,
    status: &str,
    error: Option<&str>,
    ended: bool,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.conn()?.execute(
        "UPDATE meetings SET status=?1,error=?2,ended_at=CASE WHEN ?3 THEN COALESCE(ended_at,?4) ELSE ended_at END,updated_at=?4 WHERE id=?5",
        params![status,error,ended,now,id],
    )?;
    Ok(())
}

pub fn begin_summary(db: &Database, id: &str) -> AppResult<bool> {
    let changed = db.conn()?.execute(
        "UPDATE meetings SET status='summarizing',error=NULL,updated_at=?1 WHERE id=?2 AND status<>'summarizing' AND deleted_at IS NULL",
        params![Utc::now().to_rfc3339(), id],
    )?;
    Ok(changed == 1)
}

pub fn add_chunk(
    db: &Database,
    meeting_id: &str,
    source: &str,
    start_ms: u64,
    end_ms: u64,
    path: &str,
) -> AppResult<MeetingChunk> {
    let chunk = MeetingChunk {
        id: Uuid::new_v4().to_string(),
        meeting_id: meeting_id.into(),
        source: source.into(),
        start_ms,
        end_ms,
        audio_path: path.into(),
    };
    db.conn()?.execute("INSERT INTO meeting_chunks (id,meeting_id,source,start_ms,end_ms,audio_path,status,created_at) VALUES (?1,?2,?3,?4,?5,?6,'pending',?7)", params![chunk.id,chunk.meeting_id,chunk.source,chunk.start_ms as i64,chunk.end_ms as i64,chunk.audio_path,Utc::now().to_rfc3339()])?;
    Ok(chunk)
}

pub fn claim_next_pending_chunk(
    db: &Database,
    meeting_id: &str,
) -> AppResult<Option<MeetingChunk>> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let chunk = tx.query_row("SELECT id,meeting_id,source,start_ms,end_ms,audio_path FROM meeting_chunks WHERE meeting_id=?1 AND status='pending' ORDER BY start_ms,source LIMIT 1", [meeting_id], |row| Ok(MeetingChunk { id: row.get(0)?, meeting_id: row.get(1)?, source: row.get(2)?, start_ms: row.get::<_,i64>(3)?.max(0) as u64, end_ms: row.get::<_,i64>(4)?.max(0) as u64, audio_path: row.get(5)? })).optional()?;
    if let Some(chunk) = &chunk {
        tx.execute(
            "UPDATE meeting_chunks SET status='processing',error=NULL WHERE id=?1 AND status='pending'",
            [&chunk.id],
        )?;
    }
    tx.commit()?;
    Ok(chunk)
}

pub fn requeue_chunk(db: &Database, id: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meeting_chunks SET status='pending' WHERE id=?1 AND status='processing'",
        [id],
    )?;
    Ok(())
}

pub fn complete_chunk(
    db: &Database,
    chunk: &MeetingChunk,
    segments: &[(u64, u64, String)],
) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    for (start, end, text) in segments {
        if text.trim().is_empty() {
            continue;
        }
        tx.execute("INSERT INTO meeting_segments (id,meeting_id,chunk_id,source,start_ms,end_ms,text,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)", params![Uuid::new_v4().to_string(),chunk.meeting_id,chunk.id,chunk.source,(chunk.start_ms + *start) as i64,(chunk.start_ms + *end) as i64,text.trim(),Utc::now().to_rfc3339()])?;
    }
    tx.execute(
        "UPDATE meeting_chunks SET status='done',audio_path='' WHERE id=?1",
        [&chunk.id],
    )?;
    tx.execute(
        "UPDATE meetings SET updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), chunk.meeting_id],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn fail_chunk(db: &Database, id: &str, error: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meeting_chunks SET status='failed',error=?1 WHERE id=?2",
        params![error, id],
    )?;
    Ok(())
}

pub fn pending_count(db: &Database, meeting_id: &str) -> AppResult<u64> {
    Ok(db
        .conn()?
        .query_row(
            "SELECT COUNT(*) FROM meeting_chunks WHERE meeting_id=?1 AND status IN ('pending','processing')",
            [meeting_id],
            |row| row.get::<_, i64>(0),
        )?
        .max(0) as u64)
}

pub fn retry_failed_chunks(db: &Database, meeting_id: &str) -> AppResult<u64> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let exists = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1 AND deleted_at IS NULL)",
        [meeting_id],
        |row| row.get::<_, i64>(0),
    )? != 0;
    if !exists {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let retried = tx.execute(
        "UPDATE meeting_chunks SET status='pending',error=NULL WHERE meeting_id=?1 AND status='failed' AND audio_path<>''",
        [meeting_id],
    )? as u64;
    if retried > 0 {
        tx.execute(
            "UPDATE meetings SET status='transcribing',error=NULL,updated_at=?1 WHERE id=?2",
            params![Utc::now().to_rfc3339(), meeting_id],
        )?;
    }
    tx.commit()?;
    Ok(retried)
}

pub fn failed_count(db: &Database, meeting_id: &str) -> AppResult<u64> {
    Ok(db
        .conn()?
        .query_row(
            "SELECT COUNT(*) FROM meeting_chunks WHERE meeting_id=?1 AND status='failed'",
            [meeting_id],
            |row| row.get::<_, i64>(0),
        )?
        .max(0) as u64)
}

pub fn transcript_text(db: &Database, meeting_id: &str) -> AppResult<String> {
    let conn = db.conn()?;
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1)",
        [meeting_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let mut stmt = conn.prepare(
        "SELECT source,start_ms,text,speaker_label FROM meeting_segments WHERE meeting_id=?1 ORDER BY start_ms,source,id",
    )?;
    let lines = stmt
        .query_map([meeting_id], |row| {
            let source = row.get::<_, String>(0)?;
            let start_ms = row.get::<_, i64>(1)?.max(0) as u64;
            let text = row.get::<_, String>(2)?;
            let speaker_label = row.get::<_, Option<String>>(3)?;
            Ok(format!(
                "[{}] {}: {}",
                format_timestamp(start_ms),
                speaker_label
                    .as_deref()
                    .filter(|label| !label.trim().is_empty())
                    .unwrap_or(if source == "mic" { "You" } else { "Meeting" }),
                text
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lines.join("\n"))
}

pub fn markers_text(db: &Database, meeting_id: &str) -> AppResult<String> {
    let conn = db.conn()?;
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM meetings WHERE id=?1)",
        [meeting_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(crate::error::AppError::Storage("Meeting not found".into()));
    }
    let mut stmt = conn
        .prepare("SELECT at_ms,label FROM meeting_markers WHERE meeting_id=?1 ORDER BY at_ms,id")?;
    let lines = stmt
        .query_map([meeting_id], |row| {
            let at_ms = row.get::<_, i64>(0)?.max(0) as u64;
            let label = row.get::<_, String>(1)?;
            Ok(format!(
                "[{}] {}",
                format_timestamp(at_ms),
                if label.is_empty() {
                    "Important moment"
                } else {
                    &label
                }
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lines.join("\n"))
}

pub fn export_markdown(db: &Database, meeting_id: &str) -> AppResult<String> {
    let detail = get_detail(db, meeting_id)?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let mut out = format!("# {}\n\n", detail.meeting.title);
    out.push_str(&format!("- Started: {}\n", detail.meeting.started_at));
    if let Some(ended) = &detail.meeting.ended_at {
        out.push_str(&format!("- Ended: {ended}\n"));
    }
    if let Some(source) = &detail.meeting.source_app {
        out.push_str(&format!("- Source: {source}\n"));
    }
    if !detail.meeting.participants.is_empty() {
        out.push_str(&format!(
            "- Participants: {}\n",
            detail.meeting.participants.join(", ")
        ));
    }
    if !detail.meeting.agenda.trim().is_empty() {
        out.push_str(&format!("- Agenda: {}\n", detail.meeting.agenda.trim()));
    }
    if let Some(previous_id) = &detail.meeting.previous_meeting_id {
        if let Some(previous) = get_meeting(db, previous_id)? {
            out.push_str(&format!(
                "- Continued from: {} ({})\n",
                previous.title,
                previous
                    .started_at
                    .split('T')
                    .next()
                    .unwrap_or(&previous.started_at)
            ));
        }
    }
    if !detail.meeting.tags.is_empty() {
        out.push_str(&format!("- Tags: {}\n", detail.meeting.tags.join(", ")));
    }
    if let Some(model) = &detail.meeting.summary_model {
        out.push_str(&format!("- Summary model: {model}\n"));
    }
    if let Some(cost) = detail.meeting.summary_cost {
        out.push_str(&format!("- Summary cost: ${cost:.6}\n"));
    }
    if !detail.meeting.summary_markdown.trim().is_empty() {
        out.push_str("\n");
        out.push_str(detail.meeting.summary_markdown.trim());
        out.push_str("\n");
    }
    append_action_items(
        &mut out,
        "Managed follow-ups (authoritative)",
        &detail.action_items,
        true,
    );
    if !detail.meeting.user_notes.trim().is_empty() {
        out.push_str("\n## My notes\n\n");
        out.push_str(detail.meeting.user_notes.trim());
        out.push_str("\n");
    }
    if !detail.markers.is_empty() {
        out.push_str("\n## Important moments\n\n");
        for marker in &detail.markers {
            let label = if marker.label.is_empty() {
                "Important moment"
            } else {
                &marker.label
            };
            out.push_str(&format!("- [{}] {label}\n", format_timestamp(marker.at_ms)));
        }
    }
    if !detail.questions.is_empty() {
        out.push_str("\n## Questions and answers\n\n");
        for exchange in &detail.questions {
            out.push_str(&format!(
                "### {}\n\n{}",
                exchange.question.trim(),
                exchange.answer.trim()
            ));
            if !exchange.segment_refs.is_empty() {
                out.push_str(&format!("\n\nSources: {}", exchange.segment_refs.join(" ")));
            }
            if exchange.total_segments > 0 {
                let coverage = if exchange.context_segments == exchange.total_segments {
                    "full transcript".to_string()
                } else {
                    format!(
                        "{} of {} transcript excerpts",
                        exchange.context_segments, exchange.total_segments
                    )
                };
                out.push_str(&format!("\n\nContext: {coverage}"));
            }
            out.push_str("\n\n");
        }
    }
    let versions = list_summary_versions(db, meeting_id)?;
    if versions.len() > 1 {
        out.push_str("\n## Notes version history (private archive)\n\n");
        for version in versions {
            let label = match version.origin.as_str() {
                "manual" => "Edited snapshot",
                "restored" => "Restored snapshot",
                _ => "AI-generated snapshot",
            };
            out.push_str(&format!("### {} - {}\n\n", version.created_at, label));
            if let Some(model) = version.model.as_deref() {
                out.push_str(&format!("- Model: {model}\n"));
            }
            if let Some(cost) = version.cost {
                out.push_str(&format!("- Cost: ${cost:.6}\n"));
            }
            out.push_str("\n");
            for line in version.markdown.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push_str("\n");
            }
            out.push_str("\n");
        }
    }
    if !detail.segments.is_empty() {
        out.push_str("\n## Transcript\n\n");
        for segment in &detail.segments {
            let speaker = segment_speaker(segment);
            out.push_str(&format!(
                "[{}] **{speaker}:** {}\n\n",
                format_timestamp(segment.start_ms),
                segment.text.trim()
            ));
        }
    }
    Ok(out.trim_end().to_string() + "\n")
}

/// Export only the polished, shareable meeting notes. This intentionally omits the
/// raw transcript, scratchpad, markers, private Q&A, provider details, cost, and
/// internal continuity identifiers that are present in the full archive exports.
pub fn export_polished_notes(db: &Database, meeting_id: &str) -> AppResult<String> {
    let detail = get_detail(db, meeting_id)?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let mut out = format!("# {}\n\n", detail.meeting.title);
    out.push_str(&format!(
        "- Date: {}\n",
        detail
            .meeting
            .started_at
            .split('T')
            .next()
            .unwrap_or(&detail.meeting.started_at)
    ));
    if !detail.meeting.participants.is_empty() {
        out.push_str(&format!(
            "- Participants: {}\n",
            detail.meeting.participants.join(", ")
        ));
    }
    if !detail.meeting.agenda.trim().is_empty() {
        out.push_str(&format!("- Agenda: {}\n", detail.meeting.agenda.trim()));
    }

    if detail.meeting.summary_markdown.trim().is_empty() {
        out.push_str("\n_No polished AI notes are available for this meeting._\n");
    } else {
        out.push('\n');
        let summary = if detail.meeting.summary_json.is_some() {
            without_markdown_section(detail.meeting.summary_markdown.trim(), "Action items")
        } else {
            detail.meeting.summary_markdown.trim().to_string()
        };
        out.push_str(&summary_with_completed_actions(
            summary.trim(),
            &detail.meeting.completed_actions,
        ));
        out.push('\n');
    }
    append_action_items(&mut out, "Current follow-ups", &detail.action_items, false);
    Ok(out)
}

fn without_markdown_section(markdown: &str, heading: &str) -> String {
    let target = format!("## {}", heading.trim()).to_lowercase();
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.to_lowercase() == target {
            skipping = true;
            continue;
        }
        if skipping && trimmed.starts_with("## ") {
            skipping = false;
        }
        if !skipping {
            kept.push(line);
        }
    }
    kept.join("\n").trim().to_string()
}

fn append_action_items(
    out: &mut String,
    title: &str,
    items: &[MeetingActionItem],
    include_references: bool,
) {
    if items.is_empty() {
        return;
    }
    out.push_str(&format!("\n## {title}\n\n"));
    for item in items {
        out.push_str(if item.completed { "- [x] " } else { "- [ ] " });
        out.push_str(item.task.trim());
        let mut metadata = Vec::new();
        if let Some(owner) = item
            .owner
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            metadata.push(format!("Owner: {}", owner.trim()));
        }
        if let Some(due) = item.due.as_deref().filter(|value| !value.trim().is_empty()) {
            metadata.push(format!("Due: {}", due.trim()));
        }
        if !metadata.is_empty() {
            out.push_str(" - ");
            out.push_str(&metadata.join(" | "));
        }
        if include_references && !item.segment_refs.is_empty() {
            out.push(' ');
            out.push_str(&item.segment_refs.join(" "));
        }
        out.push('\n');
    }
}

fn summary_with_completed_actions(markdown: &str, completed_actions: &[String]) -> String {
    let mut rendered = markdown.to_string();
    for task in completed_actions {
        let unchecked_with_details = format!("- [ ] {task} —");
        let checked_with_details = format!("- [x] {task} —");
        rendered = rendered.replace(&unchecked_with_details, &checked_with_details);
        let unchecked_line = format!("- [ ] {task}\n");
        let checked_line = format!("- [x] {task}\n");
        rendered = rendered.replace(&unchecked_line, &checked_line);
        if rendered.ends_with(&format!("- [ ] {task}")) {
            let prefix_len = rendered.len() - format!("- [ ] {task}").len();
            rendered.truncate(prefix_len);
            rendered.push_str(&format!("- [x] {task}"));
        }
    }
    rendered
}

pub fn export_webvtt(db: &Database, meeting_id: &str) -> AppResult<String> {
    let detail = get_detail(db, meeting_id)?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let mut out = String::from("WEBVTT\n\n");
    for segment in detail.segments {
        let speaker = segment_speaker(&segment);
        out.push_str(&format!(
            "{} --> {}\n{speaker}: {}\n\n",
            format_caption_timestamp(segment.start_ms, '.'),
            format_caption_timestamp(segment.end_ms.max(segment.start_ms + 1), '.'),
            segment.text.trim().replace("-->", "→")
        ));
    }
    Ok(out)
}

pub fn export_srt(db: &Database, meeting_id: &str) -> AppResult<String> {
    let detail = get_detail(db, meeting_id)?
        .ok_or_else(|| crate::error::AppError::Storage("Meeting not found".into()))?;
    let mut out = String::new();
    for (index, segment) in detail.segments.into_iter().enumerate() {
        let speaker = segment_speaker(&segment);
        out.push_str(&format!(
            "{}\n{} --> {}\n{speaker}: {}\n\n",
            index + 1,
            format_caption_timestamp(segment.start_ms, ','),
            format_caption_timestamp(segment.end_ms.max(segment.start_ms + 1), ','),
            segment.text.trim()
        ));
    }
    Ok(out)
}

fn format_caption_timestamp(ms: u64, separator: char) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms % 3_600_000) / 60_000;
    let seconds = (ms % 60_000) / 1_000;
    let millis = ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{millis:03}")
}

fn segment_speaker(segment: &MeetingSegment) -> &str {
    segment
        .speaker_label
        .as_deref()
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(if segment.source == "mic" {
            "You"
        } else {
            "Meeting"
        })
}

fn format_timestamp(ms: u64) -> String {
    let total = ms / 1000;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

pub fn save_summary(
    db: &Database,
    id: &str,
    markdown: &str,
    json: &str,
    provider: &str,
    model: &str,
    cost: f64,
    prompt_tokens: u64,
    completion_tokens: u64,
) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    snapshot_current_summary_if_changed(&tx, id, &now)?;
    sync_generated_action_items(&tx, id, json, &now)?;
    tx.execute(
        "INSERT INTO meeting_ai_usage (id,meeting_id,provider,model,cost,prompt_tokens,completion_tokens,request_kind,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,'summary',?8)",
        params![Uuid::new_v4().to_string(),id,provider,model,cost,prompt_tokens as i64,completion_tokens as i64,now],
    )?;
    tx.execute("UPDATE meetings SET status='ready',summary_markdown=?1,summary_json=?2,summary_provider=?3,summary_model=?4,summary_cost=?5,prompt_tokens=?6,completion_tokens=?7,summary_stale=0,error=NULL,actions_materialized=1,updated_at=?8 WHERE id=?9", params![markdown,json,provider,model,cost,prompt_tokens as i64,completion_tokens as i64,now,id])?;
    insert_summary_version(
        &tx,
        id,
        markdown,
        Some(json),
        Some(provider),
        Some(model),
        Some(cost),
        Some(prompt_tokens),
        Some(completion_tokens),
        "ai",
        &now,
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug)]
struct GeneratedActionItem {
    task: String,
    owner: Option<String>,
    due: Option<String>,
    segment_refs: Vec<String>,
}

fn generated_action_items(json: &str) -> Vec<GeneratedActionItem> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(items) = value
        .get("action_items")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    items
        .iter()
        .filter_map(|item| {
            let task = bounded_text(item.get("task")?.as_str()?, 500);
            let key = task.to_lowercase();
            if task.is_empty() || !seen.insert(key) {
                return None;
            }
            let owner = item
                .get("owner")
                .and_then(serde_json::Value::as_str)
                .map(|value| bounded_text(value, 200))
                .filter(|value| !value.is_empty());
            let due = item
                .get("due")
                .and_then(serde_json::Value::as_str)
                .map(|value| bounded_text(value, 200))
                .filter(|value| !value.is_empty());
            let segment_refs = item
                .get("segment_refs")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str)
                .map(|value| bounded_text(value, 32))
                .filter(|value| !value.is_empty())
                .take(50)
                .collect();
            Some(GeneratedActionItem {
                task,
                owner,
                due,
                segment_refs,
            })
        })
        .take(200)
        .collect()
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.trim().chars().take(max_chars).collect()
}

fn sync_generated_action_items(
    tx: &Transaction<'_>,
    meeting_id: &str,
    json: &str,
    now: &str,
) -> AppResult<()> {
    let legacy_completed = tx
        .query_row(
            "SELECT completed_actions_json FROM meetings WHERE id=?1",
            [meeting_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .unwrap_or_else(|| "[]".into());
    let legacy_completed: Vec<String> = serde_json::from_str(&legacy_completed).unwrap_or_default();

    let existing = {
        let mut stmt = tx.prepare(
            "SELECT id,task,origin,dismissed_at,COALESCE(source_task,task) FROM meeting_action_items WHERE meeting_id=?1 AND source_task IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([meeting_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut by_task: HashMap<String, (String, String, String, Option<String>)> = existing
        .into_iter()
        .map(|(id, task, origin, dismissed_at, source_task)| {
            (source_task.to_lowercase(), (id, task, origin, dismissed_at))
        })
        .collect();
    let manual_tasks = {
        let mut stmt = tx.prepare(
            "SELECT task FROM meeting_action_items WHERE meeting_id=?1 AND origin='manual' AND dismissed_at IS NULL",
        )?;
        let tasks = stmt
            .query_map([meeting_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        tasks
            .into_iter()
            .map(|task| task.to_lowercase())
            .collect::<HashSet<_>>()
    };

    for item in generated_action_items(json) {
        let refs = serde_json::to_string(&item.segment_refs)
            .map_err(|error| crate::error::AppError::Storage(error.to_string()))?;
        if let Some((id, old_task, origin, _dismissed_at)) =
            by_task.remove(&item.task.to_lowercase())
        {
            if origin == "ai" {
                tx.execute(
                    "UPDATE meeting_action_items SET task=?1,owner=?2,due=?3,segment_refs_json=?4,source_task=?1,updated_at=?5 WHERE id=?6",
                    params![item.task, item.owner, item.due, refs, now, id],
                )?;
            }
            if origin == "ai" && !old_task.eq(&item.task) {
                let was_completed = legacy_completed
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(&old_task));
                if was_completed {
                    update_legacy_completed_actions(tx, meeting_id, &old_task, false)?;
                    update_legacy_completed_actions(tx, meeting_id, &item.task, true)?;
                }
            }
        } else if !manual_tasks.contains(&item.task.to_lowercase()) {
            let completed = legacy_completed
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&item.task));
            tx.execute(
                "INSERT INTO meeting_action_items (id,meeting_id,task,owner,due,segment_refs_json,origin,source_task,completed,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,'ai',?3,?7,?8,?8)",
                params![Uuid::new_v4().to_string(), meeting_id, item.task, item.owner, item.due, refs, completed, now],
            )?;
        }
    }

    // AI rows that disappeared from a regenerated summary are no longer
    // current. User-edited rows have origin='manual' and are never touched.
    for (_, (id, _, origin, dismissed_at)) in by_task {
        if origin == "ai" && dismissed_at.is_none() {
            tx.execute("DELETE FROM meeting_action_items WHERE id=?1", [id])?;
        }
    }
    tx.execute(
        "UPDATE meetings SET actions_materialized=1 WHERE id=?1",
        [meeting_id],
    )?;
    Ok(())
}

pub fn backfill_generated_action_items(db: &Database) -> AppResult<()> {
    let mut conn = db.conn()?;
    let meetings = {
        let mut stmt = conn.prepare(
            "SELECT id,summary_json FROM meetings WHERE actions_materialized=0 AND summary_json IS NOT NULL AND summary_json<>''",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (meeting_id, json) in meetings {
        let tx = conn.transaction()?;
        sync_generated_action_items(&tx, &meeting_id, &json, &Utc::now().to_rfc3339())?;
        tx.commit()?;
    }
    Ok(())
}

pub fn backfill_summary_versions(db: &Database) -> AppResult<()> {
    let mut conn = db.conn()?;
    let meetings = {
        let mut stmt = conn.prepare(
            "SELECT id,summary_markdown,summary_json,summary_provider,summary_model,summary_cost,prompt_tokens,completion_tokens,updated_at FROM meetings m WHERE summary_markdown<>'' AND NOT EXISTS (SELECT 1 FROM meeting_summary_versions v WHERE v.meeting_id=m.id)",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<f64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (meeting_id, markdown, json, provider, model, cost, prompt, completion, updated_at) in
        meetings
    {
        let tx = conn.transaction()?;
        let origin = if json.is_some() { "ai" } else { "manual" };
        insert_summary_version(
            &tx,
            &meeting_id,
            &markdown,
            json.as_deref(),
            provider.as_deref(),
            model.as_deref(),
            cost,
            prompt.map(|value| value.max(0) as u64),
            completion.map(|value| value.max(0) as u64),
            origin,
            &updated_at,
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn save_question_answer(
    db: &Database,
    meeting_id: &str,
    question: &str,
    answer: &str,
    segment_refs: &[String],
    provider: &str,
    model: &str,
    cost: f64,
    prompt_tokens: u64,
    completion_tokens: u64,
    context_segments: u64,
    total_segments: u64,
) -> AppResult<MeetingQuestionAnswer> {
    let record = MeetingQuestionAnswer {
        id: Uuid::new_v4().to_string(),
        meeting_id: meeting_id.to_string(),
        question: question.trim().to_string(),
        answer: answer.trim().to_string(),
        segment_refs: segment_refs.to_vec(),
        provider: provider.to_string(),
        model: model.to_string(),
        cost,
        prompt_tokens,
        completion_tokens,
        context_segments,
        total_segments,
        created_at: Utc::now().to_rfc3339(),
    };
    let segment_refs_json = serde_json::to_string(&record.segment_refs)
        .map_err(|error| crate::error::AppError::Storage(error.to_string()))?;
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO meeting_ai_usage (id,meeting_id,provider,model,cost,prompt_tokens,completion_tokens,request_kind,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,'question',?8)",
        params![Uuid::new_v4().to_string(),record.meeting_id,record.provider,record.model,record.cost,record.prompt_tokens as i64,record.completion_tokens as i64,record.created_at],
    )?;
    tx.execute(
        "INSERT INTO meeting_questions (id,meeting_id,question,answer,segment_refs_json,provider,model,cost,prompt_tokens,completion_tokens,context_segments,total_segments,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![record.id,record.meeting_id,record.question,record.answer,segment_refs_json,record.provider,record.model,record.cost,record.prompt_tokens as i64,record.completion_tokens as i64,record.context_segments as i64,record.total_segments as i64,record.created_at],
    )?;
    tx.commit()?;
    Ok(record)
}

pub fn delete_question_answer(db: &Database, id: &str, meeting_id: &str) -> AppResult<()> {
    db.conn()?.execute(
        "DELETE FROM meeting_questions WHERE id=?1 AND meeting_id=?2",
        params![id, meeting_id],
    )?;
    Ok(())
}

pub fn delete_meeting(db: &Database, id: &str) -> AppResult<Vec<String>> {
    let paths = {
        let conn = db.conn()?;
        let mut stmt = conn.prepare(
            "SELECT audio_path FROM meeting_chunks WHERE meeting_id=?1 AND audio_path<>''",
        )?;
        let paths = stmt
            .query_map([id], |row| row.get::<_, String>(0))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        paths
    };
    db.conn()?
        .execute("DELETE FROM meetings WHERE id=?1", [id])?;
    Ok(paths)
}

pub fn trash_meeting(db: &Database, id: &str) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    db.conn()?.execute(
        "UPDATE meetings SET deleted_at=?1,is_favorite=0,updated_at=?1 WHERE id=?2",
        params![now, id],
    )?;
    Ok(())
}

pub fn restore_meeting(db: &Database, id: &str) -> AppResult<()> {
    db.conn()?.execute(
        "UPDATE meetings SET deleted_at=NULL,updated_at=?1 WHERE id=?2",
        params![Utc::now().to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn get_provider_settings(db: &Database) -> AppResult<MeetingProviderSettings> {
    let conn = db.conn()?;
    Ok(conn.query_row("SELECT provider,model,monthly_budget,per_meeting_budget,max_prompt_price,max_completion_price,zdr_only,deny_data_collection,auto_suggest,auto_summarize,summary_preset,custom_instructions,transcription_mode,routing_preference FROM meeting_provider_settings WHERE id=1", [], |row| Ok(MeetingProviderSettings { provider: row.get(0)?, model: row.get(1)?, monthly_budget: row.get(2)?, per_meeting_budget: row.get(3)?, max_prompt_price: row.get(4)?, max_completion_price: row.get(5)?, zdr_only: row.get::<_,i64>(6)? != 0, deny_data_collection: row.get::<_,i64>(7)? != 0, auto_suggest: row.get::<_,i64>(8)? != 0, auto_summarize: row.get::<_,i64>(9)? != 0, summary_preset: row.get(10)?, custom_instructions: row.get(11)?, transcription_mode: row.get(12)?, routing_preference: row.get(13)? })).optional()?.unwrap_or_default())
}

pub fn save_provider_settings(db: &Database, settings: &MeetingProviderSettings) -> AppResult<()> {
    db.conn()?.execute("INSERT INTO meeting_provider_settings (id,provider,model,monthly_budget,per_meeting_budget,max_prompt_price,max_completion_price,zdr_only,deny_data_collection,auto_suggest,auto_summarize,summary_preset,custom_instructions,transcription_mode,routing_preference,updated_at) VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15) ON CONFLICT(id) DO UPDATE SET provider=excluded.provider,model=excluded.model,monthly_budget=excluded.monthly_budget,per_meeting_budget=excluded.per_meeting_budget,max_prompt_price=excluded.max_prompt_price,max_completion_price=excluded.max_completion_price,zdr_only=excluded.zdr_only,deny_data_collection=excluded.deny_data_collection,auto_suggest=excluded.auto_suggest,auto_summarize=excluded.auto_summarize,summary_preset=excluded.summary_preset,custom_instructions=excluded.custom_instructions,transcription_mode=excluded.transcription_mode,routing_preference=excluded.routing_preference,updated_at=excluded.updated_at", params![settings.provider,settings.model,settings.monthly_budget,settings.per_meeting_budget,settings.max_prompt_price,settings.max_completion_price,settings.zdr_only,settings.deny_data_collection,settings.auto_suggest,settings.auto_summarize,settings.summary_preset,settings.custom_instructions,settings.transcription_mode,settings.routing_preference,Utc::now().to_rfc3339()])?;
    Ok(())
}

pub fn month_spend(db: &Database) -> AppResult<f64> {
    let month = Utc::now().format("%Y-%m").to_string();
    Ok(db.conn()?.query_row(
        "SELECT COALESCE(SUM(cost),0) FROM meeting_ai_usage WHERE substr(created_at,1,7)=?1",
        [month],
        |row| row.get(0),
    )?)
}

pub fn meeting_spend(db: &Database, meeting_id: &str) -> AppResult<f64> {
    Ok(db.conn()?.query_row(
        "SELECT COALESCE(SUM(cost),0) FROM meeting_ai_usage WHERE meeting_id=?1",
        [meeting_id],
        |row| row.get(0),
    )?)
}

pub fn usage_stats(db: &Database) -> AppResult<MeetingUsageStats> {
    let month = Utc::now().format("%Y-%m").to_string();
    let conn = db.conn()?;
    let (cost, requests, prompt, completion): (f64, i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(cost),0), COUNT(*), COALESCE(SUM(prompt_tokens),0), COALESCE(SUM(completion_tokens),0) FROM meeting_ai_usage WHERE substr(created_at,1,7)=?1",
        [month],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    Ok(MeetingUsageStats {
        month_spend: cost,
        request_count: requests.max(0) as u64,
        prompt_tokens: prompt.max(0) as u64,
        completion_tokens: completion.max(0) as u64,
    })
}

pub fn list_ai_usage(db: &Database, limit: u64) -> AppResult<Vec<MeetingAiUsageRecord>> {
    let month = Utc::now().format("%Y-%m").to_string();
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT u.id,u.meeting_id,m.title,u.request_kind,u.provider,u.model,u.cost,u.prompt_tokens,u.completion_tokens,u.created_at
         FROM meeting_ai_usage u
         LEFT JOIN meetings m ON m.id=u.meeting_id
         WHERE substr(u.created_at,1,7)=?1
         ORDER BY u.created_at DESC,u.id DESC
         LIMIT ?2",
    )?;
    let records = stmt
        .query_map(params![month, limit.clamp(1, 500) as i64], |row| {
            Ok(MeetingAiUsageRecord {
                id: row.get(0)?,
                meeting_id: row.get(1)?,
                meeting_title: row.get(2)?,
                request_kind: row.get(3)?,
                provider: row.get(4)?,
                model: row.get(5)?,
                cost: row.get(6)?,
                prompt_tokens: row.get::<_, i64>(7)?.max(0) as u64,
                completion_tokens: row.get::<_, i64>(8)?.max(0) as u64,
                created_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

pub fn recover_interrupted(db: &Database) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE meeting_chunks SET status='pending' WHERE status='processing'",
        [],
    )?;
    tx.execute("UPDATE meetings SET status='interrupted',ended_at=COALESCE(ended_at,?1),updated_at=?1 WHERE status IN ('recording','paused','transcribing','summarizing')", [Utc::now().to_rfc3339()])?;
    tx.commit()?;
    Ok(())
}

pub fn pending_meeting_ids(db: &Database) -> AppResult<Vec<String>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT meeting_id FROM meeting_chunks WHERE status='pending' ORDER BY meeting_id",
    )?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

pub fn iso_now() -> DateTime<Utc> {
    Utc::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db(name: &str) -> (Database, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("omnivox-meetings-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::init(&dir.join("test.db")).unwrap();
        (db, dir)
    }

    #[test]
    fn meeting_chunks_commit_segments_and_clear_audio_reference() {
        let (db, dir) = test_db("chunks");
        let meeting = create_meeting(&db, "Product sync", Some("Google Meet")).unwrap();
        let chunk = add_chunk(&db, &meeting.id, "system", 10_000, 20_000, "sealed.pcm").unwrap();
        assert_eq!(pending_count(&db, &meeting.id).unwrap(), 1);
        complete_chunk(&db, &chunk, &[(500, 1_500, "Launch Thursday".into())]).unwrap();
        assert_eq!(pending_count(&db, &meeting.id).unwrap(), 0);
        assert_eq!(
            transcript_text(&db, &meeting.id).unwrap(),
            "[00:00:10] Meeting: Launch Thursday"
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_chunks_can_be_retried_and_processing_chunks_recover_after_restart() {
        let (db, dir) = test_db("chunk-retry");
        let meeting = create_meeting(&db, "Recovery", None).unwrap();
        let chunk = add_chunk(&db, &meeting.id, "mic", 0, 20_000, "recoverable.pcm").unwrap();

        let claimed = claim_next_pending_chunk(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(claimed.id, chunk.id);
        assert!(claim_next_pending_chunk(&db, &meeting.id)
            .unwrap()
            .is_none());
        assert_eq!(
            get_detail(&db, &meeting.id)
                .unwrap()
                .unwrap()
                .transcription
                .processing,
            1
        );

        recover_interrupted(&db).unwrap();
        assert_eq!(
            get_detail(&db, &meeting.id)
                .unwrap()
                .unwrap()
                .transcription
                .pending,
            1
        );
        let claimed = claim_next_pending_chunk(&db, &meeting.id).unwrap().unwrap();
        fail_chunk(&db, &claimed.id, "temporary model error").unwrap();
        let progress = get_detail(&db, &meeting.id).unwrap().unwrap().transcription;
        assert_eq!(progress.total, 1);
        assert_eq!(progress.failed, 1);

        assert_eq!(retry_failed_chunks(&db, &meeting.id).unwrap(), 1);
        assert_eq!(retry_failed_chunks(&db, &meeting.id).unwrap(), 0);
        let detail = get_detail(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(detail.meeting.status, "transcribing");
        assert_eq!(detail.transcription.pending, 1);
        assert_eq!(detail.transcription.failed, 0);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_items_surface_local_work_and_interrupted_paid_work() {
        let (db, dir) = test_db("recovery-items");
        let clean = create_meeting(&db, "Finished meeting", None).unwrap();
        set_status(&db, &clean.id, "ready", None, true).unwrap();

        let interrupted = create_meeting(&db, "Interrupted notes", None).unwrap();
        update_summary_markdown(&db, &interrupted.id, "# Existing notes").unwrap();
        set_status(&db, &interrupted.id, "summarizing", None, true).unwrap();
        recover_interrupted(&db).unwrap();

        let working = create_meeting(&db, "Pending transcript", None).unwrap();
        set_status(&db, &working.id, "transcribing", None, true).unwrap();
        add_chunk(&db, &working.id, "system", 0, 20_000, "pending.pcm").unwrap();

        let failed = create_meeting(&db, "Failed transcript", None).unwrap();
        let failed_chunk = add_chunk(&db, &failed.id, "mic", 0, 20_000, "failed.pcm").unwrap();
        let claimed = claim_next_pending_chunk(&db, &failed.id).unwrap().unwrap();
        assert_eq!(claimed.id, failed_chunk.id);
        fail_chunk(&db, &claimed.id, "temporary model failure").unwrap();
        set_status(
            &db,
            &failed.id,
            "error",
            Some("temporary model failure"),
            true,
        )
        .unwrap();

        let items = list_recovery_items(&db).unwrap();
        assert!(!items.iter().any(|item| item.meeting_id == clean.id));
        assert!(items.iter().any(|item| {
            item.meeting_id == interrupted.id
                && item.status == "interrupted"
                && item.has_summary
                && item.recoverable_failed_chunks == 0
        }));
        assert!(items.iter().any(|item| {
            item.meeting_id == working.id
                && item.status == "transcribing"
                && item.pending_chunks == 1
        }));
        assert!(items.iter().any(|item| {
            item.meeting_id == failed.id
                && item.failed_chunks == 1
                && item.recoverable_failed_chunks == 1
                && item.error.as_deref() == Some("temporary model failure")
        }));

        assert_eq!(retry_failed_chunks(&db, &failed.id).unwrap(), 1);
        let retried = list_recovery_items(&db)
            .unwrap()
            .into_iter()
            .find(|item| item.meeting_id == failed.id)
            .unwrap();
        assert_eq!(retried.status, "transcribing");
        assert_eq!(retried.pending_chunks, 1);
        assert_eq!(retried.failed_chunks, 0);
        assert_eq!(retried.error, None);

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn provider_limits_round_trip() {
        let (db, dir) = test_db("provider");
        let settings = MeetingProviderSettings {
            model: "google/gemini-2.5-flash".into(),
            monthly_budget: 3.5,
            per_meeting_budget: 0.07,
            auto_summarize: false,
            summary_preset: "executive".into(),
            custom_instructions: "Call out launch risk.".into(),
            transcription_mode: "live".into(),
            routing_preference: "throughput".into(),
            ..MeetingProviderSettings::default()
        };
        save_provider_settings(&db, &settings).unwrap();
        let loaded = get_provider_settings(&db).unwrap();
        assert_eq!(loaded.model, settings.model);
        assert_eq!(loaded.monthly_budget, 3.5);
        assert_eq!(loaded.per_meeting_budget, 0.07);
        assert!(!loaded.auto_summarize);
        assert_eq!(loaded.summary_preset, "executive");
        assert_eq!(loaded.custom_instructions, "Call out launch risk.");
        assert_eq!(loaded.transcription_mode, "live");
        assert_eq!(loaded.routing_preference, "throughput");
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn meeting_ai_overrides_round_trip_and_mark_existing_notes_stale() {
        let (db, dir) = test_db("meeting-ai-options");
        let meeting = create_meeting(&db, "Customer call", None).unwrap();
        update_summary_markdown(&db, &meeting.id, "# Existing notes").unwrap();
        update_ai_options(
            &db,
            &meeting.id,
            Some("anthropic/claude-haiku-4.5"),
            Some("sales"),
            "Prioritize objections and next steps.",
        )
        .unwrap();

        let loaded = get_meeting(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(
            loaded.ai_model_override.as_deref(),
            Some("anthropic/claude-haiku-4.5")
        );
        assert_eq!(loaded.summary_preset_override.as_deref(), Some("sales"));
        assert_eq!(
            loaded.summary_instructions,
            "Prioritize objections and next steps."
        );
        assert!(loaded.summary_stale);
        assert_eq!(search_meetings(&db, "objections").unwrap().len(), 1);

        update_ai_options(&db, &meeting.id, None, None, "").unwrap();
        let reset = get_meeting(&db, &meeting.id).unwrap().unwrap();
        assert!(reset.ai_model_override.is_none());
        assert!(reset.summary_preset_override.is_none());
        assert!(reset.summary_instructions.is_empty());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn agenda_and_participants_flow_through_search_and_exports() {
        let (db, dir) = test_db("meeting-metadata");
        let meeting = create_meeting(&db, "Planning", None).unwrap();
        update_summary_markdown(&db, &meeting.id, "# Existing notes").unwrap();
        update_metadata(
            &db,
            &meeting.id,
            Some("Resolve the launch readiness gate"),
            Some(&["Alex".into(), "Jordan".into()]),
        )
        .unwrap();

        let loaded = get_meeting(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(loaded.agenda, "Resolve the launch readiness gate");
        assert_eq!(loaded.participants, vec!["Alex", "Jordan"]);
        assert!(loaded.summary_stale);
        assert_eq!(search_meetings(&db, "Jordan").unwrap().len(), 1);
        let markdown = export_markdown(&db, &meeting.id).unwrap();
        assert!(markdown.contains("- Participants: Alex, Jordan"));
        assert!(markdown.contains("- Agenda: Resolve the launch readiness gate"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn polished_notes_export_excludes_private_meeting_data() {
        let (db, dir) = test_db("polished-export");
        let meeting = create_meeting(&db, "Launch review", Some("Google Meet")).unwrap();
        update_metadata(
            &db,
            &meeting.id,
            Some("Confirm the public launch plan"),
            Some(&["Alex".into(), "Jordan".into()]),
        )
        .unwrap();
        update_notes(&db, &meeting.id, "PRIVATE SCRATCHPAD DETAIL").unwrap();
        add_marker(&db, &meeting.id, 4_000, "PRIVATE MARKER DETAIL").unwrap();
        let chunk = add_chunk(&db, &meeting.id, "system", 0, 2_000, "sealed.pcm").unwrap();
        complete_chunk(
            &db,
            &chunk,
            &[(0, 2_000, "PRIVATE RAW TRANSCRIPT DETAIL".into())],
        )
        .unwrap();
        save_question_answer(
            &db,
            &meeting.id,
            "PRIVATE QUESTION DETAIL",
            "PRIVATE ANSWER DETAIL",
            &[],
            "OpenRouter",
            "private/question-model",
            0.002,
            100,
            20,
            1,
            1,
        )
        .unwrap();
        let structured = r#"{"action_items":[{"task":"Send launch brief","owner":"Alex","due":null,"segment_refs":[]}]}"#;
        assert_eq!(generated_action_items(structured).len(), 1);
        save_summary(
            &db,
            &meeting.id,
            "# Meeting summary\n\nPOLISHED SHAREABLE DETAIL\n\n## Action items\n\n- [ ] Send launch brief — Alex",
            structured,
            "OpenRouter",
            "private/summary-model",
            0.012345,
            1_000,
            200,
        )
        .unwrap();
        assert_eq!(
            get_detail(&db, &meeting.id)
                .unwrap()
                .unwrap()
                .action_items
                .len(),
            1
        );
        set_action_completed(&db, &meeting.id, "Send launch brief", true).unwrap();

        let polished = export_polished_notes(&db, &meeting.id).unwrap();
        assert!(polished.contains("# Launch review"));
        assert!(polished.contains("Participants: Alex, Jordan"));
        assert!(polished.contains("Agenda: Confirm the public launch plan"));
        assert!(polished.contains("POLISHED SHAREABLE DETAIL"));
        assert!(
            polished.contains("- [x] Send launch brief - Owner: Alex"),
            "{polished}"
        );
        for private_detail in [
            "PRIVATE SCRATCHPAD DETAIL",
            "PRIVATE MARKER DETAIL",
            "PRIVATE RAW TRANSCRIPT DETAIL",
            "PRIVATE QUESTION DETAIL",
            "PRIVATE ANSWER DETAIL",
            "private/summary-model",
            "private/question-model",
            "Summary cost",
            "Continued from",
        ] {
            assert!(
                !polished.contains(private_detail),
                "leaked {private_detail}"
            );
        }

        let archive = export_markdown(&db, &meeting.id).unwrap();
        assert!(archive.contains("PRIVATE SCRATCHPAD DETAIL"));
        assert!(archive.contains("PRIVATE RAW TRANSCRIPT DETAIL"));
        assert!(archive.contains("PRIVATE QUESTION DETAIL"));
        assert!(archive.contains("private/summary-model"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn meeting_templates_snapshot_update_and_delete_reusable_setup() {
        let (db, dir) = test_db("meeting-templates");
        let meeting = create_meeting(&db, "Customer discovery", None).unwrap();
        update_metadata(
            &db,
            &meeting.id,
            Some("Understand renewal blockers"),
            Some(&["Alex".into(), "Morgan".into()]),
        )
        .unwrap();
        update_ai_options(
            &db,
            &meeting.id,
            Some("anthropic/claude-haiku-4.5"),
            Some("sales"),
            "Lead with objections and next steps.",
        )
        .unwrap();

        let first = save_meeting_as_template(&db, &meeting.id, "Customer calls").unwrap();
        assert_eq!(first.title, "Customer discovery");
        assert_eq!(first.agenda, "Understand renewal blockers");
        assert_eq!(first.participants, vec!["Alex", "Morgan"]);
        assert_eq!(first.summary_preset_override.as_deref(), Some("sales"));
        assert_eq!(
            first.ai_model_override.as_deref(),
            Some("anthropic/claude-haiku-4.5")
        );

        update_title(&db, &meeting.id, "Renewal review").unwrap();
        let updated = save_meeting_as_template(&db, &meeting.id, "customer CALLS").unwrap();
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.name, "customer CALLS");
        assert_eq!(updated.title, "Renewal review");
        assert_eq!(list_templates(&db).unwrap().len(), 1);

        delete_template(&db, &first.id).unwrap();
        assert!(list_templates(&db).unwrap().is_empty());
        assert!(delete_template(&db, &first.id).is_err());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recurring_meeting_link_round_trips_and_exports_provenance() {
        let (db, _dir) = test_db("continuity");
        let previous = create_meeting(&db, "Weekly product sync", None).unwrap();
        let current = create_meeting_with_previous(
            &db,
            "Weekly product sync",
            Some("Google Meet"),
            Some(&previous.id),
        )
        .unwrap();

        assert_eq!(
            current.previous_meeting_id.as_deref(),
            Some(previous.id.as_str())
        );
        let markdown = export_markdown(&db, &current.id).unwrap();
        assert!(markdown.contains("Continued from: Weekly product sync"));

        delete_meeting(&db, &previous.id).unwrap();
        assert!(get_meeting(&db, &current.id)
            .unwrap()
            .unwrap()
            .previous_meeting_id
            .is_none());

        let missing = create_meeting_with_previous(&db, "Invalid", None, Some("missing"));
        assert!(missing.is_err());
    }

    #[test]
    fn every_successful_summary_counts_toward_monthly_spend() {
        let (db, dir) = test_db("usage");
        let meeting = create_meeting(&db, "Budget test", None).unwrap();
        save_summary(
            &db,
            &meeting.id,
            "one",
            "{}",
            "OpenRouter",
            "model-a",
            0.01,
            10,
            5,
        )
        .unwrap();
        save_summary(
            &db,
            &meeting.id,
            "two",
            "{}",
            "OpenRouter",
            "model-b",
            0.02,
            20,
            8,
        )
        .unwrap();
        assert!((month_spend(&db).unwrap() - 0.03).abs() < f64::EPSILON);
        let usage = usage_stats(&db).unwrap();
        assert_eq!(usage.request_count, 2);
        assert_eq!(usage.prompt_tokens, 30);
        assert_eq!(usage.completion_tokens, 13);
        let records = list_ai_usage(&db, 200).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.request_kind == "summary"));
        assert!(records
            .iter()
            .all(|record| record.meeting_title.as_deref() == Some("Budget test")));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn summary_versions_preserve_generated_and_hand_edited_notes_before_restore() {
        let (db, dir) = test_db("summary-versions");
        let meeting = create_meeting(&db, "Version safety", None).unwrap();
        save_summary(
            &db,
            &meeting.id,
            "# First generated notes",
            r#"{"overview":"First","action_items":[]}"#,
            "OpenRouter",
            "model-a",
            0.01,
            100,
            20,
        )
        .unwrap();
        update_summary_markdown(&db, &meeting.id, "# Carefully edited notes").unwrap();
        save_summary(
            &db,
            &meeting.id,
            "# Second generated notes",
            r#"{"overview":"Second","action_items":[]}"#,
            "OpenRouter",
            "model-b",
            0.02,
            200,
            30,
        )
        .unwrap();

        let versions = list_summary_versions(&db, &meeting.id).unwrap();
        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].markdown, "# Second generated notes");
        assert_eq!(versions[0].origin, "ai");
        assert_eq!(versions[1].markdown, "# Carefully edited notes");
        assert_eq!(versions[1].origin, "manual");
        assert_eq!(versions[2].markdown, "# First generated notes");
        assert_eq!(versions[2].model.as_deref(), Some("model-a"));
        let archive = export_markdown(&db, &meeting.id).unwrap();
        assert!(archive.contains("## Notes version history (private archive)"));
        assert!(archive.contains("Carefully edited notes"));

        let restored = restore_summary_version(&db, &versions[2].id, &meeting.id).unwrap();
        assert_eq!(restored.origin, "restored");
        let current = get_meeting(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(current.summary_markdown, "# First generated notes");
        assert_eq!(current.summary_model.as_deref(), Some("model-a"));
        assert!(current.summary_stale);
        let after_restore = list_summary_versions(&db, &meeting.id).unwrap();
        assert_eq!(after_restore.len(), 4);
        assert_eq!(after_restore[0].origin, "restored");
        assert!(restore_summary_version(&db, &versions[2].id, "wrong-meeting").is_err());

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn summary_version_backfill_is_idempotent_and_history_is_bounded() {
        let (db, dir) = test_db("summary-version-backfill");
        let legacy = create_meeting(&db, "Legacy notes", None).unwrap();
        update_summary_markdown(&db, &legacy.id, "# Existing hand-written notes").unwrap();
        assert!(list_summary_versions(&db, &legacy.id).unwrap().is_empty());
        backfill_summary_versions(&db).unwrap();
        backfill_summary_versions(&db).unwrap();
        let legacy_versions = list_summary_versions(&db, &legacy.id).unwrap();
        assert_eq!(legacy_versions.len(), 1);
        assert_eq!(legacy_versions[0].origin, "manual");

        let busy = create_meeting(&db, "Many regenerations", None).unwrap();
        for index in 0..55 {
            save_summary(
                &db,
                &busy.id,
                &format!("# Version {index}"),
                r#"{"action_items":[]}"#,
                "OpenRouter",
                "test/model",
                0.0,
                1,
                1,
            )
            .unwrap();
        }
        let bounded = list_summary_versions(&db, &busy.id).unwrap();
        assert_eq!(bounded.len(), MAX_SUMMARY_VERSIONS_PER_MEETING as usize);
        assert_eq!(bounded[0].markdown, "# Version 54");
        assert_eq!(bounded.last().unwrap().markdown, "# Version 5");

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn grounded_questions_persist_export_search_and_count_as_ai_usage() {
        let (db, dir) = test_db("questions");
        let meeting = create_meeting(&db, "Roadmap", None).unwrap();
        let exchange = save_question_answer(
            &db,
            &meeting.id,
            "What was decided?",
            "Ship the beta on Thursday.",
            &["[00:00:12]".into()],
            "OpenRouter",
            "test/model",
            0.004,
            500,
            40,
            12,
            20,
        )
        .unwrap();
        let detail = get_detail(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(detail.questions.len(), 1);
        assert_eq!(detail.questions[0].segment_refs, vec!["[00:00:12]"]);
        assert_eq!(detail.questions[0].context_segments, 12);
        assert_eq!(detail.questions[0].total_segments, 20);
        assert_eq!(search_meetings(&db, "Thursday").unwrap().len(), 1);
        let exported = export_markdown(&db, &meeting.id).unwrap();
        assert!(exported.contains("## Questions and answers"));
        assert!(exported.contains("Context: 12 of 20 transcript excerpts"));
        assert!((month_spend(&db).unwrap() - 0.004).abs() < f64::EPSILON);
        assert!((meeting_spend(&db, &meeting.id).unwrap() - 0.004).abs() < f64::EPSILON);
        assert_eq!(usage_stats(&db).unwrap().request_count, 1);
        let records = list_ai_usage(&db, 200).unwrap();
        assert_eq!(records[0].request_kind, "question");
        assert_eq!(records[0].meeting_title.as_deref(), Some("Roadmap"));

        delete_question_answer(&db, &exchange.id, &meeting.id).unwrap();
        assert!(get_detail(&db, &meeting.id)
            .unwrap()
            .unwrap()
            .questions
            .is_empty());
        assert!((month_spend(&db).unwrap() - 0.004).abs() < f64::EPSILON);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn summary_generation_has_a_single_atomic_owner() {
        let (db, dir) = test_db("summary-owner");
        let meeting = create_meeting(&db, "Ownership", None).unwrap();
        assert!(begin_summary(&db, &meeting.id).unwrap());
        assert!(!begin_summary(&db, &meeting.id).unwrap());
        set_status(&db, &meeting.id, "awaiting_summary", None, true).unwrap();
        assert!(begin_summary(&db, &meeting.id).unwrap());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn markers_are_searchable_exported_and_scoped_to_their_meeting() {
        let (db, dir) = test_db("markers");
        let first = create_meeting(&db, "Roadmap", Some("Google Meet")).unwrap();
        let second = create_meeting(&db, "Hiring", None).unwrap();
        update_notes(&db, &first.id, "Review launch gates").unwrap();
        let marker = add_marker(&db, &first.id, 65_000, "pricing decision").unwrap();

        let results = search_meetings(&db, "pricing").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, first.id);
        assert!(update_marker(&db, &marker.id, &second.id, "wrong meeting").is_err());
        update_marker(&db, &marker.id, &first.id, "final pricing").unwrap();

        let markdown = export_markdown(&db, &first.id).unwrap();
        assert!(markdown.contains("[00:01:05] final pricing"));
        assert!(markdown.contains("Review launch gates"));
        assert!(delete_marker(&db, &marker.id, &second.id).is_err());
        delete_marker(&db, &marker.id, &first.id).unwrap();
        assert!(get_detail(&db, &first.id)
            .unwrap()
            .unwrap()
            .markers
            .is_empty());

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrections_tags_trash_and_caption_exports_round_trip() {
        let (db, dir) = test_db("organize");
        let meeting = create_meeting(&db, "Customer call", Some("Zoom")).unwrap();
        let chunk = add_chunk(&db, &meeting.id, "system", 1_234, 4_567, "sealed.pcm").unwrap();
        complete_chunk(
            &db,
            &chunk,
            &[
                (0, 2_000, "Ship on Tuesday".into()),
                (2_100, 3_000, "Review support coverage".into()),
            ],
        )
        .unwrap();
        update_summary_markdown(&db, &meeting.id, "# Existing notes").unwrap();
        let mut captured = get_detail(&db, &meeting.id).unwrap().unwrap();
        let segment = captured.segments.remove(0);
        let second_segment = captured.segments.remove(0);
        update_segment_text(&db, &segment.id, &meeting.id, "Ship on Thursday").unwrap();
        update_segment_speaker(&db, &segment.id, &meeting.id, Some("Alex"), true).unwrap();
        update_segment_speaker(&db, &second_segment.id, &meeting.id, Some("Jordan"), false)
            .unwrap();
        set_tags(&db, &meeting.id, &["launch".into(), "customer".into()]).unwrap();
        set_action_completed(&db, &meeting.id, "Send launch brief", true).unwrap();

        let detail = get_detail(&db, &meeting.id).unwrap().unwrap();
        assert!(detail.meeting.summary_stale);
        assert_eq!(detail.meeting.tags, vec!["launch", "customer"]);
        assert_eq!(detail.meeting.completed_actions, vec!["Send launch brief"]);
        assert_eq!(detail.segments[0].speaker_label.as_deref(), Some("Alex"));
        assert_eq!(detail.segments[1].speaker_label.as_deref(), Some("Jordan"));
        assert_eq!(search_meetings(&db, "launch").unwrap().len(), 1);
        assert_eq!(search_meetings(&db, "Jordan").unwrap().len(), 1);
        assert!(transcript_text(&db, &meeting.id)
            .unwrap()
            .contains("Alex: Ship on Thursday"));
        assert!(export_webvtt(&db, &meeting.id)
            .unwrap()
            .contains("00:00:01.234 --> 00:00:03.234\nAlex: Ship on Thursday"));
        assert!(export_srt(&db, &meeting.id)
            .unwrap()
            .contains("1\n00:00:01,234 --> 00:00:03,234\nAlex: Ship on Thursday"));

        trash_meeting(&db, &meeting.id).unwrap();
        assert!(list_meetings(&db).unwrap().is_empty());
        assert_eq!(list_deleted_meetings(&db).unwrap().len(), 1);
        restore_meeting(&db, &meeting.id).unwrap();
        assert_eq!(list_meetings(&db).unwrap().len(), 1);

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn generated_action_items_become_durable_user_managed_follow_ups() {
        let (db, dir) = test_db("durable-actions");
        let meeting = create_meeting(&db, "Launch review", None).unwrap();
        set_action_completed(&db, &meeting.id, "Send launch brief", true).unwrap();
        save_summary(
            &db,
            &meeting.id,
            "# Notes",
            r#"{"action_items":[{"task":"Send launch brief","owner":"Alex","due":"Friday","segment_refs":["[00:00:10]"]},{"task":"Book support review","owner":null,"due":null,"segment_refs":[]}]}"#,
            "OpenRouter",
            "test/model",
            0.001,
            100,
            20,
        )
        .unwrap();

        let detail = get_detail(&db, &meeting.id).unwrap().unwrap();
        assert_eq!(detail.action_items.len(), 2);
        let generated = detail
            .action_items
            .iter()
            .find(|item| item.task == "Send launch brief")
            .unwrap();
        assert_eq!(generated.origin, "ai");
        assert!(generated.completed);
        assert_eq!(generated.segment_refs, vec!["[00:00:10]"]);
        let edited = update_action_item(
            &db,
            &generated.id,
            &meeting.id,
            "Send the final launch brief",
            Some("Jordan"),
            Some("Monday"),
        )
        .unwrap();
        assert_eq!(edited.origin, "manual");
        assert!(edited.completed);
        assert_eq!(
            get_meeting(&db, &meeting.id)
                .unwrap()
                .unwrap()
                .completed_actions,
            vec!["Send the final launch brief"]
        );

        let dismissed = detail
            .action_items
            .iter()
            .find(|item| item.task == "Book support review")
            .unwrap();
        delete_action_item(&db, &dismissed.id, &meeting.id).unwrap();
        save_summary(
            &db,
            &meeting.id,
            "# Updated notes",
            r#"{"action_items":[{"task":"Send launch brief","owner":"Alex","due":"Friday","segment_refs":[]},{"task":"Book support review","owner":"Sam","due":"Tuesday","segment_refs":[]},{"task":"Confirm pricing","owner":"Morgan","due":null,"segment_refs":[]}]}"#,
            "OpenRouter",
            "test/model",
            0.001,
            100,
            20,
        )
        .unwrap();

        let actions = list_action_items(&db, Some(&meeting.id)).unwrap();
        assert_eq!(actions.len(), 2);
        assert!(actions
            .iter()
            .any(|item| item.id == edited.id && item.owner.as_deref() == Some("Jordan")));
        assert!(actions.iter().any(|item| item.task == "Confirm pricing"));
        assert!(!actions
            .iter()
            .any(|item| item.task == "Book support review"));
        assert_eq!(search_meetings(&db, "Jordan").unwrap().len(), 1);
        let polished = export_polished_notes(&db, &meeting.id).unwrap();
        assert!(polished.contains("## Current follow-ups"));
        assert!(polished.contains("Send the final launch brief"));
        assert!(polished.contains("Owner: Jordan"));
        update_summary_markdown(&db, &meeting.id, "# Hand-edited notes").unwrap();
        assert_eq!(
            get_detail(&db, &meeting.id)
                .unwrap()
                .unwrap()
                .action_items
                .len(),
            2
        );

        let manual = add_action_item(&db, &meeting.id, "Share notes", Some("Alex"), None).unwrap();
        let completed = set_action_item_completed(&db, &manual.id, &meeting.id, true).unwrap();
        assert!(completed.completed);
        delete_action_item(&db, &manual.id, &meeting.id).unwrap();
        assert!(!list_action_items(&db, Some(&meeting.id))
            .unwrap()
            .iter()
            .any(|item| item.id == manual.id));

        delete_action_item(&db, &edited.id, &meeting.id).unwrap();
        save_summary(
            &db,
            &meeting.id,
            "# Final notes",
            r#"{"action_items":[{"task":"Send launch brief","owner":"Alex","due":"Friday","segment_refs":[]}]}"#,
            "OpenRouter",
            "test/model",
            0.001,
            100,
            20,
        )
        .unwrap();
        assert!(!list_action_items(&db, Some(&meeting.id))
            .unwrap()
            .iter()
            .any(|item| item.task == "Send launch brief" || item.id == edited.id));

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn legacy_summary_action_backfill_runs_once_without_overwriting_edits() {
        let (db, dir) = test_db("action-backfill");
        let meeting = create_meeting(&db, "Legacy notes", None).unwrap();
        db.conn()
            .unwrap()
            .execute(
                "UPDATE meetings SET summary_json=?1,actions_materialized=0 WHERE id=?2",
                params![
                    r#"{"action_items":[{"task":"Legacy task","owner":"Alex","due":null,"segment_refs":[]}]}"#,
                    meeting.id
                ],
            )
            .unwrap();
        backfill_generated_action_items(&db).unwrap();
        let item = list_action_items(&db, Some(&meeting.id)).unwrap().remove(0);
        update_action_item(
            &db,
            &item.id,
            &meeting.id,
            "User-owned task",
            Some("Jordan"),
            None,
        )
        .unwrap();
        backfill_generated_action_items(&db).unwrap();
        let actions = list_action_items(&db, Some(&meeting.id)).unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].task, "User-owned task");
        assert_eq!(actions[0].origin, "manual");

        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
