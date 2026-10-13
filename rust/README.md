# PadToKBM (Rust)

Gamepad → keyboard and mouse mapper (formerly DualSense Mapper). Single executable, JSON config, supports macros with randomized delays.

## Status

Phase 1: Windows build. macOS support comes in Phase 2.

## Supported hardware

- **DualSense PS5 controller** (`054c:0ce6`) over **Bluetooth**.

v2.0.0 dropped the gilrs path and reads the DualSense BT 0x31 report directly via hidapi. **USB transport and DualSense Edge are deferred to v2.0.1**; non-DualSense pads (Xbox / 8BitDo / generic XInput) are not supported — last release with that support was v1.2.0.

## Quick start (Windows, double-click)

1. Download `PadToKBM.exe` from the [latest release](https://github.com/Luotee/pad-to-kbm/releases) into a folder you can write to (e.g. `C:\Users\<you>\Downloads\PadToKBM\`).
2. Pair DualSense over Bluetooth (PS-button-hold + Share-button-hold until the bar flashes, then add from Windows Bluetooth settings).
3. Double-click `PadToKBM.exe`. The console window opens with a startup banner. On first run it also writes `padtokbm.json` next to the exe (an older `dualsense-mapper.json` there is renamed to it, with a `.bak` copy kept) — that file has an inline keyboard cheat sheet at the top.
4. Press gamepad buttons; the mapped keys are sent to the focused application.
5. To customize: open `padtokbm.json` in Notepad, change the `value` field of any binding to a key name from the cheat sheet, save, restart the exe.
6. To quit: press **Ctrl-C** inside the console window, or just close the window.

Every held key is released on exit, on panic, and on controller disconnect, so nothing gets stuck.

The folder is portable: copy `PadToKBM.exe` + `padtokbm.json` to a USB stick, plug it into another Windows machine, run from there.

If anything goes wrong, the program shows the error and waits for you to press Enter — the console will not flash and disappear.

## CLI

| Flag | Purpose |
|---|---|
| `--config PATH` | Use a specific config file. |
| `--validate` | Parse and validate the config, then exit. |
| `--dry-run` | Print every synthesized event instead of typing real keys. Use this before going live in a game. |
| `--list-buttons` | Print every gamepad event with its label. Use this to discover which id each button has on your system. |
| `--verbose` | Debug-level logs. |
| `--no-pause` | Skip the "Press Enter to close" prompt that appears on errors. For CLI / CI use. |

## Button id reference

Each controller has its own table under `pads` (`dualsense`, `ns_pro`, `ns2_pro`), and each table's `buttons` must list every id 0..=32, even if `"type": "unbound"`. This is so the user can see the full surface without guessing. The on-device readout from `--list-buttons` is authoritative — gilrs may map ids differently across USB vs Bluetooth or across OSes.

| Id | Source | Default label |
|---:|---|---|
| 0 | Physical | Cross（叉叉） |
| 1 | Physical | Circle（圈圈） |
| 2 | Physical | Square（正方形） |
| 3 | Physical | Triangle（三角） |
| 4 | Physical | Share（拍照鍵） |
| 5 | Physical | PS 按鈕 |
| 6 | Physical | Options（Menu 鍵） |
| 7 | Physical | L3 |
| 8 | Physical | R3 |
| 9 | Physical | L1 |
| 10 | Physical | R1 |
| 11 | Physical | D-pad ↑ |
| 12 | Physical | D-pad ↓ |
| 13 | Physical | D-pad ← |
| 14 | Physical | D-pad → |
| 15 | Virtual (L-stick Y ≥ deadzone) | L-stick ↑ |
| 16 | Virtual (L-stick Y ≤ −deadzone) | L-stick ↓ |
| 17 | Virtual (L-stick X ≤ −deadzone) | L-stick ← |
| 18 | Virtual (L-stick X ≥ deadzone) | L-stick → |
| 19 | Virtual (R-stick Y ≥ deadzone) | R-stick ↑ |
| 20 | Virtual (R-stick Y ≤ −deadzone) | R-stick ↓ |
| 21 | Virtual (R-stick X ≤ −deadzone) | R-stick ← |
| 22 | Virtual (R-stick X ≥ deadzone) | R-stick → |
| 23 | Axis (L2 ≥ trigger_threshold) OR Button::LeftTrigger2 | L2（類比板機） |
| 24 | Axis (R2 ≥ trigger_threshold) OR Button::RightTrigger2 | R2（類比板機） |

## Anti-cheat self-discipline

This tool is not a cheat — it remaps physical inputs to keyboard inputs. To avoid being misclassified as one, the synthesis pipeline avoids fixed-period patterns:

- Macro step delays are always `[min, max]` ranges, never constants.
- Synthesized keys honor `min_press_ms` (randomized minimum down/up gap).
- Simultaneous events on the same gamepad tick are spread by `tick_jitter_ms`.
- No process hooking, DLL injection, or driver. User-mode `SendInput` (Windows) / `CGEvent` (macOS, Phase 2) only.
- `log_events: true` writes every synthesized event with a timestamp so you can self-audit.

## Build

```
cd rust
cargo build --release
```

Binary lands at `target/release/PadToKBM.exe`. Strip + LTO are enabled in release; expect ~3–5 MB.

## Tests

```
cargo test
```
