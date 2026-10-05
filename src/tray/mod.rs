//! System tray icon and event pump.

pub mod menu;

use tray_icon::menu::MenuEvent;
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::settings::Settings;
pub use menu::{TrayCommand, TrayMenuHandles};

/// Owns the tray icon and its menu handles.
pub struct Tray {
    // Kept alive for the lifetime of the application.
    _icon: TrayIcon,
    handles: TrayMenuHandles,
}

impl Tray {
    /// Creates the tray icon with the right-click menu.
    pub fn create(settings: &Settings) -> Result<Self, String> {
        let (menu, handles) = menu::build_menu(settings)?;
        let icon = Icon::from_rgba(app_icon_rgba(32), 32, 32).map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("Taskbar Monitor")
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            _icon: icon,
            handles,
        })
    }

    /// Keeps the checkable menu items in sync with the current state.
    pub fn sync(&self, settings: &Settings, paused: bool) {
        self.handles.set_paused(paused);
        self.handles
            .set_start_with_windows(settings.start_with_windows);
    }
}

/// Drains all pending menu selections.
pub fn drain_menu_events() -> Vec<TrayCommand> {
    let mut commands = Vec::new();
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        if let Some(command) = menu::command_for(event.id.0.as_str()) {
            commands.push(command);
        }
    }
    commands
}

/// True when the user clicked the tray icon with the left button.
pub fn drain_left_clicks() -> usize {
    let mut clicks = 0;
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            clicks += 1;
        }
    }
    clicks
}

/// Generates the tray icon (a soft blue disc with white up/down arrows so it is
/// visible on both light and dark taskbars).
pub fn app_icon_rgba(size: u32) -> Vec<u8> {
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let center = size as f32 / 2.0;
    let radius = size as f32 / 2.0 - 1.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 + 0.5 - center;
            let dy = y as f32 + 0.5 - center;
            if dx * dx + dy * dy <= radius * radius {
                set_pixel(&mut pixels, size, x as i32, y as i32, (59, 130, 246, 255));
            }
        }
    }

    let arrow_width = (size / 3).max(4) as i32;
    let arrow_height = (size / 4).max(3) as i32;
    let up_top = (size / 8) as i32;
    let down_top = size as i32 - up_top - arrow_height;
    fill_triangle(&mut pixels, size, arrow_width, arrow_height, up_top, true);
    fill_triangle(
        &mut pixels,
        size,
        arrow_width,
        arrow_height,
        down_top,
        false,
    );

    pixels
}

fn set_pixel(pixels: &mut [u8], size: u32, x: i32, y: i32, color: (u8, u8, u8, u8)) {
    if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
        return;
    }
    let offset = ((y as u32 * size + x as u32) * 4) as usize;
    pixels[offset] = color.0;
    pixels[offset + 1] = color.1;
    pixels[offset + 2] = color.2;
    pixels[offset + 3] = color.3;
}

fn fill_triangle(pixels: &mut [u8], size: u32, width: i32, height: i32, top: i32, up: bool) {
    let center = size as i32 / 2;
    for row in 0..height {
        let progress = if up { row + 1 } else { height - row };
        let half = ((progress as f32 / height as f32) * (width as f32 / 2.0)).round() as i32;
        let y = top + row;
        for dx in -half..=half {
            set_pixel(pixels, size, center + dx, y, (255, 255, 255, 255));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_correct_dimensions() {
        let icon = app_icon_rgba(32);
        assert_eq!(icon.len(), 32 * 32 * 4);
        assert_eq!(icon.len() % 4, 0);
    }

    #[test]
    fn icon_has_transparent_corners_and_colored_center() {
        let size = 32u32;
        let icon = app_icon_rgba(size);
        let corner = 0; // top-left pixel
        assert_eq!(icon[corner + 3], 0, "corner should be transparent");
        let center = ((size / 2 * size + size / 2) * 4) as usize;
        assert!(icon[center + 3] > 0, "center should be opaque");
    }
}
