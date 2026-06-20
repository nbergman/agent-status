//! xAI Grok Build scanner. Reads session signals under `~/.grok/sessions/**`
//! for context-window utilization, model usage, and session activity.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::{AgentStats, Bucket, ModelRow};

#[derive(Default, Clone)]
struct Signals {
    context_tokens_used: u64,
    context_window_tokens: u64,
    context_window_usage: f64,
    primary_model: String,
    models: Vec<String>,
}

pub fn detected(home: &Path) -> bool {
    home.join(".grok").join("auth.json").is_file()
        || cli_on_path()
        || !find_signals(home).is_empty()
}

pub fn scan(home: &Path, now: DateTime<Utc>) -> AgentStats {
    let signal_paths = find_signals(home);
    if signal_paths.is_empty() {
        return empty("No Grok Build sessions found — install the CLI and run a session.");
    }

    let mut latest: Option<Signals> = None;
    let mut models: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut active_projects: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_seen: Option<DateTime<Utc>> = None;

    for fp in &signal_paths {
        if let Some(project) = project_from_path(fp) {
            active_projects.insert(project);
        }
        let Ok(raw) = std::fs::read_to_string(fp) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let sig = parse_signals(&v);
        if let Some(model) = summary_model(fp) {
            *models.entry(model).or_insert(0) += 1;
        }
        for m in &sig.models {
            *models.entry(m.clone()).or_insert(0) += 1;
        }
        if !sig.primary_model.is_empty() {
            *models.entry(sig.primary_model.clone()).or_insert(0) += 1;
        }
        let replace = latest
            .as_ref()
            .is_none_or(|prev| sig.context_window_usage >= prev.context_window_usage);
        if replace {
            latest = Some(sig);
        }
        if let Some(summary_path) = summary_path_for(fp) {
            if let Ok(summary_raw) = std::fs::read_to_string(summary_path) {
                if let Ok(summary) = serde_json::from_str::<Value>(&summary_raw) {
                    if let Some(updated) = summary
                        .get("updated_at")
                        .or_else(|| summary.get("created_at"))
                        .and_then(|t| t.as_str())
                    {
                        if let Ok(dt) = DateTime::parse_from_rfc3339(updated) {
                            let dt = dt.with_timezone(&Utc);
                            last_seen = Some(match last_seen {
                                Some(prev) if prev > dt => prev,
                                _ => dt,
                            });
                        }
                    }
                }
            }
        }
    }

    let latest = latest.unwrap_or_default();
    let sessions = signal_paths.len() as u32;
    let buckets = if latest.context_window_tokens > 0 || latest.context_window_usage > 0.0 {
        vec![make_context_bucket(&latest, now)]
    } else {
        Vec::new()
    };

    let total_tokens = if latest.context_tokens_used > 0 {
        fmt_tokens(latest.context_tokens_used as f64)
    } else {
        "—".to_string()
    };

    let note = if buckets.is_empty() {
        "Grok Build context usage wasn't found in session signals — open a session to refresh.".to_string()
    } else {
        "From Grok Build session signals — context window fill for your most active session.".to_string()
    };

    AgentStats {
        sessions,
        active_days: active_projects.len(),
        last: last_seen
            .map(|t| humanize_when(t, now))
            .unwrap_or_else(|| "—".to_string()),
        note,
        plan_label: if latest.primary_model.is_empty() {
            "grok".to_string()
        } else {
            latest.primary_model.clone()
        },
        buckets,
        total_tokens,
        models: models_to_rows(&models),
    }
}

fn empty(note: &str) -> AgentStats {
    AgentStats {
        sessions: 0,
        active_days: 0,
        last: "—".to_string(),
        note: note.to_string(),
        plan_label: "—".to_string(),
        buckets: Vec::new(),
        total_tokens: "—".to_string(),
        models: Vec::new(),
    }
}

fn find_signals(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".grok").join("sessions");
    let pattern = format!("{}/**/signals.json", root.to_string_lossy());
    match glob::glob(&pattern) {
        Ok(paths) => paths.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

fn cli_on_path() -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join("grok").is_file())
}

fn project_from_path(fp: &Path) -> Option<String> {
    // ~/.grok/sessions/<encoded-cwd>/<session-id>/signals.json
    let parent = fp.parent()?;
    let encoded = parent.parent()?.file_name()?.to_string_lossy().to_string();
    Some(decode_project(&encoded))
}

fn decode_project(encoded: &str) -> String {
    let decoded = encoded.replace("%2F", "/").replace("%20", " ");
    decoded
        .trim_start_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&decoded)
        .to_string()
}

fn summary_path_for(signals: &Path) -> Option<PathBuf> {
    signals.parent().map(|p| p.join("summary.json"))
}

fn summary_model(signals: &Path) -> Option<String> {
    let summary = std::fs::read_to_string(summary_path_for(signals)?).ok()?;
    let v: Value = serde_json::from_str(&summary).ok()?;
    v.get("current_model_id")
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_signals(v: &Value) -> Signals {
    let models = v
        .get("modelsUsed")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Signals {
        context_tokens_used: v
            .get("contextTokensUsed")
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
        context_window_tokens: v
            .get("contextWindowTokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
        context_window_usage: v
            .get("contextWindowUsage")
            .and_then(|n| n.as_f64())
            .unwrap_or(0.0),
        primary_model: v
            .get("primaryModelId")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string(),
        models,
    }
}

fn make_context_bucket(sig: &Signals, _now: DateTime<Utc>) -> Bucket {
    let pct = if sig.context_window_usage > 0.0 {
        (sig.context_window_usage * 10.0).round() / 10.0
    } else if sig.context_window_tokens > 0 {
        ((sig.context_tokens_used as f64 / sig.context_window_tokens as f64) * 100.0 * 10.0).round()
            / 10.0
    } else {
        0.0
    };
    let left = ((100.0 - pct) * 10.0).round() / 10.0;
    let (status, status_label) = status_for(pct);
    Bucket {
        name: "Context window".to_string(),
        sub: if sig.context_window_tokens > 0 {
            format!(
                "{} / {}",
                fmt_tokens(sig.context_tokens_used as f64),
                fmt_tokens(sig.context_window_tokens as f64)
            )
        } else {
            "current session".to_string()
        },
        used_fmt: String::new(),
        used_pct: pct,
        left_pct: left,
        left_fmt: String::new(),
        limit_fmt: String::new(),
        reset: "—".to_string(),
        status: status.to_string(),
        status_label: status_label.to_string(),
        live: true,
    }
}

fn models_to_rows(models: &std::collections::HashMap<String, u32>) -> Vec<ModelRow> {
    let mut rows: Vec<_> = models
        .iter()
        .map(|(name, count)| (name.clone(), *count))
        .collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let max = rows.first().map(|(_, c)| *c).unwrap_or(1).max(1);
    rows.into_iter()
        .take(6)
        .map(|(name, count)| {
            let key = name.to_lowercase().replace('.', "-");
            ModelRow {
                name: name.clone(),
                key,
                tokens: format!("{count} sessions"),
                cost: "—".to_string(),
                pct: ((count as f64 / max as f64) * 100.0).round() as u32,
            }
        })
        .collect()
}

fn fmt_tokens(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.2}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0}K", n / 1e3)
    } else {
        format!("{}", n as u64)
    }
}

fn status_for(pct: f64) -> (&'static str, &'static str) {
    if pct < 70.0 {
        ("ok", "Healthy")
    } else if pct < 90.0 {
        ("warn", "Watch")
    } else {
        ("danger", "Near limit")
    }
}

fn humanize_when(ts: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let delta = now - ts;
    let secs = delta.num_seconds().max(0);
    let days = delta.num_days();
    if days == 0 {
        let h = secs / 3_600;
        if h > 0 {
            format!("{h}h ago")
        } else {
            format!("{}m ago", secs / 60)
        }
    } else if days == 1 {
        "yesterday".to_string()
    } else {
        format!("{days}d ago")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signals_json() {
        let tmp = tempfile::tempdir().unwrap();
        let session = tmp
            .path()
            .join(".grok")
            .join("sessions")
            .join("%2Ftmp%2Fproj")
            .join("sess-1");
        std::fs::create_dir_all(&session).unwrap();
        let signals = serde_json::json!({
            "turnCount": 3,
            "contextTokensUsed": 31804,
            "contextWindowTokens": 200000,
            "contextWindowUsage": 15.0,
            "primaryModelId": "grok-build",
            "modelsUsed": ["grok-build"]
        });
        std::fs::write(session.join("signals.json"), signals.to_string()).unwrap();
        let summary = serde_json::json!({
            "current_model_id": "grok-build",
            "updated_at": "2026-06-19T12:00:00Z"
        });
        std::fs::write(session.join("summary.json"), summary.to_string()).unwrap();

        let now = DateTime::parse_from_rfc3339("2026-06-19T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let stats = scan(tmp.path(), now);
        assert_eq!(stats.sessions, 1);
        assert_eq!(stats.buckets.len(), 1);
        assert!((stats.buckets[0].used_pct - 15.0).abs() < 0.1);
        assert_eq!(stats.total_tokens, "32K");
    }
}
