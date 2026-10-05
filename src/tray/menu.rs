//! The tray context menu and its command mapping.

use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};

use crate::settings::Settings;

pub const ID_SHOW: &str = "show";
pub const ID_HIDE: &str = "hide";
pub const ID_SETTINGS: &str = "settings";
pub const ID_PAUSE: &str = "pause";
pub const ID_STARTUP: &str = "startup";
pub const ID_EXIT: &str = "exit";

/// A user action chosen from the tray menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Show,
    Hide,
    Settings,
    TogglePause,
    ToggleStartup,
    Exit,
}

/// Maps a menu id to its command.
pub fn command_for(id: &str) -> Option<TrayCommand> {
    Some(match id {
        ID_SHOW => TrayCommand::Show,
        ID_HIDE => TrayCommand::Hide,
        ID_SETTINGS => TrayCommand::Settings,
        ID_PAUSE => TrayCommand::TogglePause,
        ID_STARTUP => TrayCommand::ToggleStartup,
        ID_EXIT => TrayCommand::Exit,
        _ => return None,
    })
}

/// Handles to the checkable items, kept so their state can be synced.
pub struct TrayMenuHandles {
    pause: CheckMenuItem,
    startup: CheckMenuItem,
}

impl TrayMenuHandles {
    pub fn set_paused(&self, paused: bool) {
        self.pause.set_checked(paused);
    }

    pub fn set_start_with_windows(&self, enabled: bool) {
        self.startup.set_checked(enabled);
    }
}

/// Builds the menu and returns it together with its checkable-item handles.
pub fn build_menu(settings: &Settings) -> Result<(Menu, TrayMenuHandles), String> {
    let menu = Menu::new();
    let show = MenuItem::with_id(ID_SHOW, "Show Taskbar Monitor", true, None);
    let hide = MenuItem::with_id(ID_HIDE, "Hide Taskbar Monitor", true, None);
    let settings_item = MenuItem::with_id(ID_SETTINGS, "Settings", true, None);
    let pause = CheckMenuItem::with_id(ID_PAUSE, "Pause Monitoring", true, false, None);
    let startup = CheckMenuItem::with_id(
        ID_STARTUP,
        "Start with Windows",
        true,
        settings.start_with_windows,
        None,
    );
    let exit = MenuItem::with_id(ID_EXIT, "Exit", true, None);

    let separator1 = PredefinedMenuItem::separator();
    let separator2 = PredefinedMenuItem::separator();
    let separator3 = PredefinedMenuItem::separator();

    menu.append_items(&[
        &show,
        &hide,
        &separator1,
        &settings_item,
        &separator2,
        &pause,
        &startup,
        &separator3,
        &exit,
    ])
    .map_err(|e| e.to_string())?;

    Ok((menu, TrayMenuHandles { pause, startup }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_ids_are_mapped() {
        assert_eq!(command_for(ID_SHOW), Some(TrayCommand::Show));
        assert_eq!(command_for(ID_HIDE), Some(TrayCommand::Hide));
        assert_eq!(command_for(ID_SETTINGS), Some(TrayCommand::Settings));
        assert_eq!(command_for(ID_PAUSE), Some(TrayCommand::TogglePause));
        assert_eq!(command_for(ID_STARTUP), Some(TrayCommand::ToggleStartup));
        assert_eq!(command_for(ID_EXIT), Some(TrayCommand::Exit));
        assert_eq!(command_for("nope"), None);
    }
}
