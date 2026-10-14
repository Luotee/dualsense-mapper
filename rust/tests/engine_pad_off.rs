//! v3.2.0 per-pad 關 (`Handle::set_pad_off`): runtime only, keyed by pad
//! kind. While the session owner is off nothing is synthesized and its
//! virtual pad is unplugged; the on→off edge releases every held key and
//! stops its macros (iron rules 2–4), and switching back resumes cleanly.

#[allow(dead_code)]
mod test_helpers;

use dualsense_mapper::config::{Binding, ButtonEntry, Config, MacroDef, MacroStep, StepAction};
use dualsense_mapper::engine::{Engine, EngineEvent, Handle};
use dualsense_mapper::gamepad::{ControllerKind, GamepadEvent};
use dualsense_mapper::virtual_pad::XState;
use dualsense_mapper::xinput_sink::{PadError, VirtualPad};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const STEP: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Ops {
    plugs: usize,
    unplugs: usize,
}

struct FakePad(Arc<Mutex<Ops>>);

impl VirtualPad for FakePad {
    fn plug(&mut self) -> Result<(), PadError> {
        self.0.lock().unwrap().plugs += 1;
        Ok(())
    }
    fn update(&mut self, _: &XState) -> Result<(), PadError> {
        Ok(())
    }
    fn unplug(&mut self) {
        self.0.lock().unwrap().unplugs += 1;
    }
}

fn spawn(cfg: Config) -> (Engine, Handle, Arc<Mutex<Ops>>) {
    let ops = Arc::new(Mutex::new(Ops::default()));
    let engine = Engine::spawn_with_fake_gamepad_and_pad(cfg, Box::new(FakePad(ops.clone()))).unwrap();
    let h = engine.handle();
    (engine, h, ops)
}

fn ev(h: &Handle, e: GamepadEvent) {
    h.fake_event(e);
    std::thread::sleep(STEP);
}

fn with_hold_left_macro(mut cfg: Config) -> Config {
    cfg.macros.insert(
        "hold_left".into(),
        MacroDef {
            repeat: false,
            steps: vec![
                MacroStep { key: "Left".into(), action: StepAction::Down, delay_ms: [500, 600] },
                MacroStep { key: "Left".into(), action: StepAction::Up, delay_ms: [15, 25] },
            ],
        },
    );
    cfg.bind_everywhere(1, ButtonEntry { label: "Circle".into(), binding: Binding::Macro("hold_left".into()) });
    cfg
}

#[test]
fn pad_off_edge_releases_and_unplugs() {
    // Keys: a held key and a running macro are released on the edge.
    let (engine, h, _) = spawn(with_hold_left_macro(test_helpers::load_example()));
    ev(&h, GamepadEvent::Connected(ControllerKind::Ns2Pro));
    ev(&h, GamepadEvent::ButtonDown(0));
    h.fake_button_down(1);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(h.held_keys_count(), 2, "bound key + macro's Left held");
    h.set_pad_off(ControllerKind::Ns2Pro, true);
    std::thread::sleep(STEP);
    assert_eq!(h.held_keys_count(), 0, "off edge releases everything");
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(h.held_keys_count(), 0, "the stopped macro presses nothing later");
    ev(&h, GamepadEvent::ButtonUp(0));
    ev(&h, GamepadEvent::ButtonDown(0));
    assert_eq!(h.held_keys_count(), 0, "nothing synthesized while off");
    ev(&h, GamepadEvent::ButtonUp(0));
    h.set_pad_off(ControllerKind::Ns2Pro, false);
    std::thread::sleep(STEP);
    ev(&h, GamepadEvent::ButtonDown(0));
    assert_eq!(h.held_keys_count(), 1, "back on: keys resume");
    ev(&h, GamepadEvent::ButtonUp(0));
    engine.shutdown();

    // Xbox mode: the virtual pad is unplugged while off and plugged again after.
    let mut cfg = test_helpers::load_example();
    cfg.mapping_enabled.insert("ns2_pro".into(), false);
    let (engine, h, ops) = spawn(cfg);
    ev(&h, GamepadEvent::Connected(ControllerKind::Ns2Pro));
    assert_eq!(ops.lock().unwrap().plugs, 1);
    h.set_pad_off(ControllerKind::Ns2Pro, true);
    std::thread::sleep(STEP);
    assert_eq!(ops.lock().unwrap().unplugs, 1, "off unplugs the virtual pad");
    h.set_pad_off(ControllerKind::Ns2Pro, false);
    std::thread::sleep(STEP);
    assert_eq!(ops.lock().unwrap().plugs, 2, "back to Xbox plugs again");
    engine.shutdown();
}

#[test]
fn pad_off_only_affects_that_kind() {
    let (engine, h, _) = spawn(test_helpers::load_example());
    h.set_pad_off(ControllerKind::Ns2Pro, true);
    ev(&h, GamepadEvent::Connected(ControllerKind::DualSense));
    ev(&h, GamepadEvent::ButtonDown(0));
    assert_eq!(h.held_keys_count(), 1, "DualSense still sends keys");
    ev(&h, GamepadEvent::ButtonUp(0));
    engine.shutdown();
}

#[test]
fn pad_off_set_before_connect_applies_on_connect() {
    let (engine, h, _) = spawn(test_helpers::load_example());
    h.set_pad_off(ControllerKind::DualSense, true);
    ev(&h, GamepadEvent::Connected(ControllerKind::DualSense));
    ev(&h, GamepadEvent::ButtonDown(0));
    assert_eq!(h.held_keys_count(), 0, "off survives (re)connect");
    // Button events still reach the board.
    assert!(h.drain_events().iter().any(|e| matches!(e, EngineEvent::ButtonDown { id: 0 })));
    engine.shutdown();
}

#[test]
fn pad_off_clears_on_set_false() {
    let (engine, h, _) = spawn(test_helpers::load_example());
    h.set_pad_off(ControllerKind::DualSense, true);
    h.set_pad_off(ControllerKind::NsPro, true);
    assert_eq!(h.off_pads(), vec![ControllerKind::DualSense, ControllerKind::NsPro]);
    h.set_pad_off(ControllerKind::DualSense, false);
    assert_eq!(h.off_pads(), vec![ControllerKind::NsPro]);
    h.set_pad_off(ControllerKind::NsPro, false);
    assert!(h.off_pads().is_empty());
    engine.shutdown();
}

#[test]
fn off_pads_in_status() {
    let (engine, h, _) = spawn(test_helpers::load_example());
    ev(&h, GamepadEvent::Connected(ControllerKind::DualSense));
    assert!(h.current_status().unwrap().off_pads.is_empty());
    h.drain_events();
    h.set_pad_off(ControllerKind::DualSense, true);
    std::thread::sleep(STEP);
    assert_eq!(h.current_status().unwrap().off_pads, vec![ControllerKind::DualSense]);
    let evs = h.drain_events();
    assert!(
        evs.iter().any(|e| matches!(e, EngineEvent::ControllerConnected { off_pads, .. } if off_pads == &vec![ControllerKind::DualSense])),
        "status re-sent with off_pads: {evs:?}"
    );
    let json = serde_json::to_value(h.current_status().unwrap()).unwrap();
    assert_eq!(json["off_pads"], serde_json::json!(["dualsense"]));
    engine.shutdown();
}

#[test]
fn set_pad_off_impl_parses_slug_and_returns_off_list() {
    use dualsense_mapper::gui::commands::set_pad_off_impl;
    let (engine, h, _) = spawn(test_helpers::load_example());
    assert_eq!(set_pad_off_impl(&h, "ns2_pro", true).unwrap(), vec!["ns2_pro".to_string()]);
    assert!(set_pad_off_impl(&h, "ps5", true).is_err());
    assert_eq!(set_pad_off_impl(&h, "ns2_pro", false).unwrap(), Vec::<String>::new());
    engine.shutdown();
}
