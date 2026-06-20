---
name: agent-status-provider-integration
description: "Research and integrate AI coding CLI providers into agent-status as equal registry-backed providers rather than Claude-centric bolt-ons."
trigger: "Use when adding, auditing, or redesigning provider support for Claude, Codex, Grok, GLM, Cursor, or other AI coding CLIs in agent-status."
author: n.bergman
source_sessions:
  - n.bergman_n.bergman's Organization_default_00406699-aa58-4d8d-b1a8-916c95a25037
  - n.bergman_n.bergman's Organization_default_019ee324-06c4-7671-8309-ced93ce027a8
  - n.bergman_n.bergman's Organization_default_47df0f43-9242-4cc1-887d-777c6974de88
contributors:
  - n.bergman
version: 1
created_by_agent: codex
created_at: 2026-06-20T03:53:51.837Z
updated_at: 2026-06-20T03:53:51.837Z
---

# Agent Status Provider Integration

## When to use

Use this skill when work touches provider support in `agent-status`: adding a new AI coding CLI, researching what a CLI exposes, fixing provider-specific usage data, or refactoring the app so Claude is one equal provider instead of the app's implicit center.

Do not use for macOS or Windows release work; use `release-macos` or `release-windows` for that.

## Workflow

1. Start with a provider data map before coding. Inspect the real local state and redact secrets:
   - Claude: `~/.claude/projects/**/*.jsonl`, macOS keychain item `Claude Code-credentials`, fallback `~/.claude/.credentials.json`
   - Codex: `~/.codex/sessions/**/rollout-*.jsonl`, `~/.codex/archived_sessions/**/rollout-*.jsonl`, `~/.codex/auth.json`
   - GLM/z.ai: `~/.zai/**`
   - Grok: local session folders/logs if present

2. Inventory the CLI surface. Run the provider help command and look for usage, limits, login, status, auth, and doctor surfaces:
   ```bash
   claude --help
   codex --help
   codex login status
   codex doctor --json
   ```
   For slash-command-only data, run it inside a real session when needed, and capture the exact output shape.

3. Deep-map log schemas. Pick the two newest session/log files by mtime, list distinct event or record types, and identify JSON paths for tokens, model, session id, timestamps, cwd/project, limits, and cost-relevant fields. For Codex, check `payload.rate_limits`, `payload.info.last_token_usage`, `payload.info.total_token_usage`, and `session_meta`.

4. Check for live usage APIs only after local schema is understood. Document request/response shape with redacted tokens. Known examples:
   - Claude live usage: `GET https://api.anthropic.com/api/oauth/usage`
   - Codex may use ChatGPT backend auth from `~/.codex/auth.json`; verify the exact endpoint and 200/403 behavior before depending on it.

5. Fit the provider into a shared contract, not a one-off UI path. Every provider should be expressible as:
   - `id`, display label, auth state
   - `buckets` for limits/windows
   - `sessions` with provider, id, project/cwd, model, tokens, cost if known, timestamp
   - `models`, `kpi`, data source, staleness, confidence/note

6. Audit Claude-centrism before and after changes. Read these files when relevant:
   - `src/App.tsx`
   - `src/components/Meter.tsx`
   - `src/components/ProvidersPanel.tsx`
   - `src-tauri/src/scanner/mod.rs`
   - `src-tauri/src/commands/usage.rs`
   - `src-tauri/src/vendors/claude.rs`
   - `README.md` and About copy

7. Backend changes should move toward a registry-shaped pipeline. Avoid manually plugging providers after a Claude-first scan. Keep local scanners and live vendors separate, then merge into one provider snapshot.

8. Verification should include Rust tests, frontend build, and a UI check when visual behavior changes:
   ```bash
   npm run build
   cargo test --manifest-path src-tauri/Cargo.toml
   ```

## Anti-patterns

- Treating Claude as the default provider with fallbacks like `snapshot.detection?.claude ?? true`.
- Hardcoding UI labels such as `live · Claude` inside shared components like meters.
- Showing Claude-only footer KPIs while the selected Overview provider is Codex, Grok, or GLM.
- Making the Sessions tab Claude-only once other providers expose session metadata.
- Ignoring archived Codex sessions; `~/.codex/archived_sessions/**/rollout-*.jsonl` can contain real usage history.
- Assuming every provider has tokens, cost, or live quota. Show honest blanks or notes when data is unavailable.
- Reverse-engineering Claude project paths from folder names; the encoding is lossy. Prefer `cwd` or project fields inside records.
- Depending on private live APIs without documenting auth refresh, error behavior, polling limits, and fallback to local logs.
