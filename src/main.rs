#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Taskbar Monitor — a minimal, native Windows 11 taskbar system monitor.
//!
//! A transparent, click-through overlay draws two lines over the taskbar
//! notification area; a tray icon and a small native settings window control
//! what is shown. Everything is pure Rust on top of the Windows API.
//!
//! This file only bootstraps the process; [`app`] owns the runtime so that the
//! overlay window can be recreated without affecting the tray or the worker.

mod app;
mod format;
mod monitor;
mod settings;
mod startup;
mod taskbar;
mod tray;
mod ui;
mod util;

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, LPARAM, WPARAM};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, PostMessageW};
use windows::core::PCWSTR;

use crate::monitor::Metrics;
use crate::settings::Settings;
use crate::util::{dlog, wide};

/// State shared between the UI thread, the monitor thread and the menu handlers.
pub struct SharedState {
    pub settings: Settings,
    pub metrics: Metrics,
    pub paused: bool,
    pub overlay_visible: bool,
}

pub type Shared = Arc<Mutex<SharedState>>;

/// Per-user mutex name used to enforce a single running instance.
const INSTANCE_MUTEX: &str = "TaskbarMonitor.SingleInstance.v1";

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    // Prevent duplicate overlays, tray icons and monitor workers.
    let _instance = match acquire_single_instance() {
        Instance::First(handle) => handle,
        Instance::Duplicate => {
            dlog("another instance is already running; signalling it and exiting");
            signal_existing_instance();
            return;
        }
    };

    let settings = {
        let mut settings = settings::storage::load();
        settings.normalize();
        settings
    };

    // Keep the autostart entry consistent with the stored preference.
    if settings.start_with_windows {
        // Always rewrite so the stored command tracks the current executable path.
        if let Err(error) = startup::windows::set_enabled(true) {
            dlog(&format!("startup registration failed: {error}"));
        }
    } else if startup::windows::is_enabled() {
        let _ = startup::windows::set_enabled(false);
    }

    let shared: Shared = Arc::new(Mutex::new(SharedState {
        settings: settings.clone(),
        metrics: Metrics::default(),
        paused: false,
        overlay_visible: true,
    }));

    let controller = match app::start(shared.clone(), settings.clone()) {
        Some(controller) => controller,
        None => {
            dlog("failed to start application controller");
            return;
        }
    };

    if !settings.start_minimized {
        ui::settings::show(shared.clone());
    }

    unsafe { app::run(controller) };

    // Reclaim the controller: joins the worker and drops the tray/overlay cleanly.
    unsafe { drop(Box::from_raw(controller)) };

    unsafe { CoUninitialize() };
    std::process::exit(0);
}

enum Instance {
    First(HANDLE),
    Duplicate,
}

/// Creates (or opens) the single-instance mutex.
///
/// `ERROR_ALREADY_EXISTS` after a successful `CreateMutexW` means another
/// instance holds it. The handle is intentionally never closed, so the mutex
/// stays owned for the lifetime of the process.
fn acquire_single_instance() -> Instance {
    let name = wide(INSTANCE_MUTEX);
    unsafe {
        match CreateMutexW(None, false, PCWSTR(name.as_ptr())) {
            Ok(handle) => {
                if is_duplicate_error(GetLastError().0) {
                    Instance::Duplicate
                } else {
                    Instance::First(handle)
                }
            }
            // If the mutex cannot be created, fail open rather than block startup.
            Err(_) => Instance::First(HANDLE(std::ptr::null_mut())),
        }
    }
}

/// True when a successful `CreateMutexW` reported that the mutex already exists.
fn is_duplicate_error(last_error: u32) -> bool {
    last_error == ERROR_ALREADY_EXISTS.0
}

/// Asks the already-running instance to open its settings window.
fn signal_existing_instance() {
    let message = app::show_settings_message();
    if message == 0 {
        return;
    }
    unsafe {
        let _ = PostMessageW(Some(HWND_BROADCAST), message, WPARAM(0), LPARAM(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_instance_error_is_detected() {
        assert!(is_duplicate_error(ERROR_ALREADY_EXISTS.0));
        assert!(!is_duplicate_error(0));
    }
}
