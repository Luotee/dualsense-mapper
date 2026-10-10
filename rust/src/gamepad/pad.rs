//! Protocol-neutral pad snapshot shared by every HID decoder
//! (DualSense 0x31, Switch Pro, ...). Diffing two of these yields the
//! `GamepadEvent` stream.

/// Number of button slots in a pad snapshot (ids `0..=29`).
pub const BUTTON_SLOTS: usize = 30;

/// Decoded snapshot of a single input frame.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PadState {
    pub stick_lx: f32,
    pub stick_ly: f32,
    pub stick_rx: f32,
    pub stick_ry: f32,
    pub trigger_l2: f32,
    pub trigger_r2: f32,
    /// Bit-vector of buttons indexed `0..BUTTON_SLOTS`. Same ids as v1.x
    /// `gamepad::button_index`, so existing config files migrate as-is.
    pub buttons: [bool; BUTTON_SLOTS],
    /// Touchpad finger 0: true when a finger is currently in contact.
    pub finger0_active: bool,
    /// Touchpad x in 0..=1919.
    pub finger0_x: u16,
    /// Touchpad y in 0..=1079.
    pub finger0_y: u16,
    /// Whole-touchpad physical click switch.
    pub touchpad_btn: bool,
}
