# PadToKBM

**English** | [繁體中文](README.zh-TW.md)

PadToKBM (formerly DualSense Mapper) maps a gamepad (DualSense, Switch Pro or Switch 2 Pro) to keyboard and mouse for use on a laptop. Originally written in Python in May 2025 to let my wife play MapleStory Worlds Artale comfortably on a MacBook; rewritten in Rust to ship as a single Windows executable.

<p align="center">
  <img src="docs/images/readme/board.png" width="820"
       alt="The key-binding board: the controller in the middle, every button's slot around it joined by a red line, a keyboard, a mouse and macro tiles below">
  <br>
  <sub>One screen: every button of the selected controller and what it does. Drag a key, mouse button or macro onto a slot to bind it. The pressed button (here Cross) and the key it sends light up.</sub>
</p>

## What you get

- **Bind by dragging, not by editing JSON.** Drag a key, mouse button or macro onto a button's slot; drag a slot onto another to swap, out to clear. Or click a slot and press the key you want. 復原／重做 (Ctrl+Z / Ctrl+Y) step back through your changes.
- **Each controller has its own bindings** — DualSense, Switch Pro and Switch 2 Pro each keep their own table.
- **Quick-slot pages.** Hold ZL / L2 (or ZR / R2) and the D-pad and face buttons switch to a second set of 8 keys; or set the trigger to act as one ordinary key.
- **29 mappable inputs** — face buttons, D-pad, both sticks as 4-way digital, L1 / R1, L2 / R2 analog triggers, L3 / R3, Share / Options / PS, plus the four touchpad quadrants.
- **The touchpad drives the mouse cursor** with a two-stage acceleration curve, and each quadrant is its own click binding.
- **Macros with randomized delays.** Every step's delay is a `[min, max]` range, never a constant tick — a looping macro doesn't fingerprint as a script.
- **No stuck keys.** Every synthesized press goes through one refcounted safety layer; `Drop` handlers plus a panic hook release everything still held even if the process dies.
- **Turn the controller off from the app** (Windows) — a Bluetooth link-level disconnect, so the pairing survives and pressing PS reconnects, exactly like a PS5.
- **One ~11 MB `.exe`.** No installer, no DLLs, no driver, no process hooking — user-mode `SendInput` only. Config is a single JSON file written next to the exe on first run.

## A look around

<table>
  <tr>
    <td width="50%"><img src="docs/images/readme/quickslots.png" alt="Switch 2 Pro board with ZL held: its quick page slot F3 and the F3 key light up"></td>
    <td width="50%"><img src="docs/images/readme/capture.png" alt="A slot waiting for a key, with the bar 按鍵鼠上的鍵，或點下面的方塊（Esc 取消）"></td>
  </tr>
  <tr>
    <td><sub><b>Quick-slot pages.</b> While ZL is held, the D-pad and face buttons use the ZL page (here ← sends F3). Each trigger switches between 快捷頁 (page) and 單鍵 (one key).</sub></td>
    <td><sub><b>Press to bind.</b> Click a slot, then press a key or a mouse button (middle, right, wheel) — or click a tile below. Esc cancels.</sub></td>
  </tr>
  <tr>
    <td><img src="docs/images/readme/macro-editor.png" alt="Macro editor: keycap steps (pressed / released) with a min–max delay under each"></td>
    <td><img src="docs/images/readme/settings.png" alt="Settings window, 一般 tab: config file path, open buttons, event log switch and the last six events"></td>
  </tr>
  <tr>
    <td><sub><b>Macros.</b> Click keys on the keyboard to add steps, drag to reorder, drag out to delete. Every delay is a min–max range, so a looping macro never repeats the exact same timing.</sub></td>
    <td><sub><b>Settings</b> (gear). Config file and the latest events, sticks, touchpad, and the randomized key timing — four tabs, saved with 儲存.</sub></td>
  </tr>
</table>

<sub>Screenshots use a demo config. Regenerate them with <code>scripts/readme-shots.mjs</code>.</sub>

## Get it

Download `PadToKBM.exe` from the [latest release](https://github.com/Luotee/pad-to-kbm/releases), pair the pad over Bluetooth, then double-click the exe. Full walkthrough and button reference: [`rust/README.md`](rust/README.md).

## Implementations

| Folder | Status | Audience |
|---|---|---|
| `legacy-python/` | Functional, frozen for reference | Developers comfortable with `pip install` |
| `rust/` | Phase 1 (Windows), Phase 2 (macOS) in progress | End users — single `.exe` |

The Rust build is the recommended path. Python is kept for blame history and because it remains usable on macOS until Phase 2 lands.

## Supported hardware

- **DualSense PS5 controller** (`054c:0ce6`) over **Bluetooth**.
- **Nintendo Switch Pro Controller** (`057e:2009`) over **Bluetooth** — pair it in Windows Bluetooth settings first.
- **Nintendo Switch 2 Pro Controller** over **Bluetooth LE** — the first time, hold the small SYNC button; afterwards any button reconnects. Do **not** pair it in Windows Bluetooth settings (it disconnects). In no-mapping mode (the default for this pad) it needs [ViGEmBus 1.22.0](https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0) and shows up to games as an Xbox controller. Windows 11 is recommended (Windows 10 limits it to ~17 Hz). The Switch 2 Pro defaults to no-mapping mode, so key bindings (including GL/GR/C, ids 30/31/32) apply only after you set `mapping_enabled.ns2_pro` to true. The `vigem_missing` and `ble_rate_limited_win10` warnings are currently reported only in the controller-status payload and the log; the GUI does not display them yet (coming with the UX redesign). If a game gets no input from the Switch 2 Pro in no-mapping mode (the default), install [ViGEmBus 1.22.0](https://github.com/nefarius/ViGEmBus/releases/tag/v1.22.0) or switch `mapping_enabled.ns2_pro` to true.
- **Rumble on connect** — the controller the app starts using gives one short buzz. Set `rumble_on_connect` to false in `padtokbm.json` to turn it off.

Not supported yet:

- DualSense USB transport (deferred to v2.0.1).
- DualSense Edge (`054c:0df2`, deferred to v2.0.1).
- Xbox / 8BitDo / generic XInput pads — v1.2.0 is the last release with the gilrs-based generic-pad path; v2.0.0 moves to a DualSense-specific raw HID reader so the connection state, touchpad, IMU and battery can be read directly from the 78-byte HID report.

## Why this exists

Existing mapper tools dropped key-release events under load, leaving keys "stuck." The Python prototype solved this with a three-layer release-on-exit defense and a macro engine with randomized delays so that scripted-feeling input patterns don't get flagged by online games. The Rust rewrite preserves both, fixes two latent bugs from the Python version (trigger idle-value mismatch across platforms; shared-key release collision), and packages it for non-technical users.

See:

- [`rust/README.md`](rust/README.md) — build, run, button reference
- [`legacy-python/README.md`](legacy-python/README.md) — original Python notes
- [`CHANGELOG.md`](CHANGELOG.md) — per-release history
