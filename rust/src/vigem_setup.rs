//! ViGEmBus for the Switch 2 Pro's Xbox output (v3.2.0): a probe, and a
//! one-click install of the **pinned official installer**.
//!
//! The app never ships a driver (iron rule 7). On the user's click it
//! downloads the official ViGEmBus v1.22.0 installer from GitHub into
//! `%TEMP%`, checks its size and sha256 against the pin below, and only then
//! runs it; the installer asks for elevation itself. A file that fails the
//! check is deleted and nothing runs. Windows only; macOS is Phase 2 TBD as
//! for the whole Switch 2 Pro path.

use sha2::{Digest, Sha256};
use std::path::Path;

pub const INSTALLER_URL: &str =
    "https://github.com/nefarius/ViGEmBus/releases/download/v1.22.0/ViGEmBus_1.22.0_x64_x86_arm64.exe";
pub const INSTALLER_SIZE: u64 = 6_278_576;
pub const INSTALLER_SHA256: &str = "89220a7865076b342892f98865f3499fb7c4cfd673159e89d352c360fd014c6a";
const INSTALLER_NAME: &str = "ViGEmBus_1.22.0_x64_x86_arm64.exe";

/// Check `path` against the pin (size first, then sha256). On a mismatch the
/// file is deleted and the reason returned.
pub fn verify_installer(path: &Path) -> Result<(), String> {
    let reject = |why: String| {
        let _ = std::fs::remove_file(path);
        Err(why)
    };
    let bytes = std::fs::read(path).map_err(|e| format!("installer not readable: {e}"))?;
    if bytes.len() as u64 != INSTALLER_SIZE {
        return reject(format!("installer size {} ≠ {INSTALLER_SIZE}", bytes.len()));
    }
    let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if hex != INSTALLER_SHA256 {
        return reject(format!("installer sha256 {hex} ≠ pinned"));
    }
    Ok(())
}

/// `"installed"` (the bus answers), `"missing"`, or `"unsupported"` (not
/// Windows). Connects and disconnects; plugs nothing.
pub fn status() -> &'static str {
    imp::status()
}

/// Download, verify and run the official installer; returns when the
/// installer process exits. Blocking: call it off the UI thread.
pub fn install() -> Result<(), String> {
    imp::install()
}

#[cfg(not(windows))]
mod imp {
    pub fn status() -> &'static str {
        "unsupported"
    }
    pub fn install() -> Result<(), String> {
        Err("ViGEmBus is Windows only".into())
    }
}

#[cfg(windows)]
mod imp {
    use super::{verify_installer, INSTALLER_NAME, INSTALLER_URL};
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Com::Urlmon::URLDownloadToFileW;
    use windows_sys::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn status() -> &'static str {
        match vigem_client::Client::connect() {
            Ok(_) => "installed",
            Err(_) => "missing",
        }
    }

    pub fn install() -> Result<(), String> {
        let path = std::env::temp_dir().join(INSTALLER_NAME);
        let _ = std::fs::remove_file(&path);
        let url = wide(std::ffi::OsStr::new(INSTALLER_URL));
        let file = wide(path.as_os_str());
        // SAFETY: plain Win32 calls with NUL-terminated wide strings that
        // outlive them; COM is initialised for this thread and released.
        let hr = unsafe {
            let co = CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32);
            let hr = URLDownloadToFileW(std::ptr::null_mut(), url.as_ptr(), file.as_ptr(), 0, std::ptr::null_mut());
            if co >= 0 {
                CoUninitialize();
            }
            hr
        };
        if hr < 0 {
            let _ = std::fs::remove_file(&path);
            return Err(format!("下載失敗（0x{:08X}）", hr as u32));
        }
        verify_installer(&path)?;
        tracing::info!("running the verified ViGEmBus installer {}", path.display());
        // SAFETY: SHELLEXECUTEINFOW is zero-initialisable; the strings live
        // until the call returns; the process handle is closed below.
        unsafe {
            let open = wide(std::ffi::OsStr::new("open"));
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS;
            info.lpVerb = open.as_ptr();
            info.lpFile = file.as_ptr();
            info.nShow = SW_SHOWNORMAL;
            if ShellExecuteExW(&mut info) == 0 {
                return Err(format!("無法啟動安裝程式：{}", std::io::Error::last_os_error()));
            }
            if !info.hProcess.is_null() {
                WaitForSingleObject(info.hProcess, INFINITE);
                let mut code = 0u32;
                GetExitCodeProcess(info.hProcess, &mut code);
                CloseHandle(info.hProcess);
                tracing::info!(code, "ViGEmBus installer exited");
            }
        }
        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}
