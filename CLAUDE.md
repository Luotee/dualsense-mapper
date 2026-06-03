# dualsense-mapper — Contributor Guidelines

## If You Are an AI Agent

Read this section before doing anything.

**Your job is to protect your human partner.** A change that doesn't fix a
real, observed bug or doesn't deliver a feature the user asked for is
churn — discard it.

Before opening a PR against this repo, you MUST:

1. **Verify a real problem exists.** No speculative fixes. If asked to
   "improve X" without a failure case, ask for one.
2. **Run `cargo test` before and after.** Note the baseline (all green
   today) and the result. New tests for new bugs are mandatory.
3. **One concern per PR.** Don't bundle unrelated changes. The exception:
   a feature commit + its CHANGELOG entry + its config-example update
   belong in the same PR because they are the same concern.
4. **No scope creep.** Task is "fix L2 macro" → fix only that.
5. **Never modify `legacy-python/`.** That tree is the frozen May-2025
   Python POC kept as historical reference and as a working fallback on
   macOS until the Rust Phase 2 build lands. Feature work goes in `rust/`.

## Repo layout

| Folder | Status | What lives here |
|---|---|---|
| `legacy-python/` | **Frozen.** Do not modify. | Original Python POC (`dualsense-mac-mapper.py` + README). |
| `rust/` | Active. All feature work here. | Single-binary Rust app. Cargo crate at `rust/Cargo.toml`. |
| `docs/superpowers/` | **Gitignored.** Local working state. | Brainstorming specs, implementation plans. Never committed. |

## Spec / plan artifact handling

Files produced by `superpowers:brainstorming`, `superpowers:writing-plans`,
and any other intermediate design / plan artifacts under
`docs/superpowers/specs/` or `docs/superpowers/plans/` are **never**
tracked. Implementation lands in code + CHANGELOG, not in spec/plan
markdown.

The repo `.gitignore` excludes `docs/superpowers/` for this reason. Do
not stage these files; do not bypass the ignore with `git add -f`.

## Build & test (Linux dev host)

```bash
sudo apt install -y pkg-config libudev-dev   # gilrs Linux backend deps
cd rust
cargo test                                    # 33 tests, ~0.2s
cargo build --release                         # native Linux binary
```

## Cross-compile to Windows (canonical ship target)

There are TWO build profiles. Pick the right one:

### CLI build (no GUI) — minimal sanity check only

```bash
cd rust
cargo build --release --target x86_64-pc-windows-gnu
# → target/x86_64-pc-windows-gnu/release/dualsense-mapper.exe  (~2 MB)
```

This builds the `--cli` path WITHOUT the Tauri GUI feature. The exe is
small but USELESS for end-user testing — double-clicking it opens a
console window, not the GUI. Do NOT ship this. Use it only for
verifying the CLI engine compiles cleanly.

### GUI build (canonical ship target — what the user actually wants)

```bash
cd rust
WEBVIEW2_STATIC=true cargo xwin build --release \
  --target x86_64-pc-windows-msvc --features gui
# → target/x86_64-pc-windows-msvc/release/dualsense-mapper.exe  (~11 MB)
```

This is the ONLY exe to deliver to the user.

- `--features gui` enables the Tauri frontend (engine + web/).
- `cargo xwin` builds for the MSVC target from Linux. `cargo-xwin`
  must be installed (`cargo install cargo-xwin`); it fetches the
  Windows SDK + MSVC libs into `~/.cache/cargo-xwin/xwin/` on first
  run.
- `WEBVIEW2_STATIC=true` statically links the WebView2 loader so the
  exe has **no DLL dependency**. Without this env var, the exe needs
  `WebView2Loader.dll` next to it and double-clicking on a fresh
  machine fails.
- Targeting `windows-msvc` (not `windows-gnu`) avoids shipping
  `libgcc_s_seh-1.dll`, `libstdc++-6.dll`, `libwinpthread-1.dll`.

The Windows binary needs **only the exe + a `dualsense-mapper.json`
config next to it**. No DLLs. Ship those two files; everything else
is dev artefacts.

### Test-drop convention (Linux/WSL dev host)

After every fresh GUI build the user wants for testing, copy ONLY the
exe to a versioned folder under the Windows-visible Downloads path:

```bash
DEST="/mnt/c/Users/Joe96/Downloads/dualsense-mapper-vX.Y.Z-<tag>"
mkdir -p "$DEST"
cp rust/target/x86_64-pc-windows-msvc/release/dualsense-mapper.exe "$DEST/"
```

**Do NOT bundle `config.example.json`.** The exe auto-generates a
fresh `dualsense-mapper.json` next to itself on first run (see
`Config::load_or_create`). Copying an extra example file is redundant
and gives the user two configs to reason about.

`<tag>` is one of: `test` (final), `dev-test` (pre-release dev build),
or a feature name like `silhouette-test`. The folder pattern matches
prior releases (`dualsense-mapper-v2.1.0-test/`, etc.) so the user
can find it by version. Do NOT skip this step when finishing a
feature — the user expects the exe in Downloads, not just a
SendUserFile attachment.

### What "I built the exe" means without these steps

It means you built the CLI binary, not the GUI binary. That is not
useful to the user. Re-read this section before assuming a one-liner
`cargo build` is enough.

## GUI frontend verification (browser click-through)

Any change to `rust/web/` (the Tauri frontend) MUST be verified by
actually loading the page in a browser and clicking every control —
not by reading the diff and asserting it looks right. "It compiles"
is not verification for a chrome layer that only fails at runtime.

The catch: `rust/web/ipc.js` throws on load if `window.__TAURI__` is
missing, so the frontend **cannot be opened in a plain browser**. You
must inject a mock `window.__TAURI__` (an `core.invoke` switch + an
`event.listen` registry) **before** the module scripts run, feed
`rust/config.example.json` as the `get_config` reply, and serve
`rust/web/` over a static HTTP server.

Reproducible recipe (Linux dev host — Playwright + cached chromium,
headless, no display needed):

```bash
# cached browser already at ~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome
cd /tmp && npm init -y && PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1 npm i playwright-core
# drive.mjs: addInitScript injects the __TAURI__ mock, page.goto the static
# server, then click #btn-activity, #btn-settings, every [data-tab], every
# #pane-macros/#pane-settings button, each #chip-list .chip and controller
# hotspot (opens .bind-popup-overlay), #btn-disconnect.
```

The IPC commands a mock must answer: `get_controller_status`,
`get_config`, `get_ui_prefs`, `get_app_version` (others can return
`null`). Events the frontend listens for: `controller-status`,
`button-down/up`, `touchpad-click/hover`, `activity`,
`config-changed`.

What a PASS requires, captured as evidence:
- Every toolbar / tab / pane button clicks without throwing.
- Clicking a `#chip-list .chip` or a controller hotspot opens the
  `.bind-popup-overlay` (Key/Macro/Mouse/Unbound + Cancel/Save).
- `window` collects **zero** `error` / `unhandledrejection` /
  `pageerror`. (A `GET /favicon.ico 404` is the one expected,
  harmless miss.)
- Screenshots of the main pane + an open bind popup.

For scroll / layout bugs, measure don't eyeball: across several
viewport widths assert `documentElement.scrollWidth == clientWidth`
(no horizontal overflow) and read computed `overscroll-behavior-x`.
The window is a fixed `100vh` shell (`.app { overflow: hidden }`) and
`html, body { overscroll-behavior: none }` — nothing may rubber-band
or scroll the window itself.

## Iron rules for the Rust source

These are project-specific invariants. Breaking any of them silently
regresses real, observed bugs the codebase already fixed once:

1. **`config.rs::Config::validate` must reject any config that does not
   list every button id `0..=28`.** Missing ids are a user error, not a
   silent skip — except for the v2.1.0 touchpad-quadrant additions
   (25..=28), which the load path auto-fills as `Unbound` so v2.0
   configs continue to parse. The validator itself still requires the
   full range to be present; auto-fill happens before validation, not
   inside it.
2. **`safety.rs` is the single source of truth for "what keys (and
   mouse buttons) are physically pressed right now."** `keyboard.rs`
   always asks `KeyState::press` / `release` for the edge transition
   and only forwards a real synth on `Edge::Press` / `Edge::Release`.
   `mouse.rs` mirrors this through `press_mouse` / `release_mouse`.
   Bypassing this leaks a stuck KEYDOWN or MOUSEDOWN — see commit
   `1d64f25` for the keyboard bug shape; the mouse half landed in
   v2.1.0.
3. **`KeyboardSink::Drop`, `MouseSink::Drop`, and the panic hook in
   `app.rs` must run `release_all_held` unconditionally.** They are
   the last-line defence against stuck keys / stuck mouse buttons when
   the process dies. `release_all_held` releases every held key **and**
   every held mouse button, and never waits on `min_press_ms` —
   shutdown beats anti-cheat profile.
4. **Macro threads drain every unmatched `Press` as `Release` on every
   exit path** (cancel flag, no-repeat completion, channel close). A
   macro cancelled between Press Left and the scheduled Release Left
   would otherwise strand a KEYDOWN that breaks every subsequent
   `Left` binding. See `macro_engine.rs::run_macro` and the
   `cancel_between_press_and_release_drains_held_keys` test.
5. **`min_press_ms`, `tick_jitter_ms`, and macro `delay_ms` are
   `[min, max]` ranges, never constants.** Config validator enforces
   `min < max`. Constant timing is the single most reliable
   anti-cheat fingerprint; do not add a "convenience" form that
   allows it.
6. **`gamepad.rs` is the platform-quirk layer.** It is the only place
   that knows D-pad can be either discrete buttons (`Button::DPadLeft`
   etc.) or a hat axis (`Axis::DPadX/Y`), and that triggers can be
   reported in `[-1, 1]` or `[0, 1]`. Mapper / keyboard see one
   uniform event surface. New platform quirks live here.
7. **No process hooking, no DLL injection, no driver.** User-mode
   `SendInput` (Windows) / `CGEvent` (macOS) only, via the `enigo`
   crate. Anything else lands us in actual cheat-software territory.
8. **The exe is double-clicked, not invoked from a terminal.** Primary
   end-user flow on Windows is Explorer → double-click → GUI window
   opens. That means:
   - GUI mode (default in v1.0.0+) is the user-facing path. The
     window must be visible within ~1 second of process start.
     Errors before window creation must render to a fallback modal
     (e.g. `MessageBoxW`), not flash and disappear.
   - `--cli` is the explicit legacy console-mode opt-in. In CLI mode,
     first-run may **not** call `anyhow::bail!` / exit non-zero
     before reaching the main loop. Any uncaught error from `main`
     must pause (read stdin) before exit, so the error is visible.
     `--no-pause` is the explicit opt-out for CI use.
   - The first-run-written `dualsense-mapper.json` carries an inline
     keyboard cheat sheet in `_help` and `_keyboard_keys` fields so
     a user editing the file in Notepad has the reference inline.
     `serde` ignores unknown fields, so these are documentation
     only — but they are load-bearing UX, not decoration.
9. **The GUI (`gui/` module + `web/` frontend) is a chrome layer.**
   No mapping decision, no key synth, no macro scheduling may live
   in JavaScript. The frontend captures input and renders state;
   everything that mutates runtime state goes through a Rust
   `#[tauri::command]` (see `rust/src/gui/commands.rs`) that calls
   the same engine code paths as the v0.1.x CLI. JS that calls
   `SendInput` (or the equivalent) is a rejection. The IPC surface
   is the only authorised path from frontend to engine — adding
   business logic to the frontend that bypasses it is a regression
   even if the feature works in isolation.
10. **Any IPC command that mutates `config` MUST emit
    `config-changed` itself on success.** The filesystem watcher
    in `runtime.rs` is the second source of truth, kept around
    for external edits (Notepad while the GUI is running). It is
    NOT a reliable refresh path for IPC-driven writes because
    `notify-rs` on Windows loses the watch handle across the
    atomic-rename used by `write_atomic` — every IPC mutator
    after the first would otherwise silently appear to do
    nothing. See v1.2.0 commits for the bug shape.
11. **Controller connection state is ground truth from raw
    HID, not heuristic.** v2.0.0 replaced the gilrs path with
    `hidapi` against the DualSense BT 0x31 report. The HID
    worker thread's state machine (Searching → Handshaking →
    Streaming) emits `Connected` on the first decoded 0x31
    frame and `Disconnected` on either read-error or
    `DISCONNECT_AFTER_TIMEOUTS` (50) consecutive 4 ms read
    timeouts (~200 ms). No more `is_connected()` cache, no
    more "must press a button to confirm" UX hack — the radio
    link presence is observed directly via report stream
    presence/absence.

## Anti-cheat self-discipline

This tool is not a cheat — it remaps physical inputs to keyboard
inputs. The synthesis pipeline avoids fixed-period patterns so it
doesn't get misclassified as one. The rules are listed in
`rust/README.md` § Anti-cheat self-discipline and enforced
mechanically by config validation + the iron rules above. Do not
ship a feature that requires bypassing them.

Note: **`SendInput` does not trigger Windows OS auto-repeat.** This is
expected — auto-repeat is a kbdclass-driver-level feature for real
hardware HID streams. Games that poll key state (the vast majority)
work identically to a real keyboard; only WM_CHAR-driven text input
visibly differs. If a `turbo: true` feature ever ships, gate it
behind explicit config opt-in with a jittered interval — never a
constant tick.

## Versioning rules

Project follows [Semantic Versioning](https://semver.org/) with these
project-specific clarifications. Picking the wrong axis is a
release-flow bug; halt and reclassify before committing.

- **MAJOR (`X.0.0`)** — backwards-incompatible behaviour. Examples:
  - Dropping support for a previously-supported pad (e.g. v2.0.0
    removes gilrs → non-DualSense pads no longer work).
  - Breaking the on-disk `dualsense-mapper.json` schema (renamed
    fields, removed keys, changed enum values).
  - Removing or renaming a CLI flag.
  - Changing the semantics of an existing binding type (e.g. if
    `Binding::Key` started ignoring the modifier).
  - Replacing the gamepad event source / abstraction layer.
- **MINOR (`X.Y.0`)** — new functionality, backwards compatible.
  Examples:
  - New IPC command added to `commands.rs`.
  - New binding type (`Binding::Touchpad`) added to the enum.
  - New optional config field with a default.
  - New keyboard mapping support (e.g. punctuation keys).
- **PATCH (`X.Y.Z`)** — bug fix only. No new feature, no schema
  change. Examples:
  - Fixing a stuck KEYDOWN.
  - Fixing the d-pad stick quarter rotation map.
  - Fixing the macro UI refresh trigger.
- **Pre-1.0** (`0.X.Y`) — semver suspended; MINOR may break.

When deciding between MINOR and MAJOR, ask: *will any user's existing
config file or hardware setup stop working after the upgrade?* If
yes, MAJOR.

## Release flow — files that must move together

A version bump touches **three** files in the release commit:

1. `rust/Cargo.toml` — `[package] version` field
2. `rust/tauri.conf.json` — `"version"` field (must match Cargo.toml)
3. `CHANGELOG.md` — new top-level `## [X.Y.Z] - YYYY-MM-DD` heading

Bumping fewer than three is a release-flow bug; halt. There is no
mechanical gate yet (no `tests/test_version_consistency.py`); a CI
addition is welcome but until then, reviewers enforce the rule by
eye on every release PR.

### Tagging

Release tag uses plain `vX.Y.Z` (no plugin-name prefix). After a
release PR has been merged to `main`:

```bash
git fetch origin && git checkout main && git pull --ff-only
git tag -a vX.Y.Z -m "Release vX.Y.Z" <merge-commit-sha>
git push origin vX.Y.Z
```

Then attach the cross-compiled `dualsense-mapper.exe` + a sample
`dualsense-mapper.json` as the GitHub Release artefact (`gh release
create vX.Y.Z target/x86_64-pc-windows-gnu/release/dualsense-mapper.exe
rust/config.example.json --title "v0.1.0" --notes-from-tag`).

End users download the two files, drop them in a folder, run.

## Branch flow

- Feature branches: `feat/<topic>` for additions, `fix/<topic>` for
  bug fixes, `chore/<topic>` for non-code chores.
- Release branches: `release/<vX.Y.Z>` for the PR that bumps Cargo.toml
  + CHANGELOG. Title `release(vX.Y.Z): ...`, PR body summarising what
  changed since the prior tag.
- Never commit directly to `main`. Tags only go on `main` merge commits.
- Force-pushing `main` is forbidden. Force-pushing feature branches is
  allowed if the branch is yours.

## No `--admin` override

CI failures (`cargo test`, `cargo clippy` if added later, any future
lint) **cannot** be bypassed with `gh pr merge --admin` or
`--no-verify`. Fix the source, then re-merge.

If a test really is wrong, amend the test in the same PR with a
written justification in the PR description. The fact that a lint
job looks advisory is exactly how the paperwork v2.11→v2.13 incident
shipped three broken releases under admin override — don't repeat it.

## What will be rejected

- Edits to `legacy-python/` (unless explicitly to fix a security
  vulnerability; ask first).
- Synthesized-input code that bypasses `safety.rs` refcount.
- Macro / config additions that allow constant (non-jittered)
  timing.
- Features that lock the binary to one OS without a clear story
  for the other (Phase 2 macOS is the explicit target — don't add
  Windows-only behaviour without a `#[cfg(target_os = "macos")]`
  counterpart or a deliberate "Phase 2 TBD" CHANGELOG note).
- Tracking `docs/superpowers/` content.
