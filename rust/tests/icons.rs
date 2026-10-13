//! The committed ICOs: app icon multi-size, tray icons one 32 px entry each
//! (tauri's Image::from_bytes keeps only the largest ICO entry, so a bigger
//! entry would be the one Windows shrinks into the tray).

fn entries(name: &str) -> Vec<u32> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons").join(name);
    let b = std::fs::read(&p).unwrap();
    assert_eq!(&b[0..4], &[0, 0, 1, 0], "{name}: ICONDIR");
    let n = u16::from_le_bytes([b[4], b[5]]) as usize;
    (0..n)
        .map(|i| {
            let e = 6 + 16 * i;
            let size = if b[e] == 0 { 256 } else { b[e] as u32 };
            let off = u32::from_le_bytes(b[e + 12..e + 16].try_into().unwrap()) as usize;
            assert_eq!(&b[off..off + 8], b"\x89PNG\r\n\x1a\n", "{name}: entry {size} is PNG");
            size
        })
        .collect()
}

#[test]
fn app_icon_has_every_size() {
    assert_eq!(entries("icon.ico"), vec![16, 24, 32, 48, 256]);
}

#[test]
fn tray_icons_are_one_32px_entry() {
    assert_eq!(entries("tray-connected.ico"), vec![32]);
    assert_eq!(entries("tray-disconnected.ico"), vec![32]);
}
