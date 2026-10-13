use crate::config::{Binding, Config, MouseButton};
use crate::gamepad::GamepadEvent;

#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Press(String),       // key value as written in config (canonical)
    Release(String),
    MacroStart { name: String, source_id: u32 },
    MacroStop { source_id: u32 },
    MousePress(MouseButton),
    MouseRelease(MouseButton),
    MouseMove { dx: i32, dy: i32 },
    /// Pure diagnostic — engine forwards as a Tauri event so the GUI
    /// can show a debug dot at the captured touchpad position.
    TouchpadClick { raw_x: u16, raw_y: u16, quadrant: u32 },
    /// Continuous touchpad hover preview. Emitted per HID frame on
    /// quadrant change, forwarded to Tauri as a diagnostic event for GUI
    /// hover rendering. `quadrant = 255` is the sentinel for "finger lifted".
    TouchpadHover { raw_x: u16, raw_y: u16, quadrant: u32 },
}

pub struct Mapper {
    cfg: Config,
    /// Last reported value per stick axis (0..=3) and trigger axis (4..=5).
    last_axis: [f32; 6],
    /// Active virtual-button state for indices 15..=24.
    virt_active: [bool; 25],
    /// Accumulates `(id, pressed)` for any virtual-button state flip that
    /// happens inside `transition_virtual` — sticks (15..=22) and analog
    /// triggers (23..=24). The engine drains this every tick via
    /// `take_visual_transitions` and forwards each entry as an
    /// `EngineEvent::ButtonDown` / `ButtonUp` so the GUI can flash the
    /// same press-ring it shows for real, gilrs-delivered button events.
    pending_visual: Vec<(u32, bool)>,
}

impl Mapper {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            last_axis: [0.0; 6],
            virt_active: [false; 25],
            pending_visual: Vec::new(),
        }
    }

    /// Drain the buffered virtual-button transitions accumulated since the
    /// last call. Each entry is `(id, pressed)`; pressed=true means the
    /// virtual press just went active, false means it just released.
    pub fn take_visual_transitions(&mut self) -> Vec<(u32, bool)> {
        std::mem::take(&mut self.pending_visual)
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn handle(&mut self, ev: GamepadEvent) -> Vec<KeyAction> {
        match ev {
            GamepadEvent::Connected(_) | GamepadEvent::Disconnected => Vec::new(),
            GamepadEvent::ButtonDown(id) => self.physical_down(id),
            GamepadEvent::ButtonUp(id)   => self.physical_up(id),
            GamepadEvent::Stick { axis, value } => {
                self.last_axis[axis as usize] = value;
                self.update_axis_virtuals(axis)
            }
            GamepadEvent::Trigger { axis, value } => {
                self.last_axis[axis as usize] = value;
                self.update_trigger_virtuals(axis)
            }
            GamepadEvent::MouseDelta { dx, dy } => vec![KeyAction::MouseMove { dx, dy }],
            GamepadEvent::TouchpadClick { raw_x, raw_y, quadrant } => {
                vec![KeyAction::TouchpadClick { raw_x, raw_y, quadrant }]
            }
            GamepadEvent::TouchpadHover { raw_x, raw_y, quadrant } => {
                vec![KeyAction::TouchpadHover { raw_x, raw_y, quadrant }]
            }
        }
    }

    fn binding_for(&self, id: u32) -> Option<&Binding> {
        self.cfg
            .pad(crate::gamepad::ControllerKind::DualSense)
            .buttons
            .get(&id.to_string())
            .map(|e| &e.binding)
    }

    fn physical_down(&self, id: u32) -> Vec<KeyAction> {
        match self.binding_for(id) {
            Some(Binding::Key(k)) => {
                tracing::info!(id, key = %k, "press");
                vec![KeyAction::Press(k.clone())]
            }
            Some(Binding::Macro(n)) => {
                tracing::info!(id, macro_name = %n, "macro start");
                vec![KeyAction::MacroStart { name: n.clone(), source_id: id }]
            }
            Some(Binding::Mouse(b)) => {
                tracing::info!(id, ?b, "mouse press");
                vec![KeyAction::MousePress(*b)]
            }
            _ => {
                tracing::debug!(id, "physical_down: id has no binding or is unbound");
                Vec::new()
            }
        }
    }

    fn physical_up(&self, id: u32) -> Vec<KeyAction> {
        match self.binding_for(id) {
            Some(Binding::Key(k)) => {
                tracing::info!(id, key = %k, "release");
                vec![KeyAction::Release(k.clone())]
            }
            Some(Binding::Macro(_)) => {
                tracing::info!(id, "macro stop");
                vec![KeyAction::MacroStop { source_id: id }]
            }
            Some(Binding::Mouse(b)) => {
                tracing::info!(id, ?b, "mouse release");
                vec![KeyAction::MouseRelease(*b)]
            }
            _ => Vec::new(),
        }
    }

    fn update_axis_virtuals(&mut self, axis: u32) -> Vec<KeyAction> {
        // gilrs convention: stick X negative = left, positive = right;
        //                   stick Y negative = down, positive = up.
        // (Note: this differs from pygame, which inverts Y. The legacy Python
        //  used pygame's flipped convention; the Rust port matches gilrs directly.)
        // axis 0 (lx): neg → 17 (left),  pos → 18 (right)
        // axis 1 (ly): neg → 16 (down),  pos → 15 (up)
        // axis 2 (rx): neg → 21 (left),  pos → 22 (right)
        // axis 3 (ry): neg → 20 (down),  pos → 19 (up)
        let (neg_id, pos_id) = match axis {
            0 => (17, 18),
            1 => (16, 15),
            2 => (21, 22),
            3 => (20, 19),
            _ => return Vec::new(),
        };
        let v = self.last_axis[axis as usize];
        let dz = self.cfg.deadzone;
        let want_neg = v < -dz;
        let want_pos = v > dz;
        let mut out = Vec::new();
        out.extend(self.transition_virtual(neg_id, want_neg));
        out.extend(self.transition_virtual(pos_id, want_pos));
        out
    }

    fn update_trigger_virtuals(&mut self, axis: u32) -> Vec<KeyAction> {
        // axis 4 (l2) → 23, axis 5 (r2) → 24. Already normalized to [0, 1].
        let id = match axis { 4 => 23, 5 => 24, _ => return Vec::new() };
        let v = self.last_axis[axis as usize];
        let want = v >= self.cfg.trigger_threshold;
        self.transition_virtual(id, want)
    }

    fn transition_virtual(&mut self, id: u32, want_active: bool) -> Vec<KeyAction> {
        let was = self.virt_active[id as usize];
        if want_active == was { return Vec::new(); }
        self.virt_active[id as usize] = want_active;
        // Record the transition so the engine can re-emit it as a visual
        // ButtonDown/Up event (the GUI press-ring otherwise never lights up
        // for stick directions or analog-trigger virtual ids, because those
        // never come back as a real gilrs ButtonPressed event).
        self.pending_visual.push((id, want_active));
        if want_active { self.physical_down(id) } else { self.physical_up(id) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ButtonEntry, MacroDef, MacroStep, StepAction};
    use std::collections::BTreeMap;

    fn cfg_with_overrides(mut overrides: Vec<(u32, Binding)>) -> Config {
        let mut buttons = BTreeMap::new();
        for id in crate::config::VALID_BUTTON_IDS {
            buttons.insert(id.to_string(), ButtonEntry {
                label: format!("b{id}"),
                binding: Binding::Unbound,
            });
        }
        for (id, b) in overrides.drain(..) {
            buttons.insert(id.to_string(), ButtonEntry { label: format!("b{id}"), binding: b });
        }
        let mut macros = BTreeMap::new();
        macros.insert("m_left_right".into(), MacroDef {
            repeat: true,
            steps: vec![MacroStep {
                key: "Left".into(), action: StepAction::Down, delay_ms: [10, 20],
            }],
        });
        let pads = crate::gamepad::ControllerKind::ALL
            .into_iter()
            .map(|k| (k.slug().to_string(), crate::pad_bindings::PadBindings {
                buttons: buttons.clone(),
                quick_l: BTreeMap::new(),
                quick_r: BTreeMap::new(),
                trigger_mode: Default::default(),
            }))
            .collect();
        Config {
            version: 2,
            deadzone: 0.4,
            trigger_threshold: 0.5,
            min_press_ms: [8, 25],
            tick_jitter_ms: [0, 3],
            log_events: false,
            pads, macros,
            touchpad_cursor_enabled: true,
            touchpad_cursor_sensitivity: 1.5,
            touchpad_midpoint_x: 960,
            touchpad_midpoint_y: 540,
            touchpad_accel_slow_threshold: 5,
            touchpad_accel_fast_threshold: 20,
            touchpad_accel_gain_slow: 0.5,
            touchpad_accel_gain_fast: 1.5,
            touchpad_deadzone_radius: 2,
            touchpad_click_freeze_enabled: true,
            mapping_enabled: Default::default(),
            rumble_on_connect: true,
        }
    }

    #[test]
    fn physical_key_press_release() {
        let cfg = cfg_with_overrides(vec![(0, Binding::Key("x".into()))]);
        let mut m = Mapper::new(cfg);
        assert_eq!(m.handle(GamepadEvent::ButtonDown(0)),
                   vec![KeyAction::Press("x".into())]);
        assert_eq!(m.handle(GamepadEvent::ButtonUp(0)),
                   vec![KeyAction::Release("x".into())]);
    }

    #[test]
    fn unbound_button_yields_nothing() {
        let cfg = cfg_with_overrides(vec![]);
        let mut m = Mapper::new(cfg);
        assert!(m.handle(GamepadEvent::ButtonDown(3)).is_empty());
    }

    #[test]
    fn stick_crosses_deadzone_then_returns() {
        let cfg = cfg_with_overrides(vec![(17, Binding::Key("Left".into()))]);
        let mut m = Mapper::new(cfg);

        // below deadzone — nothing
        assert!(m.handle(GamepadEvent::Stick { axis: 0, value: -0.2 }).is_empty());

        // cross deadzone negative — press 17
        assert_eq!(
            m.handle(GamepadEvent::Stick { axis: 0, value: -0.9 }),
            vec![KeyAction::Press("Left".into())]
        );

        // still active — nothing further
        assert!(m.handle(GamepadEvent::Stick { axis: 0, value: -0.7 }).is_empty());

        // return inside deadzone — release 17
        assert_eq!(
            m.handle(GamepadEvent::Stick { axis: 0, value: 0.0 }),
            vec![KeyAction::Release("Left".into())]
        );
    }

    #[test]
    fn trigger_threshold_drives_virtual_23() {
        let cfg = cfg_with_overrides(vec![
            (23, Binding::Macro("m_left_right".into())),
        ]);
        let mut m = Mapper::new(cfg);
        // normalized value < threshold — nothing
        assert!(m.handle(GamepadEvent::Trigger { axis: 4, value: 0.1 }).is_empty());
        // cross threshold — macro start
        assert_eq!(
            m.handle(GamepadEvent::Trigger { axis: 4, value: 0.9 }),
            vec![KeyAction::MacroStart { name: "m_left_right".into(), source_id: 23 }]
        );
        // release below threshold — macro stop
        assert_eq!(
            m.handle(GamepadEvent::Trigger { axis: 4, value: 0.0 }),
            vec![KeyAction::MacroStop { source_id: 23 }]
        );
    }

    #[test]
    fn mouse_binding_press_release() {
        let cfg = cfg_with_overrides(vec![(25, Binding::Mouse(MouseButton::Left))]);
        let mut m = Mapper::new(cfg);
        assert_eq!(m.handle(GamepadEvent::ButtonDown(25)),
                   vec![KeyAction::MousePress(MouseButton::Left)]);
        assert_eq!(m.handle(GamepadEvent::ButtonUp(25)),
                   vec![KeyAction::MouseRelease(MouseButton::Left)]);
    }

    #[test]
    fn mouse_delta_passes_through_as_move() {
        let cfg = cfg_with_overrides(vec![]);
        let mut m = Mapper::new(cfg);
        assert_eq!(m.handle(GamepadEvent::MouseDelta { dx: 10, dy: -5 }),
                   vec![KeyAction::MouseMove { dx: 10, dy: -5 }]);
    }

    #[test]
    fn shared_key_two_sources_emit_independently() {
        // D-pad Up (id 11) and L-stick Up (id 15) both bound to "Up".
        // gilrs: ly positive = up, so axis 1 value +0.9 activates id 15.
        let cfg = cfg_with_overrides(vec![
            (11, Binding::Key("Up".into())),
            (15, Binding::Key("Up".into())),
        ]);
        let mut m = Mapper::new(cfg);
        assert_eq!(m.handle(GamepadEvent::ButtonDown(11)),
                   vec![KeyAction::Press("Up".into())]);
        assert_eq!(m.handle(GamepadEvent::Stick { axis: 1, value: 0.9 }),
                   vec![KeyAction::Press("Up".into())]);
        // Releasing one MUST still produce a Release action — refcount logic in
        // safety.rs decides whether to actually let the key go up.
        assert_eq!(m.handle(GamepadEvent::ButtonUp(11)),
                   vec![KeyAction::Release("Up".into())]);
    }
}
