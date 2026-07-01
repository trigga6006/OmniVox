//! Storage for the user-editable voice-command registry.
//!
//! Rows live in `custom_voice_commands` and map to
//! [`CommandDef`](crate::postprocess::voice_commands::CommandDef)s consumed by
//! the parser.  The built-ins are seeded on first run so behavior is preserved;
//! users can then enable/disable, re-scope, edit, or add custom key combos.

use crate::error::AppResult;
use crate::postprocess::voice_commands::{
    default_command_table, CommandDef, ComboKey, KeyModifier, TriggerScope, VoiceCommand,
};
use crate::storage::database::Database;
use crate::storage::types::CustomVoiceCommand;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

// ── action <-> VoiceCommand encoding ─────────────────────────────

/// Decode a stored `action` string into a [`VoiceCommand`].
///
/// Built-ins use their variant name; custom commands use a `key:` combo spec.
/// Returns `None` for anything unrecognized (caller skips + logs).
pub fn action_to_command(action: &str) -> Option<VoiceCommand> {
    match action {
        "NewLine" => Some(VoiceCommand::NewLine),
        "NewParagraph" => Some(VoiceCommand::NewParagraph),
        "DeleteLastWord" => Some(VoiceCommand::DeleteLastWord),
        "Send" => Some(VoiceCommand::Send),
        "SelectAll" => Some(VoiceCommand::SelectAll),
        "Copy" => Some(VoiceCommand::Copy),
        "Cut" => Some(VoiceCommand::Cut),
        "Undo" => Some(VoiceCommand::Undo),
        "Redo" => Some(VoiceCommand::Redo),
        "PressTab" => Some(VoiceCommand::PressTab),
        "PressEscape" => Some(VoiceCommand::PressEscape),
        "PressEnter" => Some(VoiceCommand::PressEnter),
        other if other.starts_with("key:") => parse_key_combo(other),
        _ => None,
    }
}

/// Encode a [`VoiceCommand`] into its stored `action` string.
pub fn command_to_action(cmd: &VoiceCommand) -> String {
    match cmd {
        VoiceCommand::NewLine => "NewLine".into(),
        VoiceCommand::NewParagraph => "NewParagraph".into(),
        VoiceCommand::DeleteLastWord => "DeleteLastWord".into(),
        VoiceCommand::Send => "Send".into(),
        VoiceCommand::SelectAll => "SelectAll".into(),
        VoiceCommand::Copy => "Copy".into(),
        VoiceCommand::Cut => "Cut".into(),
        VoiceCommand::Undo => "Undo".into(),
        VoiceCommand::Redo => "Redo".into(),
        VoiceCommand::PressTab => "PressTab".into(),
        VoiceCommand::PressEscape => "PressEscape".into(),
        VoiceCommand::PressEnter => "PressEnter".into(),
        VoiceCommand::KeyCombo { modifiers, key } => encode_key_combo(modifiers, key),
    }
}

/// Encode a key combo as `key:ctrl+shift+k`.
fn encode_key_combo(modifiers: &[KeyModifier], key: &ComboKey) -> String {
    let mut parts: Vec<String> = modifiers
        .iter()
        .map(|m| match m {
            KeyModifier::Ctrl => "ctrl",
            KeyModifier::Alt => "alt",
            KeyModifier::Shift => "shift",
            KeyModifier::Meta => "meta",
        }
        .to_string())
        .collect();
    parts.push(match key {
        ComboKey::Char(c) => c.to_string(),
        ComboKey::Tab => "tab".into(),
        ComboKey::Escape => "escape".into(),
        ComboKey::Enter => "enter".into(),
        ComboKey::Space => "space".into(),
        ComboKey::Backspace => "backspace".into(),
    });
    format!("key:{}", parts.join("+"))
}

/// Parse a `key:ctrl+shift+k` spec. Last token is the key; the rest modifiers.
fn parse_key_combo(spec: &str) -> Option<VoiceCommand> {
    let body = spec.strip_prefix("key:")?;
    let tokens: Vec<&str> = body
        .split('+')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let (key_tok, mod_toks) = tokens.split_last()?;

    let mut modifiers = Vec::new();
    for t in mod_toks {
        let m = match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifier::Ctrl,
            "alt" | "option" => KeyModifier::Alt,
            "shift" => KeyModifier::Shift,
            "meta" | "cmd" | "command" | "win" | "super" => KeyModifier::Meta,
            _ => return None,
        };
        modifiers.push(m);
    }
    let key = parse_combo_key(key_tok)?;
    Some(VoiceCommand::KeyCombo { modifiers, key })
}

fn parse_combo_key(tok: &str) -> Option<ComboKey> {
    match tok.to_ascii_lowercase().as_str() {
        "tab" => Some(ComboKey::Tab),
        "escape" | "esc" => Some(ComboKey::Escape),
        "enter" | "return" => Some(ComboKey::Enter),
        "space" => Some(ComboKey::Space),
        "backspace" => Some(ComboKey::Backspace),
        lower => {
            let mut chars = lower.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None; // more than one char, not a named key
            }
            if c.is_ascii_alphanumeric() {
                Some(ComboKey::Char(c))
            } else {
                None
            }
        }
    }
}

// ── scope <-> string ─────────────────────────────────────────────

fn scope_to_str(scope: TriggerScope) -> &'static str {
    match scope {
        TriggerScope::Anywhere => "anywhere",
        TriggerScope::EndOfUtterance => "end_of_utterance",
    }
}

fn scope_from_str(s: &str) -> TriggerScope {
    match s {
        "end_of_utterance" => TriggerScope::EndOfUtterance,
        _ => TriggerScope::Anywhere,
    }
}

// ── row mapping ──────────────────────────────────────────────────

fn row_to_command(row: &rusqlite::Row) -> rusqlite::Result<CustomVoiceCommand> {
    let id_str: String = row.get(0)?;
    let phrase: String = row.get(1)?;
    let action: String = row.get(2)?;
    let trigger_scope: String = row.get(3)?;
    let enabled: bool = row.get(4)?;
    let built_in: bool = row.get(5)?;
    let sort_order: i32 = row.get(6)?;
    let created_at_str: String = row.get(7)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(CustomVoiceCommand {
        id,
        phrase,
        action,
        trigger_scope,
        enabled,
        built_in,
        sort_order,
        created_at,
    })
}

// ── seeding ──────────────────────────────────────────────────────

/// Seed the built-in commands when the table is empty (first run / after a
/// reset).  Idempotent: a no-op once rows exist.
pub fn seed_defaults(db: &Database) -> AppResult<()> {
    let conn = db.conn()?;
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM custom_voice_commands", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    for (idx, def) in default_command_table().iter().enumerate() {
        conn.execute(
            "INSERT INTO custom_voice_commands
                (id, phrase, action, trigger_scope, enabled, built_in, sort_order, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, 1, ?5, ?6)",
            params![
                Uuid::new_v4().to_string(),
                def.phrase,
                command_to_action(&def.command),
                scope_to_str(def.scope),
                idx as i64,
                now,
            ],
        )?;
    }
    Ok(())
}

// ── CRUD ─────────────────────────────────────────────────────────

/// List all commands (built-in and custom) for the management UI.
pub fn list(db: &Database) -> AppResult<Vec<CustomVoiceCommand>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, phrase, action, trigger_scope, enabled, built_in, sort_order, created_at
         FROM custom_voice_commands
         ORDER BY sort_order ASC, created_at ASC",
    )?;
    let rows = stmt
        .query_map([], row_to_command)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Load enabled commands as parser-ready [`CommandDef`]s.  Rows whose `action`
/// can't be decoded are skipped with a logged warning (never a panic).
pub fn list_enabled(db: &Database) -> AppResult<Vec<CommandDef>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT phrase, action, trigger_scope
         FROM custom_voice_commands
         WHERE enabled = 1
         ORDER BY sort_order ASC",
    )?;
    let raw = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut defs = Vec::with_capacity(raw.len());
    for (phrase, action, scope) in raw {
        match action_to_command(&action) {
            Some(command) => defs.push(CommandDef {
                phrase: phrase.to_lowercase(),
                command,
                scope: scope_from_str(&scope),
            }),
            None => {
                crate::llm::diaglog::log(&format!(
                    "voice_commands: skipping row with unparseable action '{action}'"
                ));
            }
        }
    }
    Ok(defs)
}

/// Add a custom command. `phrase` is stored lowercased. Returns the new row.
pub fn add(
    db: &Database,
    phrase: &str,
    action: &str,
    trigger_scope: &str,
) -> AppResult<CustomVoiceCommand> {
    let id = Uuid::new_v4();
    let now = Utc::now();
    let phrase = phrase.trim().to_lowercase();

    let conn = db.conn()?;
    let sort_order: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM custom_voice_commands",
        [],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO custom_voice_commands
            (id, phrase, action, trigger_scope, enabled, built_in, sort_order, created_at)
         VALUES (?1, ?2, ?3, ?4, 1, 0, ?5, ?6)",
        params![
            id.to_string(),
            phrase,
            action,
            trigger_scope,
            sort_order,
            now.to_rfc3339(),
        ],
    )?;

    Ok(CustomVoiceCommand {
        id,
        phrase,
        action: action.to_string(),
        trigger_scope: trigger_scope.to_string(),
        enabled: true,
        built_in: false,
        sort_order: sort_order as i32,
        created_at: now,
    })
}

/// Update an existing command's phrase, action, scope, and enabled flag.
pub fn update(
    db: &Database,
    id: &str,
    phrase: &str,
    action: &str,
    trigger_scope: &str,
    enabled: bool,
) -> AppResult<()> {
    let phrase = phrase.trim().to_lowercase();
    let conn = db.conn()?;
    conn.execute(
        "UPDATE custom_voice_commands
         SET phrase = ?1, action = ?2, trigger_scope = ?3, enabled = ?4
         WHERE id = ?5",
        params![phrase, action, trigger_scope, enabled, id],
    )?;
    Ok(())
}

/// Delete a command by ID.
pub fn delete(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM custom_voice_commands WHERE id = ?1", params![id])?;
    Ok(())
}

/// Clear all commands and re-seed the built-ins.
pub fn reset_to_defaults(db: &Database) -> AppResult<()> {
    {
        let conn = db.conn()?;
        conn.execute("DELETE FROM custom_voice_commands", [])?;
    }
    seed_defaults(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_combo_encode_decode_round_trips() {
        let cmd = VoiceCommand::KeyCombo {
            modifiers: vec![KeyModifier::Ctrl, KeyModifier::Shift],
            key: ComboKey::Char('k'),
        };
        let action = command_to_action(&cmd);
        assert_eq!(action, "key:ctrl+shift+k");
        assert_eq!(action_to_command(&action), Some(cmd));
    }

    #[test]
    fn named_keys_and_aliases_parse() {
        assert_eq!(
            action_to_command("key:alt+tab"),
            Some(VoiceCommand::KeyCombo {
                modifiers: vec![KeyModifier::Alt],
                key: ComboKey::Tab,
            })
        );
        // aliases: control -> Ctrl, cmd -> Meta, esc -> Escape
        assert_eq!(
            action_to_command("key:control+cmd+esc"),
            Some(VoiceCommand::KeyCombo {
                modifiers: vec![KeyModifier::Ctrl, KeyModifier::Meta],
                key: ComboKey::Escape,
            })
        );
    }

    #[test]
    fn builtin_action_names_round_trip() {
        for def in default_command_table() {
            let action = command_to_action(&def.command);
            assert_eq!(action_to_command(&action), Some(def.command));
        }
    }

    #[test]
    fn unrecognized_actions_are_none() {
        assert_eq!(action_to_command("Nonsense"), None);
        assert_eq!(action_to_command("key:"), None);
        assert_eq!(action_to_command("key:ctrl+"), None);
        assert_eq!(action_to_command("key:bogusmod+k"), None);
        assert_eq!(action_to_command("key:ctrl+notakey"), None);
    }
}
