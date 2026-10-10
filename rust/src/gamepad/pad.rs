//! Protocol-neutral pad snapshot shared by every HID decoder
//! (DualSense 0x31, Switch Pro, ...). Diffing two of these yields the
//! `GamepadEvent` stream.

/// Number of button slots in a pad snapshot (ids `0..=32`).
pub const BUTTON_SLOTS: usize = 33;

/// Decoded snapshot of a single input frame.
#[derive(Debug, Clone, PartialEq)]
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

// Hand-written: std only derives `Default` for arrays up to 32 elements and
// `BUTTON_SLOTS` is 33.
impl Default for PadState {
    fn default() -> Self {
        PadState {
            stick_lx: 0.0,
            stick_ly: 0.0,
            stick_rx: 0.0,
            stick_ry: 0.0,
            trigger_l2: 0.0,
            trigger_r2: 0.0,
            buttons: [false; BUTTON_SLOTS],
            finger0_active: false,
            finger0_x: 0,
            finger0_y: 0,
            touchpad_btn: false,
        }
    }
}
