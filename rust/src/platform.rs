//! Platform-specific controller power control.
//!
//! The DualSense HID protocol has **no** power-off / shutdown / disconnect
//! command. Verified against the nondebug/dualsense reverse-engineering
//! reference: every output report (0x02/0x05/0x31/0xa0/…) only drives
//! haptics, adaptive triggers, the light bar, the speaker, and the player
//! LEDs. The PS5 console powers a pad off at the **Bluetooth link layer**
//! (it terminates the ACL link), not via HID — the pad, having lost its
//! bonded host, then powers itself down.
//!
//! So "turn off the controller" from a PC means dropping the Bluetooth
//! bond through the OS Bluetooth stack. On Windows that is
//! `BluetoothRemoveDevice`, which unpairs the device; the DualSense reacts
//! by powering off. The trade-off the user accepted is that the pad must
//! be re-paired before it can be used again.
//!
//! This is user-mode Bluetooth *device management* — not input synthesis,
//! not a driver, not process hooking. It stays within iron rule #7.

/// Power off every currently-connected DualSense by dropping its Bluetooth
/// pairing. Returns the number of controllers removed.
///
/// USB note: this app only ever streams the Bluetooth 0x31 report, so a
/// connected pad is on Bluetooth by construction. A USB-only pad cannot be
/// powered off in software (it is bus-powered) and simply won't match.
#[cfg(target_os = "windows")]
pub fn power_off_connected_dualsense() -> anyhow::Result<usize> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Devices::Bluetooth::{
        BluetoothFindDeviceClose, BluetoothFindFirstDevice, BluetoothFindNextDevice,
        BluetoothRemoveDevice, BLUETOOTH_DEVICE_INFO, BLUETOOTH_DEVICE_SEARCH_PARAMS,
    };
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;

    // SAFETY: every struct is zero-initialised then has its `dwSize` set
    // before the FFI call, exactly as the Win32 Bluetooth API requires.
    unsafe {
        let mut params: BLUETOOTH_DEVICE_SEARCH_PARAMS = zeroed();
        params.dwSize = size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32;
        params.fReturnAuthenticated = 1; // paired
        params.fReturnRemembered = 1; // previously paired
        params.fReturnConnected = 1; // currently linked
        params.fReturnUnknown = 0;
        params.fIssueInquiry = 0; // don't scan the air — only known devices
        params.cTimeoutMultiplier = 0;
        params.hRadio = std::ptr::null_mut(); // search across all radios

        let mut info: BLUETOOTH_DEVICE_INFO = zeroed();
        info.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;

        let find = BluetoothFindFirstDevice(&params, &mut info);
        if find.is_null() {
            // No remembered/connected Bluetooth devices at all.
            return Ok(0);
        }

        let mut removed = 0usize;
        loop {
            if info.fConnected != 0 && is_dualsense_name(&decode_name(&info.szName)) {
                let rc = BluetoothRemoveDevice(&info.Address);
                if rc == ERROR_SUCCESS {
                    removed += 1;
                    tracing::info!("removed Bluetooth pairing for connected DualSense");
                } else {
                    tracing::warn!(rc, "BluetoothRemoveDevice failed");
                }
            }

            info = zeroed();
            info.dwSize = size_of::<BLUETOOTH_DEVICE_INFO>() as u32;
            if BluetoothFindNextDevice(find, &mut info) == 0 {
                break;
            }
        }

        BluetoothFindDeviceClose(find);
        Ok(removed)
    }
}

/// macOS: not yet wired up. Phase 2 will route this through IOBluetooth
/// (`IOBluetoothDevice closeConnection`). Until then, fail loudly so the
/// GUI can surface "not supported yet" rather than silently no-op.
#[cfg(target_os = "macos")]
pub fn power_off_connected_dualsense() -> anyhow::Result<usize> {
    anyhow::bail!("controller power-off is not yet implemented on macOS (Phase 2 TBD)")
}

/// Other platforms (the Linux dev host): no Bluetooth management path.
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
pub fn power_off_connected_dualsense() -> anyhow::Result<usize> {
    anyhow::bail!("controller power-off is only supported on Windows")
}

/// Decode a Win32 `szName` (UTF-16, NUL-terminated, fixed-size buffer) into
/// a Rust `String`, stopping at the first NUL.
#[cfg(target_os = "windows")]
fn decode_name(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// True when a Bluetooth device name belongs to a DualSense pad. Windows
/// reports the DualSense as "DualSense Wireless Controller"; some stacks
/// shorten it to "Wireless Controller". Matched case-insensitively so a
/// stack that title-cases differently still hits.
///
/// Kept as a free function so it is unit-testable without any Bluetooth
/// hardware or Win32 calls.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_dualsense_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("dualsense") || n.contains("wireless controller")
}

#[cfg(test)]
mod tests {
    use super::is_dualsense_name;

    #[test]
    fn matches_known_dualsense_names() {
        assert!(is_dualsense_name("DualSense Wireless Controller"));
        assert!(is_dualsense_name("Wireless Controller"));
        assert!(is_dualsense_name("dualsense edge"));
        assert!(is_dualsense_name("WIRELESS CONTROLLER"));
    }

    #[test]
    fn rejects_unrelated_names() {
        assert!(!is_dualsense_name("My Headphones"));
        assert!(!is_dualsense_name("Xbox Wireless"));
        assert!(!is_dualsense_name(""));
        assert!(!is_dualsense_name("Logitech Mouse"));
    }
}
