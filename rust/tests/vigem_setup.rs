//! v3.2.0 ViGEmBus setup: the installer is the pinned official file, checked
//! by size and sha256 before anything runs; a bad file is deleted.

use dualsense_mapper::vigem_setup::{verify_installer, INSTALLER_SHA256, INSTALLER_SIZE, INSTALLER_URL};
use std::fs;

#[test]
fn pin_matches_the_official_release() {
    assert_eq!(
        INSTALLER_URL,
        "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe"
    );
    assert_eq!(INSTALLER_SIZE, 6_278_576);
    assert_eq!(INSTALLER_SHA256, "89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a");
}

#[test]
fn installer_rejects_bad_sha_and_size() {
    let dir = tempfile::tempdir().unwrap();
    // Wrong size.
    let small = dir.path().join("small.exe");
    fs::write(&small, b"MZ not the installer").unwrap();
    let err = verify_installer(&small).unwrap_err();
    assert!(err.contains("size"), "{err}");
    assert!(!small.exists(), "wrong-size file deleted");
    // Right size, wrong bytes.
    let same = dir.path().join("same.exe");
    fs::write(&same, vec![0u8; INSTALLER_SIZE as usize]).unwrap();
    let err = verify_installer(&same).unwrap_err();
    assert!(err.contains("sha256"), "{err}");
    assert!(!same.exists(), "wrong-hash file deleted");
    // Missing file.
    assert!(verify_installer(&dir.path().join("none.exe")).is_err());
}

#[cfg(not(windows))]
#[test]
fn status_unsupported_off_windows() {
    assert_eq!(dualsense_mapper::vigem_setup::status(), "unsupported");
    assert!(dualsense_mapper::vigem_setup::install().is_err());
}

/// Downloads the real installer once and checks the pin. Network; run by
/// hand: `cargo test --test vigem_setup -- --ignored`.
#[test]
#[ignore]
fn installer_accepts_pinned_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ViGEmBus.exe");
    let ok = std::process::Command::new("curl")
        .args(["-sSfL", "-o"])
        .arg(&path)
        .arg(INSTALLER_URL)
        .status()
        .expect("curl")
        .success();
    assert!(ok, "download failed");
    verify_installer(&path).expect("pinned installer verifies");
    assert!(path.exists());
}
