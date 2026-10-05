//! Application controller.
//!
//! Owns the application lifetime, the tray icon, the monitor worker and the
//! overlay window. Crucially, the overlay is treated as a **disposable render
//! surface** driven by a stable, hidden controller window:
//!
//! * the worker and the settings UI post to the controller, never to the overlay;
//! * the controller recreates the overlay if Windows destroys it;
//! * a light health timer verifies the overlay is still showable and re-anchors
//!   it, and `TaskbarCreated` recovers from Explorer/taskbar recreation.
//!
//! Because the overlay window owns no application state, destroying it never
//! takes down the tray, the worker, or the process.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GWLP_USERDATA, GetMessageW,
    GetWindowLongPtrW, IDC_ARROW, LoadCursorW, MSG, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetTimer, SetWindowLongPtrW, TranslateMessage, WM_DEVICECHANGE,
    WM_DISPLAYCHANGE, WM_DPICHANGED, WM_POWERBROADCAST, WM_SETTINGCHANGE, WM_THEMECHANGED,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::Shared;
use crate::monitor;
use crate::settings::{self, Settings};
use crate::startup;
use crate::taskbar::display::{Overlay, WM_APP_SETTINGS, WM_APP_UPDATE};
use crate::tray::{self, Tray, TrayCommand};
use crate::ui;
use crate::util::{dlog, wide};

/// Posted by the `Exit` command to end the message loop.
const WM_APP_QUIT: u32 = 0x8010;
/// Health-check timer: verifies liveness/visibility and re-anchors the overlay.
const TIMER_HEALTH: usize = 1;
const HEALTH_MS: u32 = 2000;
const CONTROLLER_CLASS: &str = "TbMonController";
const SHOW_SETTINGS_MESSAGE: &str = "TbMonShowSettings";

static CONTROLLER_HWND: AtomicIsize = AtomicIsize::new(0);

/// Asks the controller to act. Safe to call from any thread.
pub fn notify(message: u32) {
    let raw = CONTROLLER_HWND.load(Ordering::Relaxed);
    if raw != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(raw as *mut _)), message, WPARAM(0), LPARAM(0));
        }
    }
}

/// The registered message a second instance broadcasts to ask the running
/// instance to open its settings window.
pub fn show_settings_message() -> u32 {
    register_message(SHOW_SETTINGS_MESSAGE)
}

pub(crate) struct Controller {
    hwnd: HWND,
    shared: Shared,
    overlay: Option<Overlay>,
    tray: Option<Tray>,
    paused: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    taskbar_created: u32,
    show_settings: u32,
}

/// Creates the controller window, tray and worker, and returns its raw pointer.
///
/// The message loop (`run`) and the window procedure both operate on this
/// pointer on the UI thread.
pub(crate) fn start(shared: Shared, settings: Settings) -> Option<*mut Controller> {
    unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(_) => {
                dlog("GetModuleHandleW failed");
                return None;
            }
        };

        let class_name = wide(CONTROLLER_CLASS);
        let class = WNDCLASSW {
            lpfnWndProc: Some(controller_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&class);

        let hwnd = match CreateWindowExW(
            WS_EX_TOOLWINDOW,
            PCWSTR(class_name.as_ptr()),
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        ) {
            Ok(hwnd) => hwnd,
            Err(_) => {
                dlog("controller window creation failed");
                return None;
            }
        };

        let paused = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let tray = match Tray::create(&settings) {
            Ok(tray) => Some(tray),
            Err(error) => {
                dlog(&format!("tray unavailable: {error}"));
                None
            }
        };

        let controller = Box::new(Controller {
            hwnd,
            shared: shared.clone(),
            overlay: None,
            tray,
            paused: paused.clone(),
            shutdown: shutdown.clone(),
            worker: spawn_monitor(shared, paused, shutdown),
            taskbar_created: register_message("TaskbarCreated"),
            show_settings: register_message(SHOW_SETTINGS_MESSAGE),
        });

        let ptr = Box::into_raw(controller);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        CONTROLLER_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = SetTimer(Some(hwnd), TIMER_HEALTH, HEALTH_MS, None);

        // Bring the overlay up immediately.
        (*ptr).render();
        Some(ptr)
    }
}

/// Runs the UI message loop until the application exits.
pub(crate) unsafe fn run(ptr: *mut Controller) {
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }

        if ptr.is_null() {
            continue;
        }
        // Safe: the window procedure has returned, so no borrow is outstanding.
        let controller = unsafe { &mut *ptr };
        for command in tray::drain_menu_events() {
            controller.handle_command(command);
        }
        if tray::drain_left_clicks() > 0 {
            ui::settings::show(controller.shared.clone());
        }
        if let Some(tray) = controller.tray.as_ref()
            && let Ok(guard) = controller.shared.lock()
        {
            tray.sync(&guard.settings, guard.paused);
        }
    }
}

impl Controller {
    /// Recreates the overlay if it was destroyed or never created.
    fn ensure_overlay(&mut self) {
        if self
            .overlay
            .as_ref()
            .is_none_or(|overlay| !overlay.is_alive())
        {
            if self.overlay.is_some() {
                dlog("overlay window lost; recreating");
            }
            self.overlay = Overlay::create(self.shared.clone());
            if self.overlay.is_none() {
                dlog("overlay creation failed");
            }
        }
    }

    /// Renders the overlay, creating it first if needed.
    fn render(&mut self) {
        self.ensure_overlay();
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.render();
        }
    }

    /// Periodic self-heal: keep the overlay alive and visible when the user has
    /// not hidden it, and keep it hidden when they have.
    fn health(&mut self) {
        let want_visible = self
            .shared
            .lock()
            .map(|guard| guard.overlay_visible)
            .unwrap_or(true);
        if want_visible {
            self.ensure_overlay();
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.ensure_shown();
                overlay.render();
            }
        } else if let Some(overlay) = self.overlay.as_mut() {
            overlay.hide();
        }
    }

    /// Handles a display/DPI/theme/power/topology change: rebuild the surface
    /// (it may be invalid) and re-anchor.
    fn on_system_change(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            overlay.invalidate_surface();
        }
        self.render();
    }

    fn handle_command(&mut self, command: TrayCommand) {
        match command {
            TrayCommand::Show => {
                if let Ok(mut guard) = self.shared.lock() {
                    guard.overlay_visible = true;
                }
                self.render();
            }
            TrayCommand::Hide => {
                if let Ok(mut guard) = self.shared.lock() {
                    guard.overlay_visible = false;
                }
                if let Some(overlay) = self.overlay.as_mut() {
                    overlay.hide();
                }
            }
            TrayCommand::Settings => ui::settings::show(self.shared.clone()),
            TrayCommand::TogglePause => {
                if let Ok(mut guard) = self.shared.lock() {
                    guard.paused = !guard.paused;
                    self.paused.store(guard.paused, Ordering::Relaxed);
                }
            }
            TrayCommand::ToggleStartup => {
                let enabled = {
                    let mut guard = match self.shared.lock() {
                        Ok(guard) => guard,
                        Err(_) => return,
                    };
                    guard.settings.start_with_windows = !guard.settings.start_with_windows;
                    let settings = guard.settings.clone();
                    drop(guard);
                    let _ = settings::storage::save(&settings);
                    settings.start_with_windows
                };
                let _ = startup::windows::set_enabled(enabled);
                self.on_system_change();
            }
            TrayCommand::Exit => {
                if let Ok(guard) = self.shared.lock() {
                    let _ = settings::storage::save(&guard.settings);
                }
                self.shutdown.store(true, Ordering::Relaxed);
                unsafe {
                    PostQuitMessage(0);
                }
            }
        }
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe {
            // Detach before destroying so WM_DESTROY cannot dereference `self`.
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
        CONTROLLER_HWND.store(0, Ordering::Relaxed);
    }
}

fn register_message(name: &str) -> u32 {
    let wide_name = wide(name);
    unsafe { RegisterWindowMessageW(PCWSTR(wide_name.as_ptr())) }
}

fn spawn_monitor(
    shared: Shared,
    paused: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) -> Option<JoinHandle<()>> {
    let result = std::thread::Builder::new()
        .name("tbmon-sampler".to_string())
        .spawn(move || {
            let mut monitor = monitor::Monitor::new();
            while !shutdown.load(Ordering::Relaxed) {
                let interval = shared
                    .lock()
                    .map(|guard| guard.settings.update_interval_ms)
                    .unwrap_or(1000);
                if !paused.load(Ordering::Relaxed) {
                    let metrics = monitor.sample();
                    if let Ok(mut guard) = shared.lock() {
                        guard.metrics = metrics;
                    }
                    notify(WM_APP_UPDATE);
                }
                sleep_interruptible(&shutdown, interval);
            }
        });
    match result {
        Ok(handle) => Some(handle),
        Err(error) => {
            dlog(&format!("monitor thread spawn failed: {error}"));
            None
        }
    }
}

/// Sleeps in small slices so shutdown is picked up promptly.
fn sleep_interruptible(shutdown: &AtomicBool, total_ms: u32) {
    let mut slept = 0;
    while slept < total_ms {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        slept += 50;
    }
}

unsafe extern "system" fn controller_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Controller;
        if !ptr.is_null() {
            let controller = &mut *ptr;
            match msg {
                WM_APP_UPDATE | WM_APP_SETTINGS => {
                    controller.render();
                    return LRESULT(0);
                }
                WM_APP_QUIT => {
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                WM_TIMER => {
                    controller.health();
                    return LRESULT(0);
                }
                WM_DISPLAYCHANGE | WM_DPICHANGED | WM_SETTINGCHANGE | WM_THEMECHANGED
                | WM_DEVICECHANGE | WM_POWERBROADCAST => {
                    controller.on_system_change();
                    return LRESULT(0);
                }
                _ => {
                    if controller.taskbar_created != 0 && msg == controller.taskbar_created {
                        dlog("TaskbarCreated received; re-anchoring overlay");
                        controller.on_system_change();
                        return LRESULT(0);
                    }
                    if controller.show_settings != 0 && msg == controller.show_settings {
                        ui::settings::show(controller.shared.clone());
                        return LRESULT(0);
                    }
                }
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}
