use crate::config::{MacroDef, StepAction};
use crate::mapper::{KeyAction, Source};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct MacroEngine {
    /// Per source_id (button id that started the macro), the cancel flag.
    active: HashMap<u32, Arc<AtomicBool>>,
    tx: Sender<KeyAction>,
}

impl MacroEngine {
    pub fn new(tx: Sender<KeyAction>) -> Self {
        Self { active: HashMap::new(), tx }
    }

    /// Run macro `name` (its keys carry `Source::Macro(name)`) for button
    /// `source_id`.
    pub fn start(&mut self, source_id: u32, name: &str, def: MacroDef) {
        // If already running for this source, stop first.
        self.stop(source_id);
        let cancel = Arc::new(AtomicBool::new(false));
        self.active.insert(source_id, cancel.clone());
        let tx = self.tx.clone();
        let src = Source::Macro(name.to_string());
        thread::spawn(move || run_macro(def, src, cancel, tx));
    }

    pub fn stop(&mut self, source_id: u32) {
        if let Some(flag) = self.active.remove(&source_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    pub fn stop_all(&mut self) {
        for (_, flag) in self.active.drain() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

fn run_macro(def: MacroDef, src: Source, cancel: Arc<AtomicBool>, tx: Sender<KeyAction>) {
    // Keys this macro has Pressed but not yet Released. If we exit (cancelled
    // or natural end) with anything still held, emit Release for each so the
    // KeyboardSink refcount returns to zero and OS state is balanced.
    // Without this, mid-macro cancellation strands a KEYDOWN at the OS level
    // and any later binding to the same key appears to "fail" (it never gets
    // a fresh edge because refcount is already > 0).
    let mut held: std::collections::HashSet<String> = std::collections::HashSet::new();

    let drain = |held: &mut std::collections::HashSet<String>, tx: &Sender<KeyAction>| {
        for k in held.drain() {
            tracing::info!(key = %k, "macro draining unmatched Press on exit");
            let _ = tx.send(KeyAction::Release(k, src.clone()));
        }
    };

    // No step means the cancel flag below is never checked: a looping empty
    // macro would spin this thread forever. It sends nothing anyway.
    if def.steps.is_empty() {
        return;
    }

    loop {
        for step in &def.steps {
            if cancel.load(Ordering::SeqCst) {
                drain(&mut held, &tx);
                return;
            }
            let action = match step.action {
                StepAction::Down => {
                    held.insert(step.key.clone());
                    KeyAction::Press(step.key.clone(), src.clone())
                }
                StepAction::Up => {
                    held.remove(&step.key);
                    KeyAction::Release(step.key.clone(), src.clone())
                }
            };
            if tx.send(action).is_err() {
                // Receiver gone; can't drain via channel — refcount will be
                // cleaned up by KeyboardSink::Drop on shutdown anyway.
                return;
            }
            let [lo, hi] = step.delay_ms;
            // Config validation guarantees lo < hi, so the inclusive range is non-empty.
            let ms = fastrand::u32(lo..=hi);
            interruptible_sleep(Duration::from_millis(ms as u64), &cancel);
            if cancel.load(Ordering::SeqCst) {
                drain(&mut held, &tx);
                return;
            }
        }
        if !def.repeat {
            drain(&mut held, &tx);
            return;
        }
    }
}

fn interruptible_sleep(total: Duration, cancel: &AtomicBool) {
    let step = Duration::from_millis(5);
    let mut slept = Duration::ZERO;
    while slept < total {
        if cancel.load(Ordering::SeqCst) { return; }
        let s = std::cmp::min(step, total - slept);
        thread::sleep(s);
        slept += s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MacroDef, MacroStep, StepAction};
    use std::sync::mpsc;
    use std::time::Instant;

    fn one_step_loop(key: &str, range: [u32; 2]) -> MacroDef {
        MacroDef {
            repeat: true,
            steps: vec![MacroStep { key: key.into(), action: StepAction::Down, delay_ms: range }],
        }
    }

    #[test]
    fn macro_keys_carry_macro_source() {
        let (tx, rx) = mpsc::channel::<KeyAction>();
        let mut eng = MacroEngine::new(tx);
        eng.start(7, "連發", one_step_loop("j", [5, 10]));
        let first = rx.recv_timeout(Duration::from_millis(500)).expect("first action");
        assert_eq!(first, KeyAction::Press("j".into(), crate::mapper::Source::Macro("連發".into())));
        eng.stop(7);
        // The drain on stop releases with the same source.
        let tail: Vec<KeyAction> = std::iter::from_fn(|| rx.recv_timeout(Duration::from_millis(200)).ok()).collect();
        assert!(tail.iter().any(|a| *a == KeyAction::Release("j".into(), crate::mapper::Source::Macro("連發".into()))), "{tail:?}");
    }

    #[test]
    fn start_then_stop_terminates_quickly() {
        let (tx, rx) = mpsc::channel::<KeyAction>();
        let mut eng = MacroEngine::new(tx);
        eng.start(23, "m", one_step_loop("Left", [5, 10]));

        // wait for at least one action
        let first = rx.recv_timeout(Duration::from_millis(500)).expect("first action");
        assert!(matches!(first, KeyAction::Press(ref k, _) if k == "Left"));

        let stop_at = Instant::now();
        eng.stop(23);

        // After stop, channel must go quiet within a bounded window.
        let mut last = Instant::now();
        while rx.recv_timeout(Duration::from_millis(50)).is_ok() {
            last = Instant::now();
        }
        let drain_ms = (last - stop_at).as_millis();
        assert!(drain_ms < 200, "macro drained too slowly: {drain_ms}ms");
    }

    #[test]
    fn non_repeat_macro_runs_once_and_drains() {
        // A non-repeat macro that only Presses must, on natural exit, emit
        // Release for every Press it didn't pair up. Otherwise it would leak
        // KEYDOWNs into the OS state (same class of bug as mid-loop cancel).
        let (tx, rx) = mpsc::channel::<KeyAction>();
        let mut eng = MacroEngine::new(tx);
        let def = MacroDef {
            repeat: false,
            steps: vec![
                MacroStep { key: "Left".into(),  action: StepAction::Down, delay_ms: [1, 5] },
                MacroStep { key: "Right".into(), action: StepAction::Down, delay_ms: [1, 5] },
            ],
        };
        eng.start(24, "m", def);

        let mut got = Vec::new();
        while let Ok(a) = rx.recv_timeout(Duration::from_millis(200)) {
            got.push(a);
        }
        // 2 Presses + 2 drained Releases.
        assert_eq!(got.len(), 4, "got {got:?}");
        // Step order: Press Left, Press Right come out in order.
        assert!(matches!(got[0], KeyAction::Press(ref k, _) if k == "Left"));
        assert!(matches!(got[1], KeyAction::Press(ref k, _) if k == "Right"));
        // Drain order is HashSet iteration order — not stable — so assert membership.
        let tail: Vec<&KeyAction> = got[2..].iter().collect();
        assert!(tail.iter().any(|a| matches!(a, KeyAction::Release(k, _) if k == "Left")),
                "drain missing Release Left: {got:?}");
        assert!(tail.iter().any(|a| matches!(a, KeyAction::Release(k, _) if k == "Right")),
                "drain missing Release Right: {got:?}");
    }

    #[test]
    fn cancel_between_press_and_release_drains_held_keys() {
        // macro_A-style: Press Left, delay 200ms, Release Left, delay 200ms, ...
        // Stop while we're inside the post-Press delay; the engine MUST emit
        // a synthetic Release Left before the thread exits, otherwise the
        // refcounted KEYDOWN at the OS level is stranded.
        let (tx, rx) = mpsc::channel::<KeyAction>();
        let mut eng = MacroEngine::new(tx);
        let def = MacroDef {
            repeat: true,
            steps: vec![
                MacroStep { key: "Left".into(),  action: StepAction::Down, delay_ms: [200, 250] },
                MacroStep { key: "Left".into(),  action: StepAction::Up,   delay_ms: [200, 250] },
                MacroStep { key: "Right".into(), action: StepAction::Down, delay_ms: [200, 250] },
                MacroStep { key: "Right".into(), action: StepAction::Up,   delay_ms: [200, 250] },
            ],
        };
        eng.start(23, "m", def);

        // First emitted action is Press Left.
        let first = rx.recv_timeout(Duration::from_millis(500)).expect("first action");
        assert!(matches!(first, KeyAction::Press(ref k, _) if k == "Left"));

        // Stop mid-delay, before the macro would naturally emit Release Left.
        std::thread::sleep(Duration::from_millis(50));
        eng.stop(23);

        // Collect every remaining action before the channel drains.
        let mut after = Vec::new();
        while let Ok(a) = rx.recv_timeout(Duration::from_millis(300)) {
            after.push(a);
        }
        // The drain must contain at least one Release Left.
        assert!(
            after.iter().any(|a| matches!(a, KeyAction::Release(k, _) if k == "Left")),
            "expected drained Release Left after cancel, got {after:?}"
        );
        // And must NOT contain any Press Left (we cancelled before next iteration).
        assert!(
            !after.iter().any(|a| matches!(a, KeyAction::Press(k, _) if k == "Left")),
            "expected no further Press Left after cancel, got {after:?}"
        );
    }

    #[test]
    fn empty_looping_macro_returns_instead_of_spinning() {
        // A macro saved with no steps and `loop: true` (the board's ＋ then
        // 按住時重複) must not busy-loop: with no step, the cancel flag is
        // never checked, so the thread could never be stopped.
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let def = MacroDef { repeat: true, steps: vec![] };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            run_macro(def, Source::Macro("m".into()), cancel, tx);
            let _ = done_tx.send(());
        });
        assert!(done_rx.recv_timeout(Duration::from_millis(500)).is_ok(), "run_macro spun on an empty looping macro");
        assert!(rx.try_recv().is_err(), "an empty macro sends nothing");
    }
}

