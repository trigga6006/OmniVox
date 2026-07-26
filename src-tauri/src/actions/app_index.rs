//! App discovery + fuzzy resolution for "open <app>".
//!
//! Source of truth is the Windows **AppsFolder** — the same set the Start menu's
//! "All apps" shows, unioning Win32 desktop apps and Store/UWP apps.  We read it
//! via `Get-StartApps` (which returns `{Name, AppID}` for every app) and launch
//! via `explorer.exe shell:AppsFolder\<AppID>`, which activates-or-launches any
//! app type uniformly.  Using the shell here means no COM and no new crates for
//! v1; the COM `IShellItem` enumeration is a later latency optimization.
//!
//! A spoken name is fuzzy ("spot if I" → Spotify), so [`best_match`] scores the
//! query against the index (exact → token/substring containment → edit-distance)
//! and returns a confidence the caller uses to decide auto-launch vs. confirm.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// One launchable app from the AppsFolder index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppEntry {
    /// Friendly display name, e.g. "Visual Studio Code".
    pub name: String,
    /// AppsFolder AppID / AUMID used to launch via the shell.
    pub app_id: String,
}

/// A resolved match plus its confidence (0.0–1.0).
#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub name: String,
    pub app_id: String,
    pub score: f32,
    /// True when a runner-up scored within [`AMBIGUITY_MARGIN`] of the best —
    /// the caller should confirm rather than auto-launch even if `score >= AUTO`.
    pub ambiguous: bool,
}

/// Below this, treat the query as "no such app" (never guess-launch).
pub const FLOOR: f32 = 0.50;
/// At or above this, launch immediately; in [FLOOR, AUTO) ask to confirm first.
/// Set just below the exact-token tier (0.95) so ONLY exact-name and
/// exact-whole-token matches auto-launch; prefix / substring / fuzzy matches
/// always go through confirmation.
pub const AUTO: f32 = 0.94;
/// If the runner-up is this close to the best, treat the match as ambiguous
/// and confirm (e.g. "Spotify" vs "Spotify Lite").
const AMBIGUITY_MARGIN: f32 = 0.06;

static INDEX: OnceLock<Mutex<Option<Vec<AppEntry>>>> = OnceLock::new();

/// Unix-seconds of the last successful enumeration. 0 = never refreshed via
/// [`refresh`] (a lazy [`snapshot`] load doesn't stamp it, so the first
/// [`refresh_if_stale`] after a lazy load will re-enumerate once).
static LAST_REFRESH: AtomicU64 = AtomicU64::new(0);

/// Minimum seconds between self-heal re-enumerations (they spawn PowerShell).
const REFRESH_TTL_SECS: u64 = 300;

fn cell() -> &'static Mutex<Option<Vec<AppEntry>>> {
    INDEX.get_or_init(|| Mutex::new(None))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Force a fresh enumeration (call once at startup, off the main thread).
pub fn refresh() {
    let entries = load_entries();
    let got_apps = !entries.is_empty();
    if let Ok(mut guard) = cell().lock() {
        // A transient Get-StartApps / JSON failure returns an empty Vec. Never
        // let that clobber a previously-good index — otherwise the self-heal
        // rescan fired from the "no app found" path could lock out EVERY "open
        // app" for the TTL. Caching an empty result is only acceptable when we
        // have nothing yet (keeps snapshot() from reloading synchronously).
        let keep_old =
            !got_apps && guard.as_ref().map(|e| !e.is_empty()).unwrap_or(false);
        if !keep_old {
            *guard = Some(entries);
        }
    }
    // Only stamp on a successful (non-empty) enumeration, so a transient failure
    // is retried by the next refresh_if_stale instead of suppressed for the TTL.
    if got_apps {
        LAST_REFRESH.store(now_secs(), Ordering::Relaxed);
    }
}

/// Re-enumerate at most once per [`REFRESH_TTL_SECS`]. Fired (non-blocking, off
/// the async runtime) from the "no app found" path so an app installed while
/// OmniVox is running becomes launchable on the user's NEXT attempt — without a
/// restart or a manual rescan. A no-op while within the TTL.
pub fn refresh_if_stale() {
    if now_secs().saturating_sub(LAST_REFRESH.load(Ordering::Relaxed)) < REFRESH_TTL_SECS {
        return;
    }
    refresh();
}

/// Snapshot the index, loading it on first use.
fn snapshot() -> Vec<AppEntry> {
    {
        if let Ok(guard) = cell().lock() {
            if let Some(entries) = guard.as_ref() {
                return entries.clone();
            }
        }
    }
    // First use and not pre-warmed: load now (blocking) and cache.
    let entries = load_entries();
    if let Ok(mut guard) = cell().lock() {
        *guard = Some(entries.clone());
    }
    entries
}

/// Resolve a spoken app name against the cached index.
pub fn resolve(query: &str) -> Option<ResolveResult> {
    best_match(query, &snapshot())
}

// ── Matching (pure, testable) ────────────────────────────────────────────

/// Levenshtein distance over chars.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

/// Normalized similarity in 0.0–1.0 (1.0 = identical).
fn sim(a: &str, b: &str) -> f32 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 0.0;
    }
    1.0 - (edit_distance(a, b) as f32 / max as f32)
}

/// Score how well a normalized query matches a normalized app name.
///
/// Tier order is deliberate: an exact whole-token match (0.95) outranks a
/// prefix match (0.90) so "word" resolves to "Microsoft Word", not "WordPad".
/// Only the top two tiers (exact name 1.0, exact token 0.95) clear [`AUTO`];
/// everything below confirms.
fn score(query_norm: &str, name_norm: &str) -> f32 {
    if query_norm == name_norm {
        return 1.0;
    }

    let q_tokens: Vec<&str> = query_norm.split(' ').filter(|t| !t.is_empty()).collect();
    let n_tokens: Vec<&str> = name_norm.split(' ').filter(|t| !t.is_empty()).collect();

    // Every query token is a whole token of the name ("code" in "visual studio
    // code", "word" in "microsoft word").  Ranks ABOVE prefix.
    if !q_tokens.is_empty()
        && q_tokens
            .iter()
            .all(|qt| n_tokens.iter().any(|nt| nt == qt))
    {
        return 0.95;
    }
    // Prefix ("photosh" → "photoshop").  Kept far enough below the exact-token
    // tier (0.95) that exact tokens win by more than AMBIGUITY_MARGIN — so
    // "word" decisively picks "Microsoft Word" over "WordPad" instead of
    // tripping the ambiguity confirm.
    if name_norm.starts_with(query_norm) {
        return 0.88;
    }
    // Substring ("photoshop" in "adobe photoshop 2024").
    if name_norm.contains(query_norm) {
        return 0.82;
    }

    // Fuzzy fallback: best of whole-string and per-token similarity. Token
    // similarity is discounted so a single fuzzy token can't outrank a real
    // containment match above.  Stopwords are skipped so a shared "and"/"the"
    // can't inflate a garbage match (e.g. "spotify and play" scoring 0.85
    // against "Defragment and Optimize Drives" purely on the word "and").
    const STOPWORDS: &[&str] = &["and", "the", "of", "or", "a", "an", "to", "for"];
    let whole = sim(query_norm, name_norm);
    let token_best = q_tokens
        .iter()
        .filter(|qt| !STOPWORDS.contains(qt))
        .map(|qt| {
            n_tokens
                .iter()
                .map(|nt| sim(qt, nt))
                .fold(0.0_f32, f32::max)
        })
        .fold(0.0_f32, f32::max);
    whole.max(token_best * 0.85)
}

/// Best-scoring entry for `query`, or `None` if nothing clears [`FLOOR`].
///
/// Also flags `ambiguous` when the runner-up is within [`AMBIGUITY_MARGIN`] of
/// the best, so the caller can confirm instead of guessing between two
/// near-equal candidates.
pub fn best_match(query: &str, entries: &[AppEntry]) -> Option<ResolveResult> {
    let q = super::matcher::normalize(query);
    if q.is_empty() {
        return None;
    }
    let mut best_idx: Option<usize> = None;
    let mut best_score = 0.0_f32;
    let mut second_score = 0.0_f32;
    for (i, e) in entries.iter().enumerate() {
        let s = score(&q, &super::matcher::normalize(&e.name));
        if best_idx.is_none() || s > best_score {
            second_score = best_score;
            best_score = s;
            best_idx = Some(i);
        } else if s > second_score {
            second_score = s;
        }
    }
    let idx = best_idx?;
    if best_score < FLOOR {
        return None;
    }
    // An exact full-name match (1.0) is definitive — never demote it to a
    // confirm just because a longer-named app shares the token.
    let ambiguous =
        best_score < 1.0 && second_score > 0.0 && (best_score - second_score) < AMBIGUITY_MARGIN;
    Some(ResolveResult {
        name: entries[idx].name.clone(),
        app_id: entries[idx].app_id.clone(),
        score: best_score,
        ambiguous,
    })
}

// ── Platform: enumerate + launch ─────────────────────────────────────────

#[cfg(windows)]
fn load_entries() -> Vec<AppEntry> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    #[derive(serde::Deserialize)]
    struct RawApp {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "AppID")]
        app_id: String,
    }

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-StartApps | Select-Object Name,AppID | ConvertTo-Json -Compress",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let Ok(output) = output else {
        eprintln!("app_index: failed to run Get-StartApps");
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let raw: Vec<RawApp> = serde_json::from_str(&stdout).unwrap_or_default();
    raw.into_iter()
        .filter(|r| !r.name.trim().is_empty() && !r.app_id.trim().is_empty())
        .map(|r| AppEntry {
            name: r.name,
            app_id: r.app_id,
        })
        .collect()
}

#[cfg(not(windows))]
fn load_entries() -> Vec<AppEntry> {
    Vec::new()
}

/// The identity we can attribute to an app we just launched, so a settle can
/// prove the window that appears really belongs to it (B2-4).
///
/// AppsFolder launches go through `explorer.exe shell:AppsFolder\…`, which
/// activates the target app in a SEPARATE process whose pid we never see — so
/// those resolve to `Package` (the AUMID).  We do not correlate an AUMID to a
/// window (that needs the shell property store), so a `Package` identity is
/// treated as UNPROVEN downstream: the launch still happens, but focus-dependent
/// retargeting and undo recording are skipped.
#[derive(Debug, Clone)]
pub enum LaunchIdentity {
    /// The launched process id — correlated against a candidate window's owner.
    Pid(u32),
    /// The app's AUMID/package — not correlated to a window (unproven identity).
    Package(String),
}

/// Launch (or activate) an app by its AppsFolder AppID.  Returns the launched
/// app's expected identity so a settle can verify the window that appears
/// belongs to it.  `explorer.exe` is only the launcher — its child pid is not
/// the app's — so the best correlator here is the AUMID (`Package`, unproven;
/// see [`LaunchIdentity`]).
#[cfg(windows)]
pub fn launch(app_id: &str) -> Result<LaunchIdentity, String> {
    use std::process::Command;
    Command::new("explorer.exe")
        .arg(format!("shell:AppsFolder\\{app_id}"))
        .spawn()
        .map_err(|e| format!("Failed to launch app: {e}"))?;
    Ok(LaunchIdentity::Package(app_id.to_string()))
}

#[cfg(not(windows))]
pub fn launch(_app_id: &str) -> Result<LaunchIdentity, String> {
    Err("App launching is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> Vec<AppEntry> {
        ["Spotify", "Google Chrome", "Visual Studio Code", "Discord", "Microsoft Word"]
            .iter()
            .map(|n| AppEntry {
                name: (*n).to_string(),
                app_id: format!("{n}.aumid"),
            })
            .collect()
    }

    #[test]
    fn exact_and_case_insensitive() {
        let r = best_match("spotify", &idx()).unwrap();
        assert_eq!(r.name, "Spotify");
        assert!((r.score - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn token_containment_handles_vendor_prefixes() {
        assert_eq!(best_match("chrome", &idx()).unwrap().name, "Google Chrome");
        assert_eq!(best_match("code", &idx()).unwrap().name, "Visual Studio Code");
        assert_eq!(best_match("word", &idx()).unwrap().name, "Microsoft Word");
    }

    #[test]
    fn fuzzy_handles_mishearings() {
        // ASR typo should still resolve to Spotify above the floor.
        let r = best_match("spotifi", &idx()).unwrap();
        assert_eq!(r.name, "Spotify");
        assert!(r.score >= FLOOR);
    }

    #[test]
    fn unknown_app_returns_none() {
        assert!(best_match("calculator", &idx()).is_none());
        assert!(best_match("", &idx()).is_none());
    }

    #[test]
    fn exact_token_outranks_prefix() {
        // "word" must resolve to "Microsoft Word" (exact token), not "WordPad"
        // (mere prefix) — the regression Codex flagged.
        let apps = vec![
            AppEntry { name: "WordPad".into(), app_id: "wordpad".into() },
            AppEntry { name: "Microsoft Word".into(), app_id: "word".into() },
        ];
        let r = best_match("word", &apps).unwrap();
        assert_eq!(r.name, "Microsoft Word");
        assert!(r.score >= AUTO && !r.ambiguous, "exact token should auto-launch");
    }

    #[test]
    fn close_candidates_are_flagged_ambiguous() {
        // "teams" is an exact token of both → near-tie → confirm, don't guess.
        let apps = vec![
            AppEntry { name: "Microsoft Teams".into(), app_id: "a".into() },
            AppEntry { name: "Teams Machine-Wide Installer".into(), app_id: "b".into() },
        ];
        let r = best_match("teams", &apps).unwrap();
        assert!(r.ambiguous, "two equal token matches must be ambiguous");
    }

    #[test]
    fn exact_name_wins_over_shared_token() {
        // Exact full-name match is definitive even when a longer app shares the
        // token — "spotify" → Spotify, auto-launch, not ambiguous.
        let apps = vec![
            AppEntry { name: "Spotify".into(), app_id: "a".into() },
            AppEntry { name: "Spotify Lite".into(), app_id: "b".into() },
        ];
        let r = best_match("spotify", &apps).unwrap();
        assert_eq!(r.name, "Spotify");
        assert!(!r.ambiguous && r.score >= AUTO);
    }
}
