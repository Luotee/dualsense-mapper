# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.2.5] - 2026-06-03

### Fixed

- **Horizontal scroll / two-finger swipe shook the whole window.** The
  frontend never declared `overscroll-behavior`, so it defaulted to
  `auto`: a sideways trackpad / two-finger swipe over the
  vertically-scrolling pane rubber-banded the entire WebView2 surface
  (and on some platforms armed a history back-navigation). The window
  is a fixed `100vh` shell that should never scroll or bounce. Added
  `html, body { overscroll-behavior: none }` in `rust/web/style.css` to
  contain both axes. No content ever overflowed horizontally — this is
  purely overscroll containment, not a layout change.

## [2.2.4] - 2026-05-18

### Fixed

- **In-app GitHub links and contributor docs pointed to old repo name.**
  Repository was renamed `dualsense-mac-mapper` → `dualsense-mapper` on
  GitHub. While GitHub auto-redirects the old URL, the new name is now
  canonical. Updated:
  - `rust/web/settings.js` "About" pane — GitHub releases link in the
    in-app Settings → About section.
  - `rust/README.md` Quick-start — release download link.
  - `CLAUDE.md` — contributor-guidelines title.
  - `CHANGELOG.md` — all 16 release-tag URL anchors.
  Legacy reference `legacy-python/dualsense-mac-mapper.py` (real frozen
  filename) intentionally untouched.

## [2.2.3] - 2026-05-18

### Changed

- **Application icon refresh.** Replaced `rust/icons/icon.ico` with a
  Gruvbox-themed coloured DualSense rendering ("v6 Inverted":
  cream `#ebdbb2` body, dark `#282828` outline, `#d5c4a1` touchpad
  backing, face buttons in the four gruvbox accent hues — green
  `#b8bb26` / red `#fb4934` / blue `#83a598` / orange `#fe8019` —
  and a `#fabd2f` yellow PS LED). Generated from
  `scripts/icon_variants.py`, which flood-fills a user-supplied
  line drawing (kept local under `dist/` per `.gitignore`) and
  packs the result at 16/32/48/256. Tray icons
  (`tray-connected.ico`, `tray-disconnected.ico`) are
  unchanged — they convey connection state and intentionally
  stay abstract.

## [2.2.2] - 2026-05-18

### Fixed

- **Touchpad fill no longer flattens the trapezoid into a rect.**
  v2.2.1 rendered the touchpad outer as a traced path, but the 4
  quadrant hit zones (default-bound to mouse left, so always tinted)
  used the bbox-based `mkTouchpadQuad` factory and overflowed the
  trapezoid outline. Visually the touchpad collapsed back to a flat
  rounded rectangle whenever any quad had a binding. Each quadrant
  now renders as a real sub-path: the trapezoid ∩ that quadrant
  region (minus a 1.5u cross-gap at the centre), computed via
  shapely. New constant `TOUCHPAD_QUAD_PATHS` in
  `controller_geom.generated.js`.
- **Touchpad perimeter no longer wobbles.** The Q-curve midpoint
  smoothing in v2.2.1 reconstructed the outline from a sparse 79-pt
  polygon — fine for a single closed shape, but shapely's quadrant
  intersection introduced new vertices on the cross-gap axes, and
  re-smoothing those produced a low-frequency wave along the
  trapezoid edge that read as "jagged". The touchpad outer, light
  bar inner, and 4 quad sub-paths now emit dense polylines (~600
  vertices) straight from the raw cv2 contour, with no
  `approxPolyDP` and no Q-curve smoothing. Browser antialiasing
  handles the visual smoothness; the four shapes now align
  pixel-for-pixel at every shared edge.
- **Keyboard mirror animates for generic `Alt` / `Shift` /
  `Control` bindings.** `resolveKeyName` always reports the side
  the physical press came from (`LAlt`, never plain `Alt`), so a
  config entry with the generic modifier name silently produced no
  animation even though the synth itself worked. The mirror's
  `bindingsByKey` now expands generic modifiers to both side-
  specific lookup keys and stores all keys lowercase, so a hand-
  edited config with `Alt` / `lalt` / `LALT` matches a physical
  `LAlt` press.

### Removed

- `mkTouchpadQuad` factory in `controller.js` — the 4 quadrants
  are now plain `<path d="...">` reading from `TOUCHPAD_QUAD_PATHS`,
  so the per-corner rect-with-one-rounded-corner construction is
  gone.

## [2.2.1] - 2026-05-18

### Fixed

- **Touchpad outline now matches the line drawing.** v2.2.0 rendered
  the touchpad outer + light bar inner as flat `<rect>` bounding
  boxes, which collapsed the trapezoidal light-bar shape from the
  source drawing into a uniform rounded rectangle. Both now render
  as traced Q-curve paths — the touchpad reads as the same wider-
  on-top, narrower-on-bottom shape the rest of the controller was
  retraced from. New constants `TOUCHPAD_OUTER_PATH` and
  `LIGHT_BAR_INNER_PATH` in `controller_geom.generated.js`. The
  bbox-form `TOUCHPAD` constant is retained for raw-coord → pixel
  cursor mapping; only the visible decoration changes.

## [2.2.0] - 2026-05-17

### Changed

- **Controller silhouette redesigned from user line drawing.** The
  body outline, L1/R1/L2/R2 caps, touchpad, D-pad arms, face buttons,
  stick wells, stick outer-ring spokes, PS button, and Share/Options
  buttons are now traced from a user-supplied PS5 reference drawing
  (`scripts/trace_all.py` + `scripts/trace_share_options.py` →
  `controller_geom.generated.js`). All 25 hit-zone IDs and the
  binding semantics are unchanged.
  - **L2/R2** are the same rect shape as L1/R1, stacked above with
    a 1.5u vertical gap. Distinct from L1/R1, no longer rendered as
    a "trigger horn" decoration plus separate hit zone.
  - **D-pad arms** are 4 traced pentagon paths, one per direction —
    replaces the parametric `mkArrow` pentagon that approximated
    the shape.
  - **Stick outer-ring spokes** are 8 traced arc segments (4 per
    stick), pre-rotated 45° around the stick centre so each spoke
    aligns to one cardinal direction (matching wedge IDs 15-22 /
    19-22).
  - **Share / Options** are slanted-pill paths. Share is traced;
    Options is generated by mirroring Share across `x=120` so the
    two pills are guaranteed identical in size and angle.
- **Layout widened.** Right-side mapping table grew from 320px to
  460px (more room before keyboard names get ellipsis-truncated),
  controller diagram cap raised from 480px to 600px to absorb the
  extra space on the left. Single-column breakpoint moved from
  880px to 1020px to keep the two-column view at typical window
  sizes.
- **Mouse binding colour matches Key.** Mouse chip + wedge stroke
  now use `--soft` background and `--accent` foreground, same as
  Key. The chip list reads as one consistent "input" colour rather
  than splitting Key vs Mouse visually. Macro keeps its own accent.

### Removed

- Dead JS factories: `mkArrow`, `mkQuarter`,
  `buildRoundedPolygonPath`, `buildRoundedQuarterPath`,
  `insetAlong`, `formatPt`, plus the `CORNER_RADIUS`,
  `TRIGGER_HORN_L_PATH`, `TRIGGER_HORN_R_PATH`, and
  `LIGHT_BAR_PATH` constants. `controller.js` shrank from 827 lines
  to 522 lines. Hit zones now use the new `rect` / `path` kinds in
  the renderer switch instead of per-shape factory calls.

### Internal

- New trace pipeline scripts under `scripts/`:
  `trace_all.py`, `trace_share_options.py`,
  `build_silhouette_preview.py`, `gen_controller_constants.py`.
  Each is idempotent — running `gen_controller_constants.py`
  regenerates `rust/web/controller_geom.generated.js` from the
  intermediate JSON outputs of the two trace scripts.
- CLAUDE.md gained a "Cross-compile to Windows" rewrite documenting
  the canonical GUI ship target (`cargo xwin build --release
  --target x86_64-pc-windows-msvc --features gui` with
  `WEBVIEW2_STATIC=true`) and the Downloads test-drop convention.
  Existing CLI build remains documented for engine sanity-check only.

## [2.1.0] - 2026-05-17

### Added

- **Touchpad as laptop-style touchpad.** One finger on the
  DualSense touchpad drives the OS cursor (relative motion).
  Cursor is on by default; toggle and tune sensitivity in
  Settings → Touchpad (range `[0.1, 10.0]`, default `1.5`).
  Sub-2-pixel motion is filtered so a resting finger does not
  synthesise drift.
- **Touchpad press → 4 quadrant bindings.** The whole-pad click
  emits `ButtonDown(25..=28)` based on which quadrant
  (TL=25 / TR=26 / BL=27 / BR=28, split at x=960 / y=540) the
  finger was in at click-down. A drag across quadrant boundaries
  while the click is held keeps the original quadrant — matches
  laptop touchpad behaviour, lets users drag-select with mouse
  left held throughout. Default binding for all four: mouse
  left click.
- **`Binding::Mouse(MouseButton)`** new binding type. JSON form:
  `{ "type": "mouse", "value": "<kebab-case>" }`. Supported
  values: `left`, `middle`, `right`, `wheel-up`, `wheel-down`.
  Wheel values are one-shot scrolls (press fires the scroll;
  release is a no-op).
- **GUI** controller diagram splits the touchpad rect into four
  bindable quadrants; the bind popup gains a "Mouse" segment with
  the 5-option picker; the chip list extends to ids `0..=28`.
- **Settings tab** adds a Touchpad section (cursor toggle +
  sensitivity number input). Saving pushes the values straight
  to the HID worker's atomic cursor params, so changes take
  effect on the next decoded frame — no restart.

### Changed

- **`VALID_BUTTON_IDS` extends to `0..=28`.** Ids 25..=28 are
  the four touchpad quadrants. v2.0 configs (which never had
  these ids) auto-migrate on load — `Config::fill_touchpad_defaults`
  inserts the missing ids as `Unbound`, and `ConfigDoc::load`
  patches the raw JSON view so the next `write_atomic` emits
  the new ids. Existing user configs continue to load and
  validate without manual edits.
- **Default `dualsense-mapper.json`** ships with quadrants
  25..=28 bound to `Mouse(Left)` and the two new top-level
  fields (`touchpad_cursor_enabled`, `touchpad_cursor_sensitivity`).

### Iron rules

- **Rule #1** updated: `Config::validate` now requires all ids
  `0..=28`, not `0..=24`. Loaders auto-fill the new ids before
  validation so MINOR semantics are preserved.
- **Rule #3** amended: `release_all_held` and the panic hook
  now release every held key **and** every held mouse button.
  `KeyState` tracks both refcount tables;
  `emergency_release_all` walks both and synthesises
  `Direction::Release` through enigo for each.

### Fixed (post-fix4 polish — 8 user-reported issues)

- **Visual feedback semantics.** Physical gamepad press lights only
  the mapped button; keyboard press lights all bound buttons.
  `keyboard_mirror.js` now uses defer-and-check 30 ms: on keydown,
  the mirror flash is deferred 30 ms and skipped if any `button-down`
  IPC arrives in that window (engine synth detected). Replaces
  fix4's racy 250 ms forward suppression that was missing race
  cases where the synth keydown beat the IPC by < 15 ms.
- **Touchpad cursor drift on click.** Physical press no longer
  shifts the cursor. The HID worker now suppresses cursor deltas
  for the full duration a touchpad button is held (Issue 8
  "click freeze" layer 1). Mirrors Synaptics PalmCheck-Enhanced /
  libinput thumb-detection on Clickpads.

### Added (post-fix4)

- **Touchpad continuous hover preview.** Per-frame quadrant emit
  with dedupe-on-change. New `GamepadEvent::TouchpadHover {
  raw_x, raw_y, quadrant }` (`quadrant=255` sentinel = "finger
  lifted"). Frontend live-highlights the active quadrant + draws
  a persistent debug dot at the raw finger position so users can
  empirically calibrate `touchpad_midpoint_x/y`.
- **Touchpad cursor acceleration curve.** libinput-style: slow
  region (raw |Δ| < 5 px/frame) uses gain × 0.5 for precision,
  fast (> 20 px/frame) uses × 1.5 for flick, linear interp
  between. Replaces the v2.1.0-fix4 raw `× sensitivity`
  mapping. Tunable per-axis in Settings.
- **Touchpad stationary deadzone.** Rolling 3-frame magnitude
  window. Sub-radius motion suppressed (anti-jitter). Default
  radius 2 raw px; range 0..=50 in Settings.
- **Disconnect button** in the toolbar. Sends a manual
  `disconnect_gamepad` IPC; the HID worker drops the current
  device handle and returns to Searching state (thread stays
  alive). Press any controller button to reconnect.
- **Rounded outer corners** on D-pad pentagons and stick donut
  quarters. Subtle (~0.7–0.8 px inset), matching the L1
  button's `rx="2"` corner feel. Tunable via
  `tools/controller_tuner.html` → "Corner radius" group.
- **Key binding chip list** uses 2-column CSS grid layout.
  29 rows fit the viewport without scroll.
- **New Settings fields** for cursor filter: `Click freeze`,
  `Accel slow/fast threshold`, `Accel slow/fast gain`,
  `Stationary deadzone`. Live-pushed to HID worker atomics
  via `CursorParams`.

### Config (post-fix4)

- 6 new top-level optional fields (auto-filled on load via
  `#[serde(default)]` for v2.0.0 / v2.1.0-fix4 configs):
  - `touchpad_click_freeze_enabled` (bool, default `true`)
  - `touchpad_accel_slow_threshold` (u32 raw px/frame, default `5`)
  - `touchpad_accel_fast_threshold` (u32 raw px/frame, default `20`)
  - `touchpad_accel_gain_slow` (f32, default `0.5`)
  - `touchpad_accel_gain_fast` (f32, default `1.5`)
  - `touchpad_deadzone_radius` (u32 raw px, default `2`)

## [2.0.0] - 2026-05-17

### Breaking changes

- **gilrs removed.** Non-DualSense pads (Xbox, 8BitDo, generic
  XInput) are no longer supported. Users on those pads should
  stay on v1.2.0. The on-disk `dualsense-mapper.json` schema is
  unchanged — config files migrate as-is.

### Changed

- **Controller source is now raw HID via `hidapi-rs`.** A worker
  thread opens the DualSense BT device (`054c:0ce6`), sends a
  feature `0x05` calibration read to unlock 0x31 mode, and
  blocking-reads 78-byte 0x31 reports. Each frame is decoded to a
  `DsState`, diffed against the previous snapshot, and the deltas
  pushed to the engine as `GamepadEvent`s through the same
  channel pattern the v1.x fake source used.
- **Connection state is ground truth.** The
  `Searching → Handshaking → Streaming` state machine emits
  `Connected` on the first decoded 0x31 frame and `Disconnected`
  on either read-error or 50 consecutive 4 ms read-timeouts
  (~200 ms). Pad-on → status flips within ~500 ms; PS-hold off →
  status flips within ~250 ms. No more 24-second "press any
  button to confirm" wait, no more stuck "Connected" after a
  clean power-off.
- **Single exe size**: 11.3 MB → 10.8 MB (gilrs + ~30 transitive
  deps removed).

### Deferred

- DualSense USB transport (v2.0.1).
- DualSense Edge (`054c:0df2`) (v2.0.1).
- Touchpad-as-mouse binding type (v2.1).
- IMU axes for bindings, haptic feedback, adaptive triggers
  (v2.2 +).

[2.0.0]: https://github.com/Luotee/dualsense-mapper/releases/tag/v2.0.0

## [1.2.0] - 2026-05-17

### Fixed

- **Macro tab actions appeared to do nothing.** `+ New`, `+ Step`,
  per-row key capture and right-click Delete all relied on the
  filesystem watcher to emit `config-changed` after a write. On
  Windows, `notify-rs` loses the watch handle across
  `write_atomic`'s temp + rename, so the second write onwards was
  silent and the UI never refreshed. Every IPC mutator
  (`set_binding`, `set_macro`, `delete_macro`, `rename_macro`,
  `set_settings`, `reset_settings`) now emits `config-changed`
  itself on success.
- **Settings → About read `v0.2.0`** while the binary was at
  v1.1.4. New `get_app_version` IPC returns
  `env!("CARGO_PKG_VERSION")`; the About box reads it on init.
- **Controller status no longer shows "Connected" for a paired
  but powered-off pad.** gilrs on Windows enumerates the OS
  pairing list and reports cached `is_connected()` as `true`
  for pads that are not actually transmitting; v1.1.x trusted
  that signal blindly. The Connected status now requires at
  least one real input event (button / stick / trigger) — a
  paired-but-silent pad keeps the GUI in "Waiting".
- **Settings → About links now open in the system browser.**
  Tauri CSP `default-src 'self'` was blocking `<a target="_blank">`
  silently. New `open_url` IPC command routes anchor clicks
  through `cmd /c start` on Windows.
- **Single-file Windows binary.** Switched cross-compile target
  from `x86_64-pc-windows-gnu` to `x86_64-pc-windows-msvc` via
  `cargo-xwin` + `WEBVIEW2_STATIC=true`. WebView2Loader is now
  statically linked — no more `WebView2Loader.dll` next to the
  exe. Ship is `dualsense-mapper.exe` (~11 MB) + a sample
  `dualsense-mapper.json`.

### Added

- **Keyboard → button focus mirror.** Pressing a key bound to a
  controller button while the GUI window is focused triggers the
  same yellow flash on the matching button. Releasing the key
  clears it. Suppressed while a bind popup / macro step capture
  is active, and while focus is inside form fields.

### Known limitations (planned for v1.3)

- **Real-time connection state for BT pads is best-effort.**
  gilrs `EventType::Disconnected` does not fire on Windows when
  a DualSense is powered off via PS-button-hold; `is_connected()`
  stays cached as `true`. The user-facing impact: after a clean
  pad shutdown the status indicator may remain stuck on
  "Connected" until something else triggers a re-evaluation.
  Workaround for now: press any key on the pad to confirm. v1.3
  will replace the gilrs path for DualSense pads with a raw HID
  reader (`hidapi-rs`) so connection state, touchpad, IMU and
  battery are read directly from the 78-byte HID report.

### Iron rules

- 10. Any IPC command that mutates `config` must emit
  `config-changed` itself on success; the filesystem watcher is
  the second source of truth, not the only one.
- 11. Controller connection state must require evidence of real
  input — gilrs's `EventType::Connected` / `is_connected()` lie
  on Windows for paired-but-silent BT pads. The current
  `GamepadSource::poll` only emits Connected once it has seen
  a Button or Axis event for the session; the periodic
  `is_connected()` re-scan exists only as the post-armed
  disconnect detector, and is known to be incomplete (see
  Known limitations).

[1.2.0]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.2.0

## [1.1.4] - 2026-05-17

### Fixed

- **Stick donut quarters `down` and `left` render correctly.** v1.1.0–
  v1.1.3 used reflection maps `(x, -y)` and `(y, x)` to transform the
  canonical UP quarter into DOWN and LEFT. Reflections invert
  orientation, which flipped the SVG arc sweep direction — the arcs
  for those two directions traversed the wrong way around the stick
  centre and produced visibly broken shapes. `mkQuarter` now uses
  proper rotations (`(-x, -y)` 180° for DOWN and `(y, -x)` 90° CCW
  for LEFT), matching the fix that landed in `mkArrow` in v1.1.2.

[1.1.4]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.1.4

## [1.1.3] - 2026-05-17

### Changed

- **D-pad pentagon + stick donut quarter retuned via the new
  interactive tuner.** v1.1.2 had the d-pad pentagons too sparse and
  the stick donut rings too thick / too gapped. The user dialed both
  in against the 2 SVG-unit L1↔L2 spacing as a visual reference using
  `tools/controller_tuner.html`. New values:
  - D-pad pentagon: `R_inner=2.70`, `half_w=3.40`, `R_outer=13.20`
    (gap = 3.82, length×width = 10.5×6.8, ratio 1.54).
  - Stick donut quarter: `r_in=11.60`, `r_out=15.10`, `d=1.70`
    (thickness = 3.5, gap = 2.40).

### Added

- **`tools/controller_tuner.html`** — self-contained interactive
  geometry tuner. Sliders for every hit-zone and sprite parameter in
  `rust/web/controller.js`; live SVG re-render on every input event;
  Export button generates a paste-ready snippet of the tuned values.
  Bookmark this file for future visual tweaks — it's tracked in the
  repo as a permanent design tool.

[1.1.3]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.1.3

## [1.1.2] - 2026-05-17

### Fixed

- **D-pad pentagon apex angles are now strictly 45° and gaps are
  uniform.** v1.1.1 picked R_shoulder and half_w independently so the
  apex sides came out at arbitrary slopes and the gap between
  adjacent pentagons varied. The parametrisation now enforces
  `R_shoulder = R_inner + half_w`, which makes the apex side of
  every pentagon lie on a line of slope −1 (or its 90°-rotated
  equivalent), so adjacent pentagons' apex sides are parallel and
  the gap between every pair is uniformly `R_inner * √2`.
- **Stick wedges are trapezoids with 45° sides instead of arcs.**
  The arc-based quarters from v1.1.1 produced visually non-parallel
  boundaries between adjacent quarters (each boundary was a radius,
  which converged to the centre). They now share the same
  parallel-gap geometry as the d-pad pentagons: flat outer base,
  flat inner base, two 45° side edges. Adjacent trapezoids' diagonal
  sides lie on parallel lines so the gap is uniform along the
  whole shared boundary.
- **Unbound wedges show a dashed outline.** v1.1.1 hid the unbound
  hit zones entirely (`stroke: none`); on the four-stick-directions
  case where nothing is bound the user couldn't see there were
  buttons at all. Unbound wedges now carry a thin dashed `--muted`
  stroke so the hit zone is discoverable while still reading as
  "not bound" (no fill).

[1.1.2]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.1.2

## [1.1.1] - 2026-05-17

### Fixed

- **Status no longer shows "Connected" before any pad is plugged in.**
  v1.1.0 enumerated every gilrs gamepad entry on first poll and
  unconditionally emitted `Connected` for each — but some Windows
  drivers leave stale phantom entries in `gilrs.gamepads()` from
  previous sessions. The startup scan now filters on
  `Gamepad::is_connected()` so only real, physically-attached pads
  trigger the connected status.
- **Stick wedge outlines.** A stick with all four directions bound to
  the same colour used to render as a single ring of solid colour in
  v1.1.0 — `.wedge { stroke: none; fill-opacity: 0.4 }` left no
  visual separator between adjacent quarters. Each wedge now carries
  a thin stroke in its binding colour (`--accent` for key,
  `--macro` for macro) and the arc span shrinks from 90° to 84° so
  there's a small gap between adjacent quarters. Four bound
  directions read as four separate buttons.

### Changed

- **D-pad: four label pentagons, apex inward.** v1.1.0's outward-
  arrow pentagons + underlying cross sprite read as one combined
  glyph. The new design drops the cross sprite entirely and uses
  inward-apex label shapes (flat outer base, tapered tip pointing
  toward the d-pad centre), sized closer to the face-button cluster.
  Reads as four independent targets and the press-ring animation
  flashes one label outline per direction.
- **L2 / R2 / L1 / R1 evenly spaced.** v1.1.0 had a 6 px gap between
  the trigger row and the shoulder row but only 2 px between
  shoulders and the body. Triggers move down by 4 px (ry 10 → 14)
  so trigger-shoulder, shoulder-body, and body-top edges are
  uniformly 2 px apart.

[1.1.1]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.1.1

## [1.1.0] - 2026-05-17

### Changed

- **Palette: Gruvbox Dark.** `rust/web/solarized.css` is renamed to
  `palette.css` and every CSS variable is swapped to Gruvbox Dark
  hex values (bg `#282828`, card `#3c3836`, accent `#83a598`, macro
  `#fe8019`, success `#b8bb26`, …). All UI surfaces — toolbar,
  tabs, chip rows, bind popup, macro editor, settings, activity
  drawer, controller fill / hit zones — pick the new colours up
  automatically through the existing `var(--…)` references. ICO
  assets are regenerated from the same source palette so the app
  icon and tray icons stay in lockstep.
- **D-pad hit zones are pentagon arrows.** Each direction (Up /
  Down / Left / Right) now has its own outward-pointing pentagon
  outline sized to its arm of the cross sprite, instead of the
  v1.0.x shared triangle wedge. The press-ring animation follows
  the same arrow silhouette on physical press, so each direction
  flashes its own shape — matching the face-button per-direction
  behaviour the user expected.
- **Stick hit zones are donut quarters.** Each of the 4 virtual
  stick directions (Up / Down / Left / Right) is now a quarter
  arc of an annulus around the stick well, instead of a triangle
  pointing into the centre. The L3 / R3 inner circle (id 7 / 8)
  stays concentric on top, so each stick has five distinct,
  individually-flashing hit zones (4 quarters + 1 centre).
- **L2 / R2 match L1 / R1 size.** Triggers used to be 26×11 over
  the body's top edge — visibly chunkier than the 22×6 shoulders.
  Both pairs are now 22×6 with L2 / R2 sitting six pixels above
  L1 / R1, producing a balanced top edge.

### Build

- `scripts/build_icons.py` palette constants point at the new
  Gruvbox values; existing `python3 scripts/build_icons.py` flow
  is unchanged.
- New `scripts/palette_mockup.py` renders the GUI under several
  palettes side-by-side; used during brainstorming before this
  release picked Gruvbox Dark.

[1.1.0]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.1.0

## [1.0.6] - 2026-05-17

### Added

- **Bind any OEM punctuation key** (`-` `=` `[` `]` `\` `;` `'` `,`
  `.` `/` backtick). The frontend capture box used to reject anything
  outside letters / digits / named keys; the backend already
  routed punctuation through `Key::Unicode`, which on Windows
  inserts a character but does not register as a held key. The
  punctuation chars now resolve to the matching `VK_OEM_*` virtual
  key on Windows (US layout) so games hooked at the virtual-key
  level see them as real held keys.
- **Left / right modifier names**: `LShift`, `RShift`, `LControl`
  (alias `LCtrl`), `RControl` (alias `RCtrl`), `LAlt`, `RAlt`. Bind
  them by pressing the left or right modifier in the capture box;
  the frontend distinguishes via `KeyboardEvent.code`. Generic
  `Shift` / `Control` / `Alt` names still work for binders that
  don't care about the side.

### Build

- `_keyboard_keys` cheat-sheet in the bundled
  `dualsense-mapper.json` now lists the new `modifiers_lr` and
  `punctuation` sections so a user editing the file in Notepad
  sees the full set of valid names inline.
- Two new `parse_key` tests cover punctuation round-trip and the
  six L/R modifier aliases. Test count: 49 → 51.

[1.0.6]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.6

## [1.0.5] - 2026-05-17

### Changed

- **Icon assets fill the canvas.** v1.0.4's ICOs left big empty bands
  above and below the controller because the source SVG viewBox is
  240×130 (wide-aspect 1.85) and was being fitted by width into the
  square ICO. `scripts/build_icons.py` now tight-crops the rendered
  silhouette's alpha bounding box and rescales the result to fill
  ~94% of the target square, so the controller occupies the icon at
  every resolution — same trick common Windows app icons use.
- **Drop "Esc cancels" hint from the bind popup's key-capture box.**
  Esc didn't actually cancel in v1.0.2..v1.0.4 (the popup-root
  handler doesn't fire from inside the focused capture box on every
  WebView2 build), and the user has Unbound as an explicit cancel
  path anyway. The hint text was misleading — removed for now until
  Esc handling is repaired.

[1.0.5]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.5

## [1.0.4] - 2026-05-17

### Changed

- **Icon redesign — matches the in-app SVG controller.** All three
  ICO assets (`rust/icons/icon.ico`, `tray-connected.ico`,
  `tray-disconnected.ico`) are now generated from the same geometry
  as `rust/web/controller.js`: body silhouette, touchpad notch,
  d-pad cross, four face-button dots, two stick wells, and the PS /
  Share / Options markers cut out as negative space. L1 / R1 / L2 /
  R2 (the parts that protrude above the body) are dropped from the
  icon — they were noise at icon resolution. Each ICO carries
  hand-tuned 16 / 32 / 48 / 256 layers (16 is a pure silhouette;
  32 keeps stick wells + d-pad; 48 adds face buttons + touchpad;
  256 has the full detail set). Solarized palette: accent blue for
  the app icon, success green when a controller is connected,
  muted grey when disconnected.

### Build

- New `scripts/build_icons.py` generator that produces all three
  ICOs from the same parametrised silhouette. PIL only (no SVG
  renderer dependency); each ICO is assembled as a manual
  multi-resolution container so the smaller layers stay
  hand-tuned instead of resampled from the 256 master.

[1.0.4]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.4

## [1.0.3] - 2026-05-16

### Changed

- **Closing the window (✕) now fully exits the process.** v1.0.0–v1.0.2
  followed the original spec §10 design where ✕ hid the window and the
  mapper kept running in the tray; users reported the process
  lingering in Task Manager and hitting unexpected behaviour because
  Windows convention is "✕ closes the app." The window close handler
  now calls `app.exit(0)` directly — `engine.shutdown()` still runs on
  the way out, so held keys release cleanly (Iron rule #3). The tray's
  `Quit` entry is unchanged; it becomes a convenience duplicate of ✕
  rather than the only exit path. Users who want background mapping
  while the window is hidden should minimise to the taskbar instead.

[1.0.3]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.3

## [1.0.2] - 2026-05-16

### Fixed

- **GUI press-ring lights up for stick directions and analog triggers.**
  In v1.0.0 / v1.0.1, pushing the L-stick / R-stick past the deadzone
  (ids 15–22) and pulling L2 / R2 past the trigger threshold (ids
  23–24) correctly synthesised the bound keystroke, but the SVG hit
  zone on the controller never highlighted. Root cause: those virtual
  presses were happening inside `Mapper::transition_virtual`, while the
  Engine-to-GUI event bridge only forwarded real gilrs `ButtonPressed`
  events. The mapper now buffers each virtual flip and the engine
  drains it via `Mapper::take_visual_transitions`, re-emitting
  `EngineEvent::ButtonDown` / `ButtonUp` so the existing frontend
  press-ring path lights up exactly the same way it does for physical
  face buttons.
- **D-pad and stick wedges now tint with their binding colour.** The
  triangular hit zones that overlay the d-pad cross and stick wells
  carried a `hit-invisible` class so the sprite beneath stayed
  readable, but that hid the binding state entirely. They now carry
  the `binding-key` / `binding-macro` class with a `fill-opacity: 0.4`
  modifier so a bound direction tints visibly while still letting the
  cross / well sprite read through. Unbound wedges stay fully
  transparent — same as v1.0.0.
- **Bind popup key capture stays active after the first keystroke.**
  Previously each capture required clicking the capture box, pressing
  one key, then clicking again to change it. The capture box now
  auto-focuses when the Key segment opens, the listener stays attached,
  and each subsequent keypress overwrites the captured value in place.
  Escape still cancels via the popup-root handler.

### Changed

- **Default `config.example.json` ships a MapleStory-friendly profile.**
  Cross → `Alt`, Circle → `z`, Square → `Shift`, Triangle → `a`,
  R2 → `Shift`, PS → `Space`, Options → `Enter`, D-pad and L-stick
  → arrow keys, R-stick / L1 / R1 / L2 / L3 / R3 / Share unbound by
  default. The sample `macro_A` definition stays in the `macros`
  section so users can see the macro schema even though it isn't
  bound out of the box. `examples/maple_artale.json` is kept in sync
  with the default for parity.

[1.0.2]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.2

## [1.0.1] - 2026-05-16

Cosmetic patch on top of v1.0.0. No code or behaviour change.

### Fixed

- Default `dualsense-mapper.json` written on first run: drop the
  misleading "觸發巨集" suffix from the L2 (id 23) label. The
  suffix described the default *binding* (a macro) rather than the
  button itself, so anyone who later remapped L2 saw a stale label.
  Existing users with their own config are unaffected.

[1.0.1]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.1

## [1.0.0] - 2026-05-16

First GUI release. Version jumps from `0.1.x` straight to `1.0.0` to
mark the project as feature-complete for Phase 1 (Windows GUI mapper);
the underlying changeset is the same one previously tracked as `0.2.0`
during development. The exe now opens a real window with a controller
diagram, click-to-capture remap, step-list macro editor, Solarized Light
theme, and a tray-resident background mapper. The v0.1.x console flow
stays available via the new `--cli` flag.

### Added

- **Tauri 2.x GUI shell** (`rust/src/gui/`). Window opens within ~1 s of
  double-click; close-on-X hides to tray, Quit on the tray menu is the
  only way to exit the process.
- **System tray** with two icon states (connected / disconnected) and a
  3-item menu (Open / Pause mapper / Quit). Tray icon swaps green ↔ grey
  on controller connect / disconnect.
- **Mappings tab** with a full SVG DualSense diagram, all 25 hit zones
  (face buttons + D-pad + L1/R1 + L2/R2 + L3/R3 + stick virtual
  directions). Live highlight on physical press; click any button to
  open a bind popup with Key / Macro / Unbound segmented control.
- **Click-to-capture** key binding: in Key mode, the popup shows
  "Press the key to bind…", normalises the next `KeyboardEvent` via the
  spec §7.4 table, and writes it. No more typing key names by hand.
- **Macros tab** with a left-pane macro list (showing which buttons each
  macro is bound to) and a right-pane step-list editor. `+ Step`, `+
  Quick tap…` (sugar for down+up pair), drag-to-reorder, Loop toggle,
  inline `min < max` validation, Save / Discard. Rename / Duplicate /
  Delete via right-click; Delete blocked-with-confirmation when the
  macro is bound to any button.
- **Settings tab** with the 5 top-level config fields (deadzone,
  trigger_threshold, min_press_ms[min,max], tick_jitter_ms[min,max],
  log_events), a "Reset to defaults" button, and an "Open config file
  in editor" button (Notepad on Windows, `open -t` on macOS Phase 2).
- **Activity log drawer** (📊 toggle in the toolbar): live stream of
  gamepad events + synthesised key emits + macro lifecycle. Throttled
  to ≤1 paint per `requestAnimationFrame`, capped at 200 DOM rows.
  Drawer-open state persists across restarts in `dualsense-mapper.ui.json`.
- **Solarized Light theme** pinned to Ethan Schoonover's spec —
  10 CSS variables drive every UI element, no invented colours.
- **File watcher** (`notify` crate) for external edits to the config:
  user opens `dualsense-mapper.json` in Notepad, saves, and the live
  engine hot-rebinds within 250 ms. Validation failure surfaces the
  Rust error verbatim instead of silently dropping the change.
- **`Engine` + `Handle`** abstraction (`rust/src/engine.rs`): the v0.1.x
  blocking mapper loop is now wrapped in a thread-safe handle that the
  GUI mutates while it runs. Atomic flags for pause + capture-active +
  shutdown; `RwLock<Config>` for hot rebinding; channel of
  `EngineEvent`s for the GUI bridge.
- **`ConfigDoc`** raw-JSON-preserving reader/writer
  (`rust/src/config_io.rs`): GUI writes through it so the `_help` and
  `_keyboard_keys` inline cheat sheet (and any other `_*` doc fields)
  round-trip byte-for-byte. Atomic write via `*.tmp` + rename.
- **Iron rule #9 (new)**: the GUI is a chrome layer. No mapping
  decision, no key synth, no macro scheduling lives in JavaScript.
  Every runtime-state mutation routes through `#[tauri::command]` in
  `rust/src/gui/commands.rs`. JS that calls `SendInput` is a rejection.
- **`--cli` flag** in `main.rs`: opt-in to the v0.1.x console mode.
  Useful for `--validate`, `--list-buttons`, headless dry-runs, and CI.

### Changed

- **Iron rule #8 reframed** for the GUI-first world: window must be
  visible within ~1 s of double-click; CLI mode is the explicit
  opt-in legacy path (rule still applies there). See `CLAUDE.md`.
- **Default mode is GUI, not CLI**. v0.1.x users who scripted the exe
  with no flags will need to add `--cli` to keep the previous
  behaviour. The 1-line migration is documented in `rust/README.md`.

### Fixed

- **Iron rule #3 panic hook actually works now.** v0.1.x installed a
  panic hook that captured a freshly-allocated `safety::shared()` Arc
  — not the live engine's Arc. On panic, the hook drained an empty
  map, leaving real held keys stuck at the OS level. Fixed by routing
  through a `OnceLock<SharedKeyState>` that the engine binds via
  `safety::register_global` after spawn. The hook now drains the
  actual engine state. Latent in v0.1.0 and v0.1.1.

### Build

- New cargo feature `gui` gates the Tauri dependency tree so
  `cargo test` and `cargo build` on a Linux dev host without
  webkit2gtk dev libs still work. Production builds use
  `cargo build --release --target x86_64-pc-windows-gnu --features gui`.

[1.0.0]: https://github.com/Luotee/dualsense-mapper/releases/tag/v1.0.0

## [0.1.1] - 2026-05-16

End-user double-click UX pass — Windows users open the exe by double
clicking, not from a terminal. v0.1.0 first-run flow exited with code 1
which closed the console window before they could read anything.

### Changed

- **First-run no longer exits.** When the bundled default
  `dualsense-mapper.json` is written next to the exe, the program keeps
  running with that default. The user can edit the file later and
  restart to customize. (Previous behaviour: write default + exit code
  1, which made the console window vanish for double-click users.)
- **Errors pause for "Press Enter to close".** Any uncaught error from
  `main` prints the chain, then waits on stdin, so the console window
  stays visible long enough to read what went wrong. `--no-pause` flag
  added for CLI / CI users who want immediate exit.
- **Startup banner.** On normal start the exe prints program name,
  version, config path, and "Press Ctrl-C or close window to quit".
  First-run users also get a "Wrote default config — edit it in
  Notepad" note.
- First-run-written `dualsense-mapper.json` now embeds an inline
  keyboard cheat sheet (`_help` + `_keyboard_keys` fields) so end users
  discover valid key names directly in the file they are editing,
  without having to consult the README. Both fields start with `_` and
  are silently ignored by the config loader (serde drops unknown keys),
  so the file validates and round-trips normally.

### Added

- `--no-pause` CLI flag.

### Removed

- GitHub Release no longer bundles a separate `config.example.json`.
  The exe writes the same content on first run, so shipping both was
  redundant.

## [0.1.0] - 2026-05-16

First Rust rewrite ship. Single-binary Windows `.exe` portable bundle.
Legacy Python (`legacy-python/`) remains in repo as frozen reference.

### Added

- Rust crate at `rust/` producing a single `dualsense-mapper(.exe)` binary.
- JSON config schema (`config.example.json` shipped alongside binary):
  - Every button id `0..=24` must be present; `"type": "unbound"` for unused.
  - Bindings: `key` (single key by name), `macro` (named macro), `unbound`.
  - Macros: ordered steps with `[min, max]` random delays.
- Three discoverability layers for button ids:
  1. Exhaustive `config.example.json` listing all 25 ids with labels.
  2. README cheat-sheet table.
  3. `--list-buttons` CLI live readout (authoritative for the current OS / driver).
- CLI flags: `--config PATH`, `--validate`, `--dry-run`, `--list-buttons`, `--verbose`.
- Default config path: **next to the executable** as `dualsense-mapper.json`.
  Portable — copy the folder to a USB stick, runs anywhere.
- Stuck-key prevention (four layers):
  - Refcounted key state in `safety.rs`.
  - `Drop` on `KeyboardSink` releases everything held.
  - Panic hook releases keys before unwind exits the process.
  - Ctrl-C handler drains the loop cleanly.
- Anti-cheat self-discipline (carries the Python POC's intent forward):
  - `min_press_ms` floor enforces a randomized minimum KEYDOWN→KEYUP gap on
    every synthesized press, so transient bot-shaped patterns get smoothed.
  - `tick_jitter_ms` adds ±jitter when multiple keys fire on the same tick.
  - Macro step delays are always `[min, max]` ranges; constant delays are
    rejected by the config validator.
- Macro engine on dedicated `std::thread`s with cancellable `AtomicBool` flag.
  Cancellation or natural exit **drains every unmatched Press** as a Release
  before the thread returns, so mid-macro release of the source button can
  never strand a KEYDOWN at the OS level.
- D-pad hat-axis handling — gilrs reports many controllers' D-pad as
  `Axis::DPadX` / `Axis::DPadY` (-1 / 0 / +1) instead of discrete
  `DPadLeft/Right/Up/Down` buttons. `gamepad.rs` watches both paths and
  synthesises `ButtonDown/Up(11..=14)` from axis crossings so the mapper
  sees a single, uniform event surface.
- Trigger normalization detects both `[-1, 1]` (Linux gilrs) and `[0, 1]`
  (Windows XInput) conventions by sign, so idle-trigger value never sits
  on the activation threshold.
- Verbose pipeline tracing — every gilrs event, mapper decision, and
  enigo emit logs at `info` level; debug fills in dropped events and
  parse details. `--verbose 2> log.txt` is the canonical bug-report dump.
- 33 unit + integration tests covering schema, validation, mapper,
  refcount, macro drain on cancel, shipped configs.

### Cross-platform notes

- Build target: `x86_64-pc-windows-gnu` from a Linux dev host requires
  `mingw-w64`. Linux test/dev build requires `pkg-config + libudev-dev`
  (gilrs's Linux backend).
- macOS support is Phase 2 (uses `enigo` 0.6's `CGEvent` backend, no
  code change expected in `keyboard.rs`).

[0.1.1]: https://github.com/Luotee/dualsense-mapper/releases/tag/v0.1.1
[0.1.0]: https://github.com/Luotee/dualsense-mapper/releases/tag/v0.1.0
