use crate::config::Config;
use crate::engine::Engine;
use crate::safety;
use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub struct RunOptions {
    pub dry_run: bool,
    pub shutdown: Arc<AtomicBool>,
}

pub fn run(cfg: Config, opts: RunOptions) -> Result<()> {
    // The panic hook is installed once, at the top of main::real_main, before
    // either CLI or GUI dispatch — so it is already active when we arrive here.
    // Iron Rule #3 is satisfied by that hook (abnormal path) and by
    // KeyboardSink::Drop → release_all_held (normal shutdown path).
    let engine = Engine::spawn(cfg, opts.dry_run)?;
    // Bind the engine's key state to the global so the panic hook can drain it.
    safety::register_global(engine.handle().key_state());
    // ...and to its virtual pad, so the hook can unplug it.
    crate::xinput_sink::register_global(engine.handle().virtual_pad());
    while !opts.shutdown.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(50));
    }
    // shutdown() signals the engine thread and joins it; the engine thread's
    // graceful-shutdown path drains macros and then drops KeyboardSink, which
    // calls release_all_held() — Iron Rule #3 guaranteed.
    engine.shutdown();
    Ok(())
}
