# Cleaner privacy posture

Cleaner is local-only by design. This file is a verified audit, current as of Phase 17 (all 14
scanner categories implemented) — not an aspiration. Each claim below states how it was checked, so a
future change that breaks one is easy to re-verify with the same command.

## No network capability in Cleaner's own code

```
grep -rln "reqwest\|TcpStream\|std::net::" src/cleaner/
```

returns nothing. dodo as a whole links `reqwest` (the in-app updater) and `tokio` (the PostgreSQL
driver), but nothing under `src/cleaner/` imports either — this is not a policy Cleaner promises to
follow, it is a fact about which crates its own source names. There is no code path by which a scan
result, a file path, or a cleanup report could reach the network, because no networking API is ever
called from this module.

## No telemetry, no remote upload

Every scan result stays in `CleanerState` (in-memory, per-session) until the user closes the app or
starts a new scan. Nothing is written anywhere except:

- the moved-to-Trash files themselves (the point of "Clean selected"),
- `cleaner-ignored-items.json` (Phase 10's "Keep" list — the ignored path strings the user
  marked, nothing else), stored under `data_dir()` like every other Cleaner or dodo persisted file
  (never uploaded — see the persistence paragraph in `CLAUDE.md`).

"Export a local scan report" (listed under required UI interactions) is not implemented yet — see
`docs/cleaner/known-limitations.md`. When it lands, it must write to a user-chosen local file only.

## No content inspection beyond structural metadata

Verified per category, by reading what each scanner actually opens with `fs::read`/`read_to_string`/
`plist::Value::from_file` rather than assuming from its description:

- **Mail Files** (`macos::scanners::mail_files`) never reads a file's contents — only
  `fs::read_dir` to list attachment names, sizes and modified times. It cannot see a message body,
  because it never opens a `.emlx`/mbox file to look.
- **AI Apps** (`macos::scanners::ai_apps`, `ai_app_providers`) never opens a prompt/history file or a
  model's weight file. The one exception — Ollama's `collect_ollama_model_names` — reads only
  directory and file *names* inside the manifest tree, never a manifest's JSON body.
- **Universal Binaries** (`macos::scanners::universal_binaries`) reads a Mach-O executable's raw
  bytes, but only to parse the fat-header/architecture-slice structure (`object` crate) — never to
  execute it, and never to inspect anything the binary does at runtime.
- **Installed Apps / Orphaned Files / App uninstall** read `Info.plist` (bundle id, name, version —
  app metadata, not user content) and directory listings only.
- **Node Tooling / Homebrew / Xcode / Docker Cache** all classify by path name and size; none opens a
  cache entry's content to decide what it is.
- The Language Files scanner reads `.GlobalPreferences.plist`'s `AppleLanguages` key (a language
  preference, not personal content) and each `.lproj` folder's *name*, never a `.strings`/`.stringsdict`
  file's contents.

No scanner in the current 14-category set reads a credential file's contents. The closest name-only
mentions — `~/.npmrc`, `~/.bunfig.toml`, Docker's `config.json`/certificates — are all documented as
explicitly out of scope in their respective scanner's doc comment, and `grep`ing for those filenames
across `src/cleaner/` turns up only those doc comments, never a `fs::read` call against them.

## Logging never carries sensitive content

`ScanWarning`/`ItemWarning` messages (the only free-form diagnostic strings this module produces)
are built from paths, counts and fixed English phrases — never a file's contents, a Mail attachment's
subject, an AI prompt, or credential material. Grepping every scanner's warning-construction call
sites confirms each one interpolates only a path, an error's `Display` output, or a literal string.

## Threat model notes carried over from the safety model

- The allow-list deletion model (`docs/cleaner/safety-model.md`) means a bug can, at worst, cause an
  incorrectly-scoped Trash move — never an upload, since there is no upload path to reach.
- The persisted `cleaner-ignored-items.json` file holds absolute paths, which on a single-user Mac
  reveal the username in `/Users/<name>/...` — the same fact every other file on the machine already
  reveals; this is not treated as a new exposure, since it never leaves the local `data_dir()` file.

## Future implementation requirements (unchanged from Phase 1)

- Keep Mail/AI prompt content out of logs and diagnostics if either scanner is ever extended to look
  at more than filenames.
- Keep scanner reports local unless the user explicitly exports to a local file (Phase 17 status: not
  yet implemented — see known-limitations.md).
- Any future privileged helper (root-owned System Junk locations, Docker VM internals) must use
  authenticated XPC, a fixed request schema and no arbitrary command execution — see
  `docs/cleaner/safety-model.md`.
