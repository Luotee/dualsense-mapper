//! Gamepad event source abstraction.
//!
//! v2.0.0 onwards: DualSense BT raw HID only via `hidapi`. The v1.x
//! gilrs path has been removed. See
//! `docs/superpowers/specs/2026-05-17-v2.0.0-raw-hid-dualsense-design.md`
//! for the design.

pub mod controller;
pub mod cursor_params;
pub mod events;
pub mod ds_protocol;
pub mod hid_source;
pub mod ns_protocol;
pub mod pad;

pub use controller::ControllerKind;
pub use cursor_params::CursorParams;
pub use events::GamepadEvent;
pub use hid_source::HidSource as GamepadSource;
pub use pad::{PadState, BUTTON_SLOTS};
