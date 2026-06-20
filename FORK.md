# Fork

This repository is a fork of [dennisrongo/agent-status](https://github.com/dennisrongo/agent-status), maintained by [nbergman](https://github.com/nbergman).

## Remotes

```bash
git remote -v
# origin    https://github.com/nbergman/agent-status.git
# upstream  https://github.com/dennisrongo/agent-status.git
```

Pull upstream fixes:

```bash
git fetch upstream
git merge upstream/main   # or rebase, per your preference
```

## Distribution

| Setting | Value |
|---------|-------|
| Bundle ID | `com.nbergman.agentstatus` |
| Updater | `https://github.com/nbergman/agent-status/releases/latest/download/latest.json` |
| Signing | Configure your own `TAURI_SIGNING_*` keys before shipping releases (see `docs/RELEASE.md`) |

The updater pubkey in `tauri.conf.json` is inherited from upstream until you generate a new keypair for this fork's releases.

## Roadmap

Multi-agent refactor (Claude, Codex, Grok, GLM) with a unified provider registry — see terminal research notes from Jun 2026.
