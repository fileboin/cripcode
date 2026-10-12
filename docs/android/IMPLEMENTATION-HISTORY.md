# CripCode Android — Implementation History

A verified, chronological record of the Android work: what was committed, what
each commit actually changed, and what is still uncommitted. Read
[README.md](README.md) first.

Every commit hash below was checked against `git log` in this repository. File
counts and line counts come from `git log --stat`. Nothing here is inferred from
memory.

---

## The Android work is unpushed

**This is the reason a previous agent could not find the Android history.**

| | |
|---|---|
| Local branch | `feat/isolate-cripcode` |
| Local HEAD | `1ebcec75` |
| `origin/feat/isolate-cripcode` | `10dc121b` |
| Commits ahead of origin | **15 — every Android commit** |

`10dc121b` (`chore(release): prepare v0.18.8`) is the commit immediately
**before** the first Android commit. So the Android work exists **only in this
local working copy**. A fresh clone from GitHub contains none of it.

Reproduce with:

```bash
git log --oneline origin/feat/isolate-cripcode..HEAD
```

Pushing is out of scope for this documentation pass and requires explicit
approval.

---

## Committed Android work — oldest first

All 15 commits, 2026-09-14 → 2026-09-23.

### 1. `b420325b` — 2026-09-14 16:48:52 — `feat(android): add Android mobile workspace foundation`

The entry point. 87 files, +1276 / −38.

- Committed the whole Android app module `src-tauri/gen/android/`: Gradle
  wrapper, `build.gradle.kts` files, `settings.gradle`, `buildSrc` Kotlin
  (`BuildTask.kt`, `RustPlugin.kt`), `MainActivity.kt`, manifest, resources,
  launcher icons, themes, `file_paths.xml`.
- Bundled assets: project templates (`nextjs-basic`, `expo-mobile`,
  `flutter-mobile`, …), Nerd Font files, `scrcpy-server.jar`.
- Added `src-tauri/src/android_paths.rs` — the process-global app data dir.
- Wired Android startup in `lib.rs` `.setup()`: set the app data dir,
  initialise logging, read app state, init the default agent, init analytics.
- Mobile workspace UI: `CompactTopbar`, `compact-workspace.css`, mobile-safe
  window handling, app command palette entries.

### 2. `c635626e` — 2026-09-14 20:05:26 — `feat: add Android SSH PTY seam`

3 files, +286 / −103. Introduced `commands/session_backend.rs` and the
`SessionBackend` trait, so a terminal session's byte transport is an interface
rather than a hard-wired PTY process. This is the seam that later let Android
substitute an SSH channel with zero changes above it.

### 3. `5cb6ba30` — 2026-09-14 21:06:22 — `feat: add Android russh SSH transport`

8 files, +1917 / −174. The core of the whole feature.

- Added `russh = 0.63.3` (features `ring`, `rsa`, `flate2`) as an **Android-only**
  target dependency. The comment in `Cargo.toml` records why `ring` and not
  `aws-lc-rs`: `ring` keeps the NDK build toolchain-free.
- New `commands/ssh/transport_russh.rs`: connect/authenticate, PTY open, the
  async channel pump, and the app-private host-key probe + strict handler.
- Desktop gains `jni`-free code paths untouched; every Android branch is behind
  `#[cfg(target_os = "android")]`.
- Adapted `pty_session.rs` and `session_backend.rs` to open a remote session
  through the seam.

### 4. `cfa80c8b` — 2026-09-14 21:33:36 — `fix: use Android app-private paths for remote config`

3 files, +102 / −30. Android has no home directory, so `get_config_path()` in
`ssh/config.rs`, `ssh/remote_projects.rs` and `ssh/ollama.rs` became
platform-branched: app-private data dir on Android, `~/CripCode/.cripcode/…`
unchanged on desktop.

### 5. `34bd0bb4` — 2026-09-14 21:57:53 — `feat: add Android SSH private-key import`

6 files, +281 / −7. New `ssh/key_import.rs` + the `import_ssh_key` command, and
UI in `AddServerModal` so a key can be pasted on a phone. Android-only
registration in `lib.rs`. 7 unit tests including "storage failure fails closed
and never echoes the key".

### 6. `9fb70102` — 2026-09-15 17:47:46 — `fix: map Android SSH exit status correctly`

3 files, +217 / −5. A real device-test finding: SSH servers may deliver
`exit-status` **after** channel EOF, and stopping the pump at EOF turned that
race into a permanent `-1`. New `ssh/exit_tracker.rs` makes the ordering rules
explicit and transport-independent, with a 10-test suite including
`status_after_eof_is_still_captured` and `pump_lifecycle_matrix`.

### 7. `76324c39` — 2026-09-15 18:51:58 — `feat: add Android russh exec transport`

4 files, +287 / −134. New `ssh/exec.rs`: the remote command seam. Desktop spawns
the `ssh` CLI exactly as before; Android runs the same command over
`transport_russh::exec`. `ssh/files.rs` was migrated onto it (net −134 lines
there).

### 8. `5e28658f` — 2026-09-15 19:15:32 — `feat: move Android remote git to russh exec transport`

1 file, +7 / −16. `ssh/remote_git.rs` now calls `exec::run` on both platforms.

### 9. `db2cc7f3` — 2026-09-15 19:33:01 — `feat: move Android remote build and dev server to russh exec transport`

2 files, +8 / −12. Same migration for `remote_build.rs` and
`remote_dev_server.rs`.

### 10. `1661ed8c` — 2026-09-15 19:51:17 — `feat: move Android Ollama to russh exec transport`

1 file, +16 / −29. `ssh/ollama.rs` migrated.

### 11. `6d446b88` — 2026-09-15 21:02:10 — `feat: add Android russh tunnel for remote preview`

2 files, +261 / −85. `start_tunnel` in `transport_russh.rs`: one authenticated
SSH connection, one `direct-tcpip` channel per accepted local connection, a
`JoinSet` for in-flight copies, an `AliveGuard`, and a ticker that detects a
dropped SSH connection. `remote_preview.rs` keeps one registry for both
platforms, differing only in the `TunnelInfo` payload (PID vs `TunnelHandle`).

### 12. `60f028b3` — 2026-09-15 21:59:02 — `fix: allow Android readiness for remote workflow`

1 file, +40. Desktop setup gates on a local toolchain (`homebrew`, `node`,
`git`, `gh`). On Android there is no local toolchain — the VPS provides it — so
readiness became "at least one SSH server is configured". Pure function
`android_base_ready(configured_server_count)`, unit-tested; the desktop branch
is untouched and the downstream gates stay in place.

### 13. `5680ca01` — 2026-09-15 22:32:26 — `fix: detach remote background commands from stdin`

2 files, +6 / −3. Another device-test finding: `nohup bash -c … &` inherited the
SSH channel's stdin, so the russh exec never saw the channel close. Added
`< /dev/null` to the build-start and dev-server-start commands, with a test
asserting it.

### 14. `f652a4cf` — 2026-09-22 20:18:18 — `fix(android): add visible SSH Servers entry point on dashboard`

1 file, +13 / −3. The remote flow had no reachable entry point on the Android
dashboard.

### 15. `1ebcec75` — 2026-09-23 06:23:40 — `feat(android): in-app update flow (versionCode check, verified download, installer handoff)`

12 files, +856 / −4. New `commands/android_update.rs` (~506 lines) plus
`AndroidUpdateBanner.tsx`, `src/lib/androidUpdate.ts` and 110 lines of frontend
tests. `build.rs` forwards `tauri.android.versionCode` into the binary as
`ANDROID_VERSION_CODE`. The manifest gained the `FileProvider` entry needed for
the installer handoff. Details in [ARCHITECTURE.md §6](ARCHITECTURE.md#6-in-app-update-android-only).

---

## Uncommitted work

Present in the working tree, **not committed**. Treat it as in-progress. Read it
before changing anything nearby — some of it is the fix for issues still listed
as open.

### Backend

| File | Δ | What |
|---|---|---|
| `src-tauri/src/commands/ssh/secrets.rs` | +65 | New `SessionSecretStore` (process-lifetime in-memory) and `store_for_platform()`; desktop keeps `KeyringStore`. Test for round-trip + idempotent delete. |
| `src-tauri/src/commands/ssh/transport_russh.rs` | +114 | **Password auth enabled on Android** via `store_for_platform()`; `open_shell` takes `Option<&str>` so the plain Terminal gets a login shell instead of exec'ing `user@host`. |
| `src-tauri/src/commands/ssh/config.rs` | +155 | `add`/`update`/`delete` routed through `store_for_platform()`; the Android "password not available" fail-closed blocks removed; a blank password field on a config-only edit now succeeds even when the session store is empty. 2 new tests. |
| `src-tauri/src/commands/pty_session.rs` | +103 | `android_remote_command` rewritten as a pure, host-testable argv parser returning `Option<String>`; 3 new tests. |
| `src-tauri/src/commands/android_update.rs` | +436 | Progress/complete/failed events, `CRIPCODE_UPDATE_DIAG` diagnostics, shared-upload dedupe, bridge-failure classification. |
| `src-tauri/src/commands/update_diagnostics.rs` | new (untracked) | Reads local `CRIPCODE_UPDATE_DIAG` log lines and opens the Android share sheet. 6 tests. |
| `src-tauri/src/commands/ssh/files.rs` | +49 | `[ANDROID-DIAG]` tracing around server lookup (path, count, ids, FOUND/NOT_FOUND). |
| `src-tauri/src/commands/ssh/remote_projects.rs` | +53 | Diagnostics for the remote-projects config path. |
| `src-tauri/src/commands/ssh/{remote_build,remote_dev_server}.rs` | +39 / +35 | Further Android-side adjustments. |
| `src-tauri/src/commands/ssh/remote_agent.rs` | +6 | `[ANDROID-DIAG]` server-id tracing. |
| `src-tauri/src/logging.rs` | +2 | `get_log_dir()` made `pub(crate)` for diagnostics. |
| `src-tauri/src/lib.rs` | +5 | Registers `update_diagnostics_log` + `android_share_text`. |
| `src-tauri/build.rs` | (committed earlier) | — |

### Frontend

| File | Δ | What |
|---|---|---|
| `src/components/ssh/AddServerModal.tsx` | +182 | **Duplicate-server fix.** One form now yields at most one server: the first Test persists a single draft (`testDraftId`), later Test/Save update that same id, and closing without saving deletes the draft. Plus `diagLog` tracing and no auto-focus on Android (soft keyboard vs fixed-overlay hit-testing). |
| `src/components/AndroidUpdateBanner.tsx` | +92 | Update diagnostics entry point. |
| `src/lib/androidUpdate.ts` | +218 | Event-based download with a terminal-event fallback when the invoke response is lost. |
| `src/lib/androidServerDiag.ts` | new (untracked) | `androidServerDiag()` — `[ANDROID-DIAG]` console + logger lines, no-ops off Android. |
| `src/lib/diagLog.ts` | new (untracked) | General diagnostic logger. |
| `src/components/AndroidUpdateDiagnosticsModal.tsx` | new (untracked) | Local view/copy/share of update diagnostics. |
| `src/components/ssh/AddServerModal.test.tsx` | +85 | Tests for the single-server behaviour. |
| `src/components/AndroidUpdateBanner.test.tsx` | +86 | Update-flow tests. |
| `src/lib/{androidUpdate,updateDiagnostics}.test.ts` | new (untracked) | Update/diagnostics tests. |
| `src/components/ssh/AddRemoteProjectModal.tsx` | +72 | Remote-project mapping UI. |
| `src/components/ssh/ServerList.tsx` | +14 | Server list layout. |
| `src/components/dashboard/AgentLaunchPanel.tsx` | +36 | Agent launch surface. |
| `src/main.tsx`, `src/hooks/useAppSetup.ts`, `src/components/ssh/RemoteAgentPanel.tsx`, `src/components/ssh/RemoteProjectList.tsx`, `src/lib/{ptySession,remoteAgent}.ts` | small | Wiring and diagnostics. |
| `src/styles/features/{ssh,update-banner}.css` | +7 / +96 | Styles. |

### Build / configuration

| File | Δ | What |
|---|---|---|
| `src-tauri/gen/android/app/build.gradle.kts` | +17 | `signingConfigs { release { … } }` read from `keystore.properties`, applied to the `release` build type **only when that file exists**. |
| `scripts/conditional-build.mjs` | new (untracked) | `beforeBuildCommand` gate. Vite always rewrites `dist` with fresh mtimes and Tauri embeds `dist` via `include_bytes!`, so an unchanged frontend still forced a full Rust recompile on every Android build. The script hashes everything that can change the Vite/tsc output and skips `pnpm build` when the existing `dist` came from exactly that input. |
| `src-tauri/tauri.conf.json` | ±6 | `beforeBuildCommand` → `node scripts/conditional-build.mjs`; version → `0.18.21`; `android.versionCode` → `18020`. |
| `src-tauri/Cargo.toml` | ±1 | version → `0.18.18`. |
| `src-tauri/gen/android/.gitignore` | ±3 | — |

### Not part of the Android work

Five stray untracked files matching
`src-tauri/C?\u{200}\u{200}Usersbojan…ss-kill-test-<pid>-survived-marker` are
Windows console-encoding artefacts from PTY kill tests. They are unrelated to
Android and were left untouched.

---

## Documented history before Android

For context on where Android work started: `2ead514e` (`feat(remote): SSH remote
workspace, host-key gate, and generic credentials`) and the commits before it
built the desktop SSH subsystem that Android later mirrors. Android reuses that
subsystem's policy and UI; it replaces only the transport.

---

## Related documents

- [README.md](README.md)
- [ARCHITECTURE.md](ARCHITECTURE.md)
- [TEST-RESULTS.md](TEST-RESULTS.md)
- [KNOWN-ISSUES.md](KNOWN-ISSUES.md)
- [BUILD-AND-SIGNING.md](BUILD-AND-SIGNING.md)
- [NEXT-STEPS.md](NEXT-STEPS.md)