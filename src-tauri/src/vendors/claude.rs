//! Claude (Anthropic) LIVE subscription usage — the same data Claude Code's
//! status bar / `/usage` shows. Reads the OAuth token Claude Code stored
//! (macOS keychain `Claude Code-credentials`, else `~/.claude/.credentials.json`)
//! and calls `GET https://api.anthropic.com/api/oauth/usage`.
//!
//! This is an UNDOCUMENTED endpoint used with the user's own subscription token,
//! at the user's request. If the token is missing/expired we fall back silently.

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::time::Duration as StdDuration;

use crate::scanner::Bucket;

const ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
/// Keychain generic-password service name Claude Code stores its login under.
const SERVICE: &str = "Claude Code-credentials";
/// OAuth token endpoint and public client id Claude Code uses to exchange a
/// refresh token for a fresh access token. Reverse-engineered from Claude
/// Code's own flow; used here only with the user's own stored refresh token.
const TOKEN_ENDPOINT: &str = "https://console.anthropic.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeLive {
    /// An OAuth token was found.
    pub configured: bool,
    /// Fetch + parse succeeded.
    pub ok: bool,
    /// A token exists but it was rejected (HTTP 401) — the Claude Code login
    /// expired and the user must sign in again. Distinct from a transient
    /// network failure so the UI can give a clear re-auth instruction.
    pub expired: bool,
    pub error: Option<String>,
    pub buckets: Vec<Bucket>,
}

impl ClaudeLive {
    fn off(configured: bool, error: Option<String>) -> Self {
        Self { configured, ok: false, expired: false, error, buckets: Vec::new() }
    }
}

/// Whether a usable Claude Code install is present: a stored login token, or
/// the `claude` CLI somewhere on PATH. Cheap, no process spawn.
pub fn detected() -> bool {
    read_token().is_some() || cli_on_path()
}

fn cli_on_path() -> bool {
    let Some(paths) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&paths).any(|dir| {
        let exe = dir.join("claude");
        exe.is_file()
            || exe.with_extension("exe").is_file()
            || exe.with_extension("cmd").is_file()
    })
}

/// Read the Claude Code OAuth access token (platform-specific).
pub fn read_token() -> Option<String> {
    parse_token_json(&read_raw_credentials()?)
}

/// The raw credentials JSON Claude Code stored — macOS keychain first, then the
/// `~/.claude/.credentials.json` file. Shared by token reads and the refresh.
fn read_raw_credentials() -> Option<String> {
    #[cfg(target_os = "macos")]
    if let Some(raw) = keychain_read() {
        return Some(raw);
    }
    read_credentials_file()
}

#[cfg(target_os = "macos")]
fn keychain_read() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_credentials_file() -> Option<String> {
    let home = dirs::home_dir()?;
    std::fs::read_to_string(home.join(".claude").join(".credentials.json")).ok()
}

fn parse_token_json(raw: &str) -> Option<String> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    v.get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Refresh an expired Claude Code access token using the stored refresh token,
/// then write the rotated credentials back to the same store Claude Code reads.
///
/// The refresh token is SINGLE-USE: the server invalidates the old one and
/// returns a new one. If we obtain new tokens but fail to persist them, the
/// user is locked out of Claude Code — so persistence failures are hard errors
/// and we fall back to the credentials file rather than dropping the new token.
pub async fn refresh(now: DateTime<Utc>) -> Result<(), String> {
    let raw = read_raw_credentials().ok_or("No Claude Code login found to refresh.")?;
    let mut root: Value = serde_json::from_str(raw.trim())
        .map_err(|e| format!("stored credentials unreadable: {e}"))?;
    let oauth = root
        .get_mut("claudeAiOauth")
        .and_then(|v| v.as_object_mut())
        .ok_or("stored credentials missing the claudeAiOauth object")?;
    let refresh_token = oauth
        .get("refreshToken")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("No refresh token stored — sign in from Claude Code (/login).")?
        .to_string();

    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(15))
        .build()
        .map_err(|e| format!("client init: {e}"))?;

    let resp = client
        .post(TOKEN_ENDPOINT)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
            "client_id": CLIENT_ID,
        }))
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;

    let status = resp.status();
    if !status.is_success() {
        // 400 invalid_grant / 401 → the refresh token itself is dead (already
        // rotated by another Claude Code session, revoked, or fully expired).
        // Only a real login can fix it.
        if status.as_u16() == 400 || status.as_u16() == 401 {
            return Err("Refresh token expired — sign in again from Claude Code (/login).".into());
        }
        return Err(format!("token endpoint returned HTTP {}", status.as_u16()));
    }

    let tok: Value = resp
        .json()
        .await
        .map_err(|e| format!("invalid token response: {e}"))?;
    let access = tok
        .get("access_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("token response had no access_token")?
        .to_string();
    // Replace the refresh token (single-use rotation). If the server omitted a
    // new one, keep the prior value rather than blanking it.
    let new_refresh = tok
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let expires_in = tok.get("expires_in").and_then(|v| v.as_i64()).unwrap_or(28_800);
    let expires_at = (now + Duration::seconds(expires_in)).timestamp_millis();

    // Merge into the existing object so every field Claude Code wrote
    // (subscriptionType, organizationUuid, accountUuid, email, …) is preserved.
    oauth.insert("accessToken".into(), Value::String(access));
    if let Some(r) = new_refresh {
        oauth.insert("refreshToken".into(), Value::String(r));
    }
    oauth.insert("expiresAt".into(), Value::Number(expires_at.into()));
    if let Some(scope) = tok.get("scope").and_then(|s| s.as_str()) {
        let scopes: Vec<Value> = scope
            .split_whitespace()
            .map(|s| Value::String(s.to_string()))
            .collect();
        if !scopes.is_empty() {
            oauth.insert("scopes".into(), Value::Array(scopes));
        }
    }

    let serialized = serde_json::to_string(&root).map_err(|e| format!("serialize: {e}"))?;
    write_credentials(&serialized)
}

/// Persist refreshed credentials to the store Claude Code reads.
fn write_credentials(json: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        match keychain_write(json) {
            // Keep a co-existing credentials file in sync if one is present so
            // the two stores can't diverge.
            Ok(()) => {
                let _ = write_credentials_file_if_exists(json);
                Ok(())
            }
            // Keychain write failed AFTER the server already rotated the refresh
            // token — persist to the file so the only-valid tokens aren't lost.
            Err(e) => write_credentials_file(json)
                .map_err(|fe| format!("keychain update failed ({e}) and file fallback failed ({fe})")),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        write_credentials_file(json)
    }
}

#[cfg(target_os = "macos")]
fn keychain_write(json: &str) -> Result<(), String> {
    let account = keychain_account()
        .or_else(|| std::env::var("USER").ok())
        .ok_or("could not determine keychain account")?;
    // `-U` updates the existing item in place (preserving its access control),
    // so Claude Code keeps reading it without a new keychain prompt.
    let status = std::process::Command::new("security")
        .args(["add-generic-password", "-U", "-a", &account, "-s", SERVICE, "-w", json])
        .status()
        .map_err(|e| format!("spawn security: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("security add-generic-password returned non-zero".into())
    }
}

/// Read the account name on the existing keychain item so we update that exact
/// item rather than creating a divergent one under a different account.
#[cfg(target_os = "macos")]
fn keychain_account() -> Option<String> {
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", SERVICE])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        // Format: `    "acct"<blob>="dennisrongo"`
        if let Some(idx) = line.find("\"acct\"") {
            if let Some(eq) = line[idx..].find('=') {
                let val = line[idx + eq + 1..].trim().trim_matches('"');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
        }
    }
    None
}

fn write_credentials_file(json: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("no home directory")?;
    let dir = home.join(".claude");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create ~/.claude: {e}"))?;
    let path = dir.join(".credentials.json");
    std::fs::write(&path, json).map_err(|e| format!("write credentials file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_credentials_file_if_exists(json: &str) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("no home directory")?;
    let path = home.join(".claude").join(".credentials.json");
    if path.exists() {
        write_credentials_file(json)
    } else {
        Ok(())
    }
}

pub async fn fetch(now: DateTime<Utc>) -> ClaudeLive {
    let Some(token) = read_token() else {
        return ClaudeLive::off(false, None);
    };

    let client = match reqwest::Client::builder()
        .timeout(StdDuration::from_secs(12))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ClaudeLive::off(true, Some(format!("client init: {e}"))),
    };

    let resp = client
        .get(ENDPOINT)
        .header("Authorization", format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status();
            if !status.is_success() {
                let expired = status.as_u16() == 401;
                let hint = if expired {
                    " (Claude Code login expired — open Claude Code to re-auth)"
                } else {
                    ""
                };
                let mut off =
                    ClaudeLive::off(true, Some(format!("HTTP {}{hint}", status.as_u16())));
                off.expired = expired;
                return off;
            }
            match r.json::<Value>().await {
                Ok(v) => {
                    let buckets = parse(&v, now);
                    if buckets.is_empty() {
                        ClaudeLive::off(true, Some("no usage windows in response".into()))
                    } else {
                        ClaudeLive { configured: true, ok: true, expired: false, error: None, buckets }
                    }
                }
                Err(e) => ClaudeLive::off(true, Some(format!("invalid JSON: {e}"))),
            }
        }
        Err(e) => ClaudeLive::off(true, Some(format!("request error: {e}"))),
    }
}

/// Parse the normalized `limits[]` array into display buckets.
pub fn parse(v: &Value, now: DateTime<Utc>) -> Vec<Bucket> {
    let Some(limits) = v.get("limits").and_then(|l| l.as_array()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for lim in limits {
        let kind = lim.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        let pct = lim.get("percent").and_then(value_as_f64).unwrap_or(0.0);
        let severity = lim.get("severity").and_then(|s| s.as_str()).unwrap_or("normal");
        let resets_at = lim.get("resets_at").and_then(|r| r.as_str());
        let scope_model = lim
            .get("scope")
            .and_then(|s| s.get("model"))
            .and_then(|m| m.get("display_name"))
            .and_then(|d| d.as_str());

        let (name, sub) = match kind {
            "session" => ("Session".to_string(), "5-hour window".to_string()),
            "weekly_all" => ("Week · all models".to_string(), "resets weekly".to_string()),
            "weekly_scoped" => (
                format!("Week · {}", scope_model.unwrap_or("scoped")),
                "resets weekly".to_string(),
            ),
            other if !other.is_empty() => (titleize(other), String::new()),
            _ => continue,
        };

        let (status, status_label) = severity_status(severity);
        out.push(Bucket {
            name,
            sub,
            used_fmt: String::new(),
            used_pct: (pct * 10.0).round() / 10.0,
            left_pct: ((100.0 - pct) * 10.0).round() / 10.0,
            left_fmt: String::new(),
            limit_fmt: String::new(),
            reset: countdown(resets_at, now),
            status: status.to_string(),
            status_label: status_label.to_string(),
            live: true,
        });
    }
    out
}

fn severity_status(sev: &str) -> (&'static str, &'static str) {
    match sev {
        "normal" | "ok" | "low" => ("ok", "Healthy"),
        "warning" | "warn" | "approaching" | "medium" => ("warn", "Watch"),
        _ => ("danger", "Near limit"),
    }
}

fn countdown(resets_at: Option<&str>, now: DateTime<Utc>) -> String {
    let Some(ts) = resets_at else { return "—".to_string() };
    let Ok(reset) = DateTime::parse_from_rfc3339(ts) else { return "—".to_string() };
    let secs = (reset.with_timezone(&Utc) - now).num_seconds();
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

fn titleize(s: &str) -> String {
    s.replace('_', " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-17T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn parses_real_limits_array() {
        let v = json!({
            "limits": [
                { "kind": "session", "percent": 55, "severity": "normal", "resets_at": "2026-06-18T00:49:59+00:00" },
                { "kind": "weekly_all", "percent": 22, "severity": "normal", "resets_at": "2026-06-22T23:59:59+00:00" },
                { "kind": "weekly_scoped", "percent": 0, "severity": "normal", "resets_at": null,
                  "scope": { "model": { "display_name": "Sonnet" } } }
            ]
        });
        let b = parse(&v, now());
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].name, "Session");
        assert_eq!(b[0].used_pct, 55.0);
        assert!(b[0].live);
        assert_eq!(b[0].reset, "4h 49m");
        assert_eq!(b[1].name, "Week · all models");
        assert_eq!(b[2].name, "Week · Sonnet");
        assert_eq!(b[2].reset, "—");
    }

    #[test]
    fn missing_limits_is_empty() {
        assert!(parse(&json!({ "foo": 1 }), now()).is_empty());
    }

    #[test]
    fn severity_maps_to_status() {
        let v = json!({ "limits": [
            { "kind": "session", "percent": 95, "severity": "critical", "resets_at": null }
        ]});
        let b = parse(&v, now());
        assert_eq!(b[0].status, "danger");
    }
}
