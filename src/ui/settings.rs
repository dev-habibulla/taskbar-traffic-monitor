//! A small, DPI-aware Win32 dialog for editing the settings.
//!
//! Changes apply and persist immediately; closing the window only closes the
//! window — the application keeps running in the background.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_WINDOW, CreateFontIndirectW, DeleteObject, GetSysColorBrush, HFONT, HGDIOBJ, LOGFONTW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_PUSHBUTTON, CREATESTRUCTW, CW_USEDEFAULT,
    CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, ES_NUMBER, GWLP_USERDATA,
    GetWindowLongPtrW, GetWindowTextW, HMENU, IDC_ARROW, LoadCursorW, NONCLIENTMETRICSW,
    RegisterClassW, SPI_GETNONCLIENTMETRICS, SW_RESTORE, SW_SHOW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowTextW, ShowWindow, SystemParametersInfoW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_SETFONT,
    WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_GROUP, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};
use windows::core::PCWSTR;

use crate::Shared;
use crate::app;
use crate::settings::{Settings, ThemeMode};
use crate::taskbar::display::WM_APP_SETTINGS;
use crate::util::wide;

const WINDOW_CLASS: &str = "TbMonSettings";

const ID_UPLOAD: u16 = 1001;
const ID_DOWNLOAD: u16 = 1002;
const ID_CPU: u16 = 1003;
const ID_MEMORY: u16 = 1004;
const ID_TOTAL: u16 = 1005;
const ID_INTERVAL: u16 = 1010;
const ID_STARTUP: u16 = 1020;
const ID_MINIMIZED: u16 = 1021;
const ID_THEME_SYSTEM: u16 = 1030;
const ID_THEME_LIGHT: u16 = 1031;
const ID_THEME_DARK: u16 = 1032;
const ID_APPLY: u16 = 1040;
const ID_CLOSE: u16 = 1041;

static SETTINGS_HWND: AtomicIsize = AtomicIsize::new(0);

struct SettingsUi {
    shared: Shared,
    font: HFONT,
    c_upload: HWND,
    c_download: HWND,
    c_cpu: HWND,
    c_memory: HWND,
    c_total: HWND,
    e_interval: HWND,
    c_startup: HWND,
    c_minimized: HWND,
    r_system: HWND,
    r_light: HWND,
    r_dark: HWND,
}

/// Shows the settings window, focusing an existing one if already open.
pub fn show(shared: Shared) {
    let existing = SETTINGS_HWND.load(Ordering::Relaxed);
    if existing != 0 {
        unsafe {
            let hwnd = HWND(existing as *mut c_void);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
        return;
    }
    unsafe {
        create_window(shared);
    }
}

unsafe fn create_window(shared: Shared) {
    unsafe {
        let instance = match GetModuleHandleW(None) {
            Ok(instance) => instance,
            Err(_) => return,
        };
        let dpi = {
            let dpi = GetDpiForSystem();
            if dpi == 0 { 96 } else { dpi }
        };

        let class_name = wide(WINDOW_CLASS);
        let class = WNDCLASSW {
            lpfnWndProc: Some(settings_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&class);

        let title = wide("Taskbar Monitor Settings");
        let shared_ptr = Box::into_raw(Box::new(shared));
        let result = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            scale(380, dpi),
            scale(520, dpi),
            None,
            None,
            Some(instance.into()),
            Some(shared_ptr as *const c_void),
        );
        match result {
            Ok(hwnd) => {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = SetForegroundWindow(hwnd);
            }
            Err(_) => {
                let _ = Box::from_raw(shared_ptr);
            }
        }
    }
}

fn scale(value: i32, dpi: u32) -> i32 {
    (value * dpi as i32 + 48) / 96
}

unsafe fn create_controls(
    parent: HWND,
    instance: windows::Win32::Foundation::HINSTANCE,
    shared: &Shared,
    dpi: u32,
) -> SettingsUi {
    unsafe {
        let settings = shared
            .lock()
            .map(|g| g.settings.clone())
            .unwrap_or_default();

        let font = message_font(dpi);
        let margin = scale(16, dpi);
        let step = scale(26, dpi);
        let gap = scale(14, dpi);
        let mut y = margin;
        let x = margin;

        let label = |text: &str, y: i32| -> HWND {
            add_control(
                parent,
                "STATIC",
                text,
                WS_CHILD.0 | WS_VISIBLE.0,
                0,
                x,
                y,
                scale(300, dpi),
                scale(18, dpi),
                instance,
                font,
            )
        };
        let checkbox = |text: &str, id: u16, y: i32| -> HWND {
            add_control(
                parent,
                "BUTTON",
                text,
                WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTOCHECKBOX as u32,
                id,
                x,
                y,
                scale(300, dpi),
                scale(20, dpi),
                instance,
                font,
            )
        };
        let radio = |text: &str, id: u16, y: i32, group: bool| -> HWND {
            let mut style = WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTORADIOBUTTON as u32;
            if group {
                style |= WS_GROUP.0;
            }
            add_control(
                parent,
                "BUTTON",
                text,
                style,
                id,
                x,
                y,
                scale(300, dpi),
                scale(20, dpi),
                instance,
                font,
            )
        };

        label("Display", y);
        y += step;
        let c_upload = checkbox("Upload Speed", ID_UPLOAD, y);
        y += step;
        let c_download = checkbox("Download Speed", ID_DOWNLOAD, y);
        y += step;
        let c_cpu = checkbox("CPU", ID_CPU, y);
        y += step;
        let c_memory = checkbox("Memory", ID_MEMORY, y);
        y += step;
        let c_total = checkbox("Total Traffic", ID_TOTAL, y);
        y += step + gap;

        label("Monitoring", y);
        y += step;
        label("Update Interval (ms)", y);
        let edit_style = WS_CHILD.0
            | WS_VISIBLE.0
            | WS_TABSTOP.0
            | WS_BORDER.0
            | ES_NUMBER as u32
            | ES_AUTOHSCROLL as u32;
        let e_interval = add_control(
            parent,
            "EDIT",
            "",
            edit_style,
            ID_INTERVAL,
            margin + scale(170, dpi),
            y,
            scale(90, dpi),
            scale(22, dpi),
            instance,
            font,
        );
        y += step + gap;

        label("Startup", y);
        y += step;
        let c_startup = checkbox("Start with Windows", ID_STARTUP, y);
        y += step;
        let c_minimized = checkbox("Start minimized", ID_MINIMIZED, y);
        y += step + gap;

        label("Appearance", y);
        y += step;
        label("Theme", y);
        let r_system = radio("System", ID_THEME_SYSTEM, y, true);
        let theme_x = margin + scale(90, dpi);
        let r_light = add_control(
            parent,
            "BUTTON",
            "Light",
            WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTORADIOBUTTON as u32,
            ID_THEME_LIGHT,
            theme_x,
            y,
            scale(70, dpi),
            scale(20, dpi),
            instance,
            font,
        );
        let r_dark = add_control(
            parent,
            "BUTTON",
            "Dark",
            WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTORADIOBUTTON as u32,
            ID_THEME_DARK,
            theme_x + scale(75, dpi),
            y,
            scale(70, dpi),
            scale(20, dpi),
            instance,
            font,
        );
        y += step + gap;

        add_control(
            parent,
            "BUTTON",
            "Apply",
            WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_PUSHBUTTON as u32,
            ID_APPLY,
            x,
            y,
            scale(90, dpi),
            scale(26, dpi),
            instance,
            font,
        );
        add_control(
            parent,
            "BUTTON",
            "Close",
            WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_PUSHBUTTON as u32,
            ID_CLOSE,
            x + scale(100, dpi),
            y,
            scale(90, dpi),
            scale(26, dpi),
            instance,
            font,
        );

        let ui = SettingsUi {
            shared: shared.clone(),
            font,
            c_upload,
            c_download,
            c_cpu,
            c_memory,
            c_total,
            e_interval,
            c_startup,
            c_minimized,
            r_system,
            r_light,
            r_dark,
        };
        load_values(&ui, &settings);
        ui
    }
}

unsafe fn load_values(ui: &SettingsUi, settings: &Settings) {
    unsafe {
        set_checked(ui.c_upload, settings.show_upload);
        set_checked(ui.c_download, settings.show_download);
        set_checked(ui.c_cpu, settings.show_cpu);
        set_checked(ui.c_memory, settings.show_memory);
        set_checked(ui.c_total, settings.show_total_traffic);
        set_checked(ui.c_startup, settings.start_with_windows);
        set_checked(ui.c_minimized, settings.start_minimized);
        set_checked(ui.r_system, settings.theme == ThemeMode::System);
        set_checked(ui.r_light, settings.theme == ThemeMode::Light);
        set_checked(ui.r_dark, settings.theme == ThemeMode::Dark);

        let interval = wide(&settings.update_interval_ms.to_string());
        let _ = SetWindowTextW(ui.e_interval, PCWSTR(interval.as_ptr()));
    }
}

unsafe fn read_values(ui: &SettingsUi) -> Settings {
    unsafe {
        let mut settings = ui
            .shared
            .lock()
            .map(|g| g.settings.clone())
            .unwrap_or_default();
        settings.show_upload = is_checked(ui.c_upload);
        settings.show_download = is_checked(ui.c_download);
        settings.show_cpu = is_checked(ui.c_cpu);
        settings.show_memory = is_checked(ui.c_memory);
        settings.show_total_traffic = is_checked(ui.c_total);
        settings.start_with_windows = is_checked(ui.c_startup);
        settings.start_minimized = is_checked(ui.c_minimized);

        if let Some(interval) = read_interval(ui.e_interval) {
            settings.update_interval_ms = interval;
        }
        settings.theme = if is_checked(ui.r_dark) {
            ThemeMode::Dark
        } else if is_checked(ui.r_light) {
            ThemeMode::Light
        } else {
            ThemeMode::System
        };
        settings.normalize();
        settings
    }
}

unsafe fn apply(ui: &SettingsUi) {
    unsafe {
        let new = read_values(ui);
        let startup_changed = {
            let mut guard = match ui.shared.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if guard.settings == new {
                return;
            }
            let changed = guard.settings.start_with_windows != new.start_with_windows;
            guard.settings = new.clone();
            changed
        };
        let _ = crate::settings::storage::save(&new);
        if startup_changed {
            let _ = crate::startup::windows::set_enabled(new.start_with_windows);
        }
        app::notify(WM_APP_SETTINGS);
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn add_control(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    id: u16,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    instance: windows::Win32::Foundation::HINSTANCE,
    font: HFONT,
) -> HWND {
    unsafe {
        let class_w = wide(class);
        let text_w = wide(text);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_w.as_ptr()),
            PCWSTR(text_w.as_ptr()),
            WINDOW_STYLE(style),
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as usize as *mut c_void)),
            Some(instance),
            None,
        )
        .unwrap_or_default();
        if !font.is_invalid() {
            let _ = SendMessageW(
                hwnd,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }
        hwnd
    }
}

fn message_font(dpi: u32) -> HFONT {
    unsafe {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let ok = SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some(&mut metrics as *mut _ as *mut c_void),
            Default::default(),
        )
        .is_ok();
        if ok {
            return CreateFontIndirectW(&metrics.lfMessageFont);
        }
        // Fallback: scale the system font height for this DPI.
        let mut logfont = LOGFONTW {
            lfHeight: -((9 * dpi as i32) / 72).max(9),
            ..Default::default()
        };
        let face = wide("Segoe UI");
        let count = face.len().min(logfont.lfFaceName.len());
        logfont.lfFaceName[..count].copy_from_slice(&face[..count]);
        CreateFontIndirectW(&logfont)
    }
}

unsafe fn is_checked(hwnd: HWND) -> bool {
    unsafe {
        SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        )
        .0 == 1
    }
}

unsafe fn set_checked(hwnd: HWND, checked: bool) {
    unsafe {
        let _ = SendMessageW(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            Some(WPARAM(if checked { 1 } else { 0 })),
            Some(LPARAM(0)),
        );
    }
}

unsafe fn read_interval(hwnd: HWND) -> Option<u32> {
    unsafe {
        let mut buffer = [0u16; 16];
        let length = GetWindowTextW(hwnd, &mut buffer);
        if length <= 0 {
            return None;
        }
        let text = String::from_utf16_lossy(&buffer[..length as usize]);
        text.trim().parse::<u32>().ok()
    }
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        let ui_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsUi;
        match msg {
            WM_CREATE => {
                let create = lparam.0 as *const CREATESTRUCTW;
                if create.is_null() {
                    return LRESULT(-1);
                }
                let shared_ptr = (*create).lpCreateParams as *mut Shared;
                let shared = Box::from_raw(shared_ptr);
                let dpi = {
                    let dpi = GetDpiForSystem();
                    if dpi == 0 { 96 } else { dpi }
                };
                let ui = create_controls(hwnd, (*create).hInstance, &shared, dpi);
                let ui_ptr = Box::into_raw(Box::new(ui));
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, ui_ptr as isize);
                SETTINGS_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
                return LRESULT(0);
            }
            WM_COMMAND => {
                if !ui_ptr.is_null() {
                    let id = (wparam.0 & 0xFFFF) as u16;
                    let code = ((wparam.0 >> 16) & 0xFFFF) as u16;
                    match id {
                        ID_APPLY => apply(&*ui_ptr),
                        ID_CLOSE => {
                            let _ = DestroyWindow(hwnd);
                        }
                        ID_INTERVAL => {
                            if code == 0x0300 {
                                // EN_CHANGE
                                apply(&*ui_ptr);
                            }
                        }
                        _ => {
                            if code == 0 {
                                // BN_CLICKED
                                apply(&*ui_ptr);
                            }
                        }
                    }
                }
                return LRESULT(0);
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                return LRESULT(0);
            }
            WM_DESTROY => {
                SETTINGS_HWND.store(0, Ordering::Relaxed);
                if !ui_ptr.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    let ui = Box::from_raw(ui_ptr);
                    if !ui.font.is_invalid() {
                        let _ = DeleteObject(HGDIOBJ(ui.font.0));
                    }
                }
                return LRESULT(0);
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling_tracks_dpi() {
        assert_eq!(scale(100, 96), 100);
        assert_eq!(scale(100, 120), 125);
        assert_eq!(scale(100, 144), 150);
    }

    #[test]
    fn settings_defaults_are_sane() {
        // Guard against accidental default drift that the UI depends on.
        assert_eq!(crate::settings::config::DEFAULT_UPDATE_INTERVAL_MS, 1000);
    }
}
