//! OpenAI Codex CLI scanner. Reads rollout logs under `~/.codex/sessions/**`
//! for live rate-limit meters (`rate_limits` in event payloads) and session
//! activity (turn counts, models).

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

use super::{AgentStats, Bucket, ModelRow};

#[derive(Default, Clone)]
struct RateLimits {
    plan_type: String,
    primary_used: f64,
    primary_resets_at: Option<i64>,
    secondary_used: f64,
    secondary_resets_at: Option<i64>,
    seen_at: DateTime<Utc>,
}

pub fn detected(home: &Path) -> bool {
    home.join(".codex").join("auth.json").is_file()
        || cli_on_path()
        || !find_rollouts(home).is_empty()
}

pub fn scan(home: &Path, now: DateTime<Utc>) -> AgentStats {
    let rollouts = find_rollouts(home);
    if rollouts.is_empty() {
        return empty(
            "No Codex session logs found — install the CLI and run a session.",
        );
    }

    // Prefer rate limits from the newest rollout file (by mtime), not the
    // newest event scattered across older session logs.
    let latest_limits = limits_from_newest_rollout(&rollouts, now);
    let mut models: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut active_days: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut last_ts: Option<DateTime<Utc>> = None;

    for fp in &rollouts {
        if let Some(day) = day_from_path(fp) {
            active_days.insert(day);
        }
        let Ok(content) = std::fs::read_to_string(fp) else {
            continue;
        };
        for line in content.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let ts = parse_ts(&v);
            if let Some(t) = ts {
                last_ts = Some(match last_ts {
                    Some(prev) if prev > t => prev,
                    _ => t,
                });
            }
            if v.get("type").and_then(|t| t.as_str()) == Some("turn_context") {
                if let Some(model) = v
                    .pointer("/payload/model")
                    .and_then(|m| m.as_str())
                    .filter(|s| !s.is_empty())
                {
                    *models.entry(model.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let sessions = rollouts.len() as u32;
    let plan_label = latest_limits
        .as_ref()
        .map(|l| l.plan_type.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".to_string());

    let buckets = latest_limits
        .as_ref()
        .map(|l| limits_to_buckets(l, now))
        .unwrap_or_default();

    let note = if buckets.is_empty() {
        "Codex rate limits weren't found in your latest rollout — run `codex /status` in a session to refresh.".to_string()
    } else {
        let when = latest_limits
            .as_ref()
            .map(|l| humanize_when(l.seen_at, now))
            .unwrap_or_else(|| "recently".to_string());
        format!(
            "Live from your latest Codex rollout ({when}) — the same 5-hour and weekly utilization `codex /status` shows."
        )
    };

    let model_rows = models_to_rows(&models);

    AgentStats {
        sessions,
        active_days: active_days.len(),
        last: last_ts
            .map(|t| humanize_when(t, now))
            .unwrap_or_else(|| "—".to_string()),
        note,
        plan_label,
        buckets,
        total_tokens: "—".to_string(),
        models: model_rows,
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

fn find_rollouts(home: &Path) -> Vec<PathBuf> {
    let root = home.join(".codex").join("sessions");
    let pattern = format!("{}/**/rollout-*.jsonl", root.to_string_lossy());
    match glob::glob(&pattern) {
        Ok(paths) => paths.filter_map(Result::ok).collect(),
        Err(_) => Vec::new(),
    }
}

/// Walk rollout files newest-first (mtime) and return rate limits from the
/// first file that contains them.
fn limits_from_newest_rollout(rollouts: &[PathBuf], now: DateTime<Utc>) -> Option<RateLimits> {
    let mut sorted = rollouts.to_vec();
    sorted.sort_by(|a, b| mtime(b).cmp(&mtime(a)));
    for fp in &sorted {
        if let Some(limits) = limits_from_rollout(fp, now) {
            return Some(limits);
        }
    }
    None
}

fn mtime(path: &Path) -> std::time::SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
}

/// Latest `rate_limits` event inside a single rollout file.
fn limits_from_rollout(fp: &Path, now: DateTime<Utc>) -> Option<RateLimits> {
    let content = std::fs::read_to_string(fp).ok()?;
    let mut best: Option<RateLimits> = None;
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = parse_ts(&v);
        let rl = v
            .pointer("/payload/rate_limits")
            .or_else(|| v.pointer("/payload/info/rate_limits"));
        let Some(rl) = rl else { continue };
        let Some(parsed) = parse_rate_limits(rl, ts.unwrap_or(now)) else {
            continue;
        };
        if best.as_ref().is_none_or(|prev| parsed.seen_at >= prev.seen_at) {
            best = Some(parsed);
        }
    }
    best
}

fn cli_on_path() -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let exe = dir.join("codex");
        exe.is_file() || exe.with_extension("exe").is_file()
    })
}

fn day_from_path(fp: &Path) -> Option<String> {
    // .../sessions/2026/04/29/rollout-....jsonl
    let mut parts = fp.components().collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let day = parts.pop()?.as_os_str().to_string_lossy().to_string();
    let month = parts.pop()?.as_os_str().to_string_lossy().to_string();
    let year = parts.pop()?.as_os_str().to_string_lossy().to_string();
    if year.len() == 4 && month.len() == 2 && day.len() == 2 {
        Some(format!("{year}-{month}-{day}"))
    } else {
        None
    }
}

fn parse_ts(v: &Value) -> Option<DateTime<Utc>> {
    let ts = v.get("timestamp")?.as_str()?;
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn parse_rate_limits(v: &Value, seen_at: DateTime<Utc>) -> Option<RateLimits> {
    let primary = v.get("primary")?;
    let secondary = v.get("secondary")?;
    let primary_used = primary.get("used_percent")?.as_f64()?;
    let secondary_used = secondary.get("used_percent")?.as_f64()?;
    Some(RateLimits {
        plan_type: v
            .get("plan_type")
            .and_then(|p| p.as_str())
            .unwrap_or("codex")
            .to_string(),
        primary_used,
        primary_resets_at: primary.get("resets_at").and_then(|r| r.as_i64()),
        secondary_used,
        secondary_resets_at: secondary.get("resets_at").and_then(|r| r.as_i64()),
        seen_at,
    })
}

fn limits_to_buckets(l: &RateLimits, now: DateTime<Utc>) -> Vec<Bucket> {
    vec![
        make_limit_bucket(
            "Session",
            "5-hour window",
            l.primary_used,
            l.primary_resets_at,
            now,
        ),
        make_limit_bucket(
            "Week · all models",
            "resets weekly",
            l.secondary_used,
            l.secondary_resets_at,
            now,
        ),
    ]
}

fn make_limit_bucket(
    name: &str,
    sub: &str,
    used_pct: f64,
    resets_at: Option<i64>,
    now: DateTime<Utc>,
) -> Bucket {
    let pct = (used_pct * 10.0).round() / 10.0;
    let left = ((100.0 - pct) * 10.0).round() / 10.0;
    let (status, status_label) = status_for(pct);
    let reset = resets_at
        .and_then(|s| Utc.timestamp_opt(s, 0).single())
        .map(|dt| countdown(Some(dt), now))
        .unwrap_or_else(|| "—".to_string());
    Bucket {
        name: name.to_string(),
        sub: sub.to_string(),
        used_fmt: String::new(),
        used_pct: pct,
        left_pct: left,
        left_fmt: String::new(),
        limit_fmt: String::new(),
        reset,
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
                tokens: format!("{count} turns"),
                cost: "—".to_string(),
                pct: ((count as f64 / max as f64) * 100.0).round() as u32,
            }
        })
        .collect()
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

fn countdown(reset: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(reset) = reset else {
        return "ready".to_string();
    };
    let secs = (reset - now).num_seconds();
    if secs <= 0 {
        return "resetting".to_string();
    }
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    let m = (secs % 3_600) / 60;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{m}m")
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
    use std::io::Write;

    #[test]
    fn parses_rate_limits_from_rollout() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join(".codex").join("sessions").join("2026").join("06").join("19");
        std::fs::create_dir_all(&sessions).unwrap();
        let line = r#"{"type":"event_msg","timestamp":"2026-06-19T12:00:00.000Z","payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":6.0,"window_minutes":300,"resets_at":1781720131},"secondary":{"used_percent":44.0,"window_minutes":10080,"resets_at":1781742907}}}}"#;
        let mut f = std::fs::File::create(sessions.join("rollout-test.jsonl")).unwrap();
        writeln!(f, "{line}").unwrap();

        let now = DateTime::parse_from_rfc3339("2026-06-19T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let stats = scan(tmp.path(), now);
        assert_eq!(stats.sessions, 1);
        assert_eq!(stats.plan_label, "pro");
        assert_eq!(stats.buckets.len(), 2);
        assert!((stats.buckets[0].used_pct - 6.0).abs() < 0.1);
        assert!((stats.buckets[1].used_pct - 44.0).abs() < 0.1);
        assert!(stats.buckets[0].live);
    }

    #[test]
    fn prefers_newest_rollout_file_over_newer_event_in_older_file() {
        let tmp = tempfile::tempdir().unwrap();
        let old_day = tmp
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("10");
        let new_day = tmp
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("19");
        std::fs::create_dir_all(&old_day).unwrap();
        std::fs::create_dir_all(&new_day).unwrap();

        let stale = r#"{"type":"event_msg","timestamp":"2026-06-19T23:00:00.000Z","payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":99.0,"resets_at":1781720131},"secondary":{"used_percent":90.0,"resets_at":1781742907}}}}"#;
        let fresh = r#"{"type":"event_msg","timestamp":"2026-06-19T10:00:00.000Z","payload":{"rate_limits":{"plan_type":"pro","primary":{"used_percent":6.0,"resets_at":1781720131},"secondary":{"used_percent":44.0,"resets_at":1781742907}}}}"#;

        let old_path = old_day.join("rollout-old.jsonl");
        let new_path = new_day.join("rollout-new.jsonl");
        std::fs::write(&old_path, format!("{stale}\n")).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&new_path, format!("{fresh}\n")).unwrap();

        let now = DateTime::parse_from_rfc3339("2026-06-19T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let stats = scan(tmp.path(), now);
        assert!((stats.buckets[0].used_pct - 6.0).abs() < 0.1);
        assert!((stats.buckets[1].used_pct - 44.0).abs() < 0.1);
    }
}
