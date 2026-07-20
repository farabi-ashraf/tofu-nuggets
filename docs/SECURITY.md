# Security & secret hygiene

Short, practical rules for keeping this public repo clean. Read before committing
anything related to signing, releases, or configuration.

## Never commit

- Private keys of any kind — the Tauri updater **private** key, `.pem`, `.key`,
  `.pfx`, `.p12`.
- Passwords, tokens, API keys, `TAURI_SIGNING_PRIVATE_KEY`, its password.
- `.env` / `.env.*` files.
- Machine-local Claude config (`.claude/settings.local.json`, `.claude/launch.json`) —
  it leaks local usernames and absolute paths.
- Logs (`*.log`) — may contain absolute paths and desktop layout.

All of the above are in `.gitignore`. Keep them there; do not `git add -f` past it.

## How update signing keys flow (public vs private)

The updater uses one keypair. The two halves go to two very different places:

| Half | Where it lives | Committed? |
|---|---|---|
| **Public key** | `plugins.updater.pubkey` in `app/src-tauri/tauri.conf.json` | **Yes** — safe, it only *verifies* signatures |
| **Private key** | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY` | **Never** — signs releases; anyone with it can ship malware as us |
| **Private-key password** | GitHub Actions secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | **Never** |

CI (`tauri-apps/tauri-action`) reads the two secrets from the Actions environment at
build time, signs the installer + `latest.json`, and publishes the release. The private
key never touches the repo, a developer machine's tracked files, or a log line.

Generate with `npx tauri signer generate`. Store the private key + password **only** in
your password manager and as the two GitHub Actions secrets. Delete any local copy of
the private key file after adding the secret.

## Pre-public / pre-release history scan (run before every visibility or release change)

History is public forever once pushed. Before flipping visibility or cutting a release:

```bash
# 1. Secrets across ALL commits, not just the current tree
git log --all -p | grep -nEi \
  'BEGIN [A-Z ]*PRIVATE KEY|api[_-]?key|secret|password\s*[:=]|token|AKIA[0-9A-Z]{16}|ghp_|github_pat_|TAURI_SIGNING_PRIVATE_KEY\s*[:=]'

# 2. Local username / absolute-path leaks
git log --all -p | grep -nEi 'C:\\\\Users\\\\[^\\]+|/Users/[^/]+|\.env'

# 3. Files that should never have been committed
git log --all --diff-filter=A --name-only --pretty=format: | sort -u \
  | grep -Ei '\.(key|pem|pfx|p12|env|log)$|settings\.local'
```

A clean run = no output. If anything appears, treat it as an incident (below) before
going public.

## Reporting a vulnerability

Do **not** open a public issue for a security problem. Email the maintainer at
**farabigithub@gmail.com** with steps to reproduce and impact. Expect an
acknowledgement within a few days.

## If a secret ever leaks

Order matters — **rotate first, purge second.** History rewrites do not un-leak a
secret that has already been pushed; assume it is compromised the moment it lands on a
public remote.

1. **Rotate immediately.** Revoke/regenerate the exposed key or token (for the updater
   key: generate a new keypair, update `pubkey` in `tauri.conf.json`, replace both
   Actions secrets). The old signature is now worthless to an attacker.
2. **Purge from history.** Remove the file/string from every commit with
   `git filter-repo --path <file> --invert-paths` (or `--replace-text`), then
   `git push --force`.
3. **Invalidate downstream.** If a release was signed with a leaked key, pull or
   supersede it.
4. **Record it.** Note what leaked and the rotation in `MEMORY.md` so the next session
   knows the key changed.

## Accepted findings (not affected)

Advisories that are real upstream but unreachable in the shipped product. Re-check when
the underlying dependency moves.

- **`glib` < 0.20 (RUSTSEC-2024-0429, moderate)** — pulled only through the GTK stack
  (`glib → gtk 0.18 → tao/webkit2gtk → tauri`), which Tauri gates behind
  `cfg(target_os = "linux")`. The build is Windows-only, so glib is never compiled or
  shipped: `cargo tree -i glib --target x86_64-pc-windows-msvc` prints nothing. It cannot
  be bumped in isolation (`gtk 0.18` pins `glib ^0.18`, and Tauri 2.11 owns the gtk
  stack), which is why Dependabot's auto-update job fails. Dependabot alert dismissed as
  "Vulnerable code is not actually used." Revisit if/when a Linux build is added or Tauri
  moves to glib ≥ 0.20.
