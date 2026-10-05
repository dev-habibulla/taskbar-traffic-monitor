//! Locating the Windows taskbar and computing where the overlay sits.
//!
//! Windows 11 no longer supports deskbands, so the monitor is drawn by a
//! transparent, click-through overlay window anchored to the notification area
//! — the reliable native-looking approach.

pub mod display;

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetWindowRect, IsWindowVisible,
};

use crate::util::wide;

const TASKBAR_CLASS: &str = "Shell_TrayWnd";
const TRAY_NOTIFY_CLASS: &str = "TrayNotifyWnd";

/// Screen-space rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn from_win(rect: RECT) -> Self {
        Self {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        }
    }
}

/// Which screen edge the taskbar is docked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarEdge {
    Bottom,
    Top,
    Left,
    Right,
}

/// Everything needed to place and scale the overlay.
#[derive(Debug, Clone, Copy)]
pub struct Anchor {
    pub taskbar_hwnd: HWND,
    pub taskbar: Rect,
    pub tray: Rect,
    pub monitor: Rect,
    pub edge: TaskbarEdge,
    pub dpi: u32,
}

/// Scales a 96-DPI logical value to the given DPI.
pub fn scale(value: i32, dpi: u32) -> i32 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    (value * dpi as i32 + 48) / 96
}

/// Determines the docked edge from the taskbar and monitor rectangles.
pub fn detect_edge(taskbar: Rect, monitor: Rect) -> TaskbarEdge {
    let center_x = (monitor.left + monitor.right) / 2;
    let center_y = (monitor.top + monitor.bottom) / 2;
    if taskbar.width() >= taskbar.height() {
        if taskbar.top >= center_y {
            TaskbarEdge::Bottom
        } else {
            TaskbarEdge::Top
        }
    } else if taskbar.left >= center_x {
        TaskbarEdge::Right
    } else {
        TaskbarEdge::Left
    }
}

/// True when an auto-hidden taskbar has slid off the monitor.
pub fn is_auto_hidden(edge: TaskbarEdge, taskbar: Rect, monitor: Rect) -> bool {
    let tolerance = 2;
    match edge {
        TaskbarEdge::Bottom => taskbar.top >= monitor.bottom - tolerance,
        TaskbarEdge::Top => taskbar.bottom <= monitor.top + tolerance,
        TaskbarEdge::Left => taskbar.right <= monitor.left + tolerance,
        TaskbarEdge::Right => taskbar.left >= monitor.right - tolerance,
    }
}

/// Top-left screen position of the overlay: just before the notification area.
pub fn overlay_position(
    edge: TaskbarEdge,
    taskbar: Rect,
    tray: Rect,
    width: i32,
    height: i32,
    gap: i32,
) -> (i32, i32) {
    match edge {
        TaskbarEdge::Bottom | TaskbarEdge::Top => {
            let x = (tray.left - gap - width).clamp(taskbar.left, taskbar.right - width);
            let y = taskbar.top + (taskbar.height() - height) / 2;
            (x, y)
        }
        TaskbarEdge::Left | TaskbarEdge::Right => {
            let y = (tray.top - gap - height).clamp(taskbar.top, taskbar.bottom - height);
            let x = taskbar.left + (taskbar.width() - width) / 2;
            (x, y)
        }
    }
}

/// Finds the primary taskbar and resolves its geometry, DPI and tray area.
pub fn find_primary() -> Option<Anchor> {
    unsafe {
        let class = wide(TASKBAR_CLASS);
        let hwnd = FindWindowW(
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR::null(),
        )
        .ok()?;
        if hwnd.is_invalid() {
            return None;
        }
        resolve_anchor(hwnd)
    }
}

fn resolve_anchor(taskbar_hwnd: HWND) -> Option<Anchor> {
    unsafe {
        let mut taskbar_rect = RECT::default();
        GetWindowRect(taskbar_hwnd, &mut taskbar_rect).ok()?;
        let taskbar = Rect::from_win(taskbar_rect);

        let monitor = monitor_rect(taskbar_hwnd).unwrap_or(taskbar);
        let edge = detect_edge(taskbar, monitor);

        let mut dpi = GetDpiForWindow(taskbar_hwnd);
        if dpi == 0 {
            dpi = 96;
        }

        let tray = tray_rect(taskbar_hwnd, taskbar, dpi);

        Some(Anchor {
            taskbar_hwnd,
            taskbar,
            tray,
            monitor,
            edge,
            dpi,
        })
    }
}

unsafe fn monitor_rect(hwnd: HWND) -> Option<Rect> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        Some(Rect::from_win(info.rcMonitor))
    }
}

unsafe fn tray_rect(taskbar_hwnd: HWND, taskbar: Rect, dpi: u32) -> Rect {
    unsafe {
        let class = wide(TRAY_NOTIFY_CLASS);
        if let Ok(hwnd) = FindWindowExW(
            Some(taskbar_hwnd),
            None,
            windows::core::PCWSTR(class.as_ptr()),
            windows::core::PCWSTR::null(),
        ) && !hwnd.is_invalid()
        {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_ok() {
                return Rect::from_win(rect);
            }
        }
        // Fallback: reserve a right-hand slice of a horizontal taskbar (or a
        // bottom slice of a vertical one) for the tray area.
        let reserve = scale(200, dpi);
        if taskbar.width() >= taskbar.height() {
            Rect {
                left: (taskbar.right - reserve).max(taskbar.left),
                top: taskbar.top,
                right: taskbar.right,
                bottom: taskbar.bottom,
            }
        } else {
            Rect {
                left: taskbar.left,
                top: (taskbar.bottom - reserve).max(taskbar.top),
                right: taskbar.right,
                bottom: taskbar.bottom,
            }
        }
    }
}

/// True when the taskbar window is not visible (e.g. auto-hidden).
pub fn taskbar_visible(taskbar_hwnd: HWND) -> bool {
    unsafe { IsWindowVisible(taskbar_hwnd).as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: i32, t: i32, r: i32, b: i32) -> Rect {
        Rect {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }

    #[test]
    fn scale_respects_dpi() {
        assert_eq!(scale(100, 96), 100);
        assert_eq!(scale(100, 120), 125);
        assert_eq!(scale(100, 144), 150);
        assert_eq!(scale(100, 192), 200);
        assert_eq!(scale(100, 0), 100);
    }

    #[test]
    fn detects_all_taskbar_edges() {
        let monitor = rect(0, 0, 1920, 1080);
        assert_eq!(
            detect_edge(rect(0, 1040, 1920, 1080), monitor),
            TaskbarEdge::Bottom
        );
        assert_eq!(detect_edge(rect(0, 0, 1920, 40), monitor), TaskbarEdge::Top);
        assert_eq!(
            detect_edge(rect(0, 0, 40, 1080), monitor),
            TaskbarEdge::Left
        );
        assert_eq!(
            detect_edge(rect(1880, 0, 1920, 1080), monitor),
            TaskbarEdge::Right
        );
    }

    #[test]
    fn positions_left_of_tray_on_bottom_taskbar() {
        let taskbar = rect(0, 1040, 1920, 1080);
        let tray = rect(1700, 1040, 1920, 1080);
        let (x, y) = overlay_position(TaskbarEdge::Bottom, taskbar, tray, 300, 40, 4);
        assert_eq!(x, 1700 - 4 - 300);
        assert_eq!(y, 1040); // (40 - 40) / 2 == 0
    }

    #[test]
    fn positions_above_tray_on_vertical_taskbar() {
        let taskbar = rect(1880, 0, 1920, 1080);
        let tray = rect(1880, 900, 1920, 1080);
        let (x, y) = overlay_position(TaskbarEdge::Right, taskbar, tray, 40, 60, 4);
        assert_eq!(y, 900 - 4 - 60);
        // Vertical taskbar: width 40 equals the overlay width 40, so x is centered at 1880.
        assert_eq!(x, 1880);
    }

    #[test]
    fn position_is_clamped_into_the_taskbar() {
        let taskbar = rect(0, 1040, 1920, 1080);
        let tray = rect(0, 1040, 100, 1080); // absurd tray at the far left
        let (x, _) = overlay_position(TaskbarEdge::Bottom, taskbar, tray, 300, 40, 4);
        assert!(x >= taskbar.left);
    }

    #[test]
    fn auto_hide_detection() {
        let monitor = rect(0, 0, 1920, 1080);
        let docked = rect(0, 1040, 1920, 1080);
        let hidden = rect(0, 1078, 1920, 1080);
        assert!(!is_auto_hidden(TaskbarEdge::Bottom, docked, monitor));
        assert!(is_auto_hidden(TaskbarEdge::Bottom, hidden, monitor));

        let left_hidden = rect(0, 0, 2, 1080);
        assert!(is_auto_hidden(TaskbarEdge::Left, left_hidden, monitor));
    }

    #[test]
    fn locates_the_real_taskbar_on_this_machine() {
        let anchor = find_primary().expect("Shell_TrayWnd should exist on Windows 11");
        assert!(anchor.taskbar.width() > 0 && anchor.taskbar.height() > 0);
        assert!(anchor.tray.width() > 0 && anchor.tray.height() > 0);
        assert!(anchor.dpi >= 96, "unexpected DPI {}", anchor.dpi);
        assert!(anchor.monitor.width() > 0 && anchor.monitor.height() > 0);
        assert!(taskbar_visible(anchor.taskbar_hwnd));
    }
}
