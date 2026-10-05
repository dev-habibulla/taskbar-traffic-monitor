//! Registers the application to run at login via the per-user Run key.
//!
//! Everything lives under `HKEY_CURRENT_USER`, so no administrator rights are
//! required.

use std::ffi::c_void;
use std::path::PathBuf;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};

use crate::util::wide;

/// The per-user autostart key.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The value name used inside the Run key.
pub const VALUE_NAME: &str = "TaskbarMonitor";

/// Full path of the running executable.
pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

/// The command stored in the registry (quoted so paths with spaces work).
pub fn exe_command() -> String {
    format!("\"{}\"", exe_path().display())
}

/// True when this application is registered to start with Windows.
pub fn is_enabled() -> bool {
    is_enabled_for(RUN_KEY, VALUE_NAME)
}

/// Adds or removes the autostart entry.
pub fn set_enabled(enabled: bool) -> Result<(), String> {
    let command = if enabled {
        exe_command()
    } else {
        String::new()
    };
    set_enabled_for(RUN_KEY, VALUE_NAME, &command)
}

/// Query helper with an explicit key/value (used by tests).
pub fn is_enabled_for(subkey: &str, name: &str) -> bool {
    let subkey_w = wide(subkey);
    let name_w = wide(name);
    let mut buffer = [0u16; 1024];
    let mut size = std::mem::size_of_val(&buffer) as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey_w.as_ptr()),
            windows::core::PCWSTR(name_w.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut c_void),
            Some(&mut size),
        )
    };
    result.0 == 0
}

/// Write helper with an explicit key/value/command (used by tests).
///
/// An empty command removes the value.
pub fn set_enabled_for(subkey: &str, name: &str, command: &str) -> Result<(), String> {
    let subkey_w = wide(subkey);
    let name_w = wide(name);
    unsafe {
        if command.is_empty() {
            let mut key = HKEY::default();
            let open = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                windows::core::PCWSTR(subkey_w.as_ptr()),
                None,
                KEY_SET_VALUE,
                &mut key,
            );
            if open.0 != 0 {
                return Ok(()); // Key absent: nothing to remove.
            }
            let delete = RegDeleteValueW(key, windows::core::PCWSTR(name_w.as_ptr()));
            let _ = RegCloseKey(key);
            if delete.0 != 0 && delete.0 != 2 {
                return Err(format!("RegDeleteValueW failed ({})", delete.0));
            }
            return Ok(());
        }

        let mut key = HKEY::default();
        let create = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            windows::core::PCWSTR(subkey_w.as_ptr()),
            None,
            windows::core::PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if create.0 != 0 {
            return Err(format!("RegCreateKeyExW failed ({})", create.0));
        }

        let mut data: Vec<u8> = Vec::new();
        for unit in wide(command) {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        let set = RegSetValueExW(
            key,
            windows::core::PCWSTR(name_w.as_ptr()),
            None,
            REG_SZ,
            Some(&data),
        );
        let _ = RegCloseKey(key);
        if set.0 != 0 {
            return Err(format!("RegSetValueExW failed ({})", set.0));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses a sandbox key (never the real Run key) so tests never add an
    // autostart entry to the machine running them.
    const TEST_KEY: &str = r"Software\TaskbarMonitor\StartupTest";

    fn test_value() -> String {
        format!("TestValue{}", std::process::id())
    }

    #[test]
    fn enable_disable_round_trip() {
        let name = test_value();
        assert!(!is_enabled_for(TEST_KEY, &name));

        set_enabled_for(TEST_KEY, &name, "\"C:/fake/app.exe\"").unwrap();
        assert!(is_enabled_for(TEST_KEY, &name));

        set_enabled_for(TEST_KEY, &name, "").unwrap();
        assert!(!is_enabled_for(TEST_KEY, &name));
    }

    #[test]
    fn disabling_a_missing_value_is_ok() {
        let name = format!("Missing{}", std::process::id());
        assert_eq!(set_enabled_for(TEST_KEY, &name, ""), Ok(()));
    }

    #[test]
    fn exe_command_is_quoted() {
        let command = exe_command();
        assert!(command.starts_with('"') && command.ends_with('"'));
    }
}
