use dualsense_mapper::config::Config;
use std::path::PathBuf;

#[test]
fn shipped_example_config_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
    let cfg = Config::load_from_path(&path).expect("load+validate");
    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.buttons.len(), 33);
    // The shipped default keeps a sample `macro_A` definition in the
    // `macros` section so users can see the schema even if no button is
    // bound to it out of the box.
    assert!(cfg.macros.contains_key("macro_A"));
}

#[test]
fn shipped_maple_profile_validates() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/maple_artale.json");
    Config::load_from_path(&path).expect("maple profile load+validate");
}
