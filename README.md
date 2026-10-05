# Taskbar Monitor

A minimal, native Windows 11 system monitor that draws a small two-line readout
directly on the taskbar notification area — in the spirit of TrafficMonitor, but
written entirely in Rust on top of the Windows API.

```
↑ 835.7 KB/s   CPU 59%
↓  41.6 KB/s   MEM 48%
```

- Pure **Rust** — no React, Vite, Node.js, Tauri, Electron or WebView.
- **347 KB** release binary.
- Text only: no background, card, border, shadow or rounded corners. The glyphs
  are drawn straight onto the taskbar and blend with whatever is behind them.
- Click-through: the taskbar underneath stays fully usable.

## Features

- **Upload / Download speed** with automatic unit selection (`B/s → KB/s → MB/s → GB/s → TB/s`).
- **CPU** usage and **Memory** usage percentages.
- Optional **Total Traffic** (bytes transferred this session), off by default.
- Every metric can be enabled/disabled independently; disabled items leave no
  blank space (the layout compacts itself).
- **System tray** icon with a right-click menu.
- **Start with Windows** (per-user `Run` key, no admin rights needed).
- Follows the Windows **Light / Dark** theme automatically.
- **DPI aware** (100/125/150/175%…) and handles multi-monitor setups, taskbar
  position changes and taskbar auto-hide.

## How the taskbar placement works

Windows 11 removed deskbands, so an in-taskbar "band" is not possible. The
monitor is instead a **transparent, click-through, topmost overlay window**
anchored immediately to the left of the notification area (`TrayNotifyWnd`
inside `Shell_TrayWnd`). It is:

- `WS_EX_LAYERED` with per-pixel alpha (via `UpdateLayeredWindow`), so only the
  glyphs are ever painted — never a background.
- `WS_EX_TRANSPARENT` + `HTTRANSPARENT`, so mouse input passes through to the
  taskbar.
- `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST`, so it never appears in
  Alt+Tab and never steals focus.

The text is rendered with **DirectWrite** (Segoe UI Variable Text, regular
weight, sized to match the taskbar clock) and the thin up/down arrows with
**Direct2D**, composited into a WIC bitmap with premultiplied alpha and uploaded
to the layered window via `UpdateLayeredWindow`. No Unicode arrow glyphs are
used — the arrows are custom thin, anti-aliased strokes, so their shape and
weight stay under our control and every metric shares one colour family.

The layout is built from four independent metric groups (upload, download, CPU,
memory), with a consistent tight gap inside each group and a wider gap between
groups. Numeric columns reserve a fixed width so the widget does not jitter as
values change.

### Lifecycle & self-healing

The overlay is deliberately treated as a **disposable render surface**. A stable,
hidden controller window owns the application lifetime, the tray, the worker and
the overlay:

- the worker and the settings window post to the **controller**, never to the
  overlay, so a lost overlay cannot break them;
- every render verifies the window's *real* state (`IsWindowVisible` /
  `IsIconic`) and re-asserts topmost z-order, instead of trusting a cached flag —
  so a hide or z-order demotion by Windows is corrected automatically;
- if the overlay window is destroyed by the OS, the controller recreates it;
- a 2-second health timer plus `WM_SETTINGCHANGE`, `WM_DISPLAYCHANGE`,
  `WM_DPICHANGED`, `WM_DEVICECHANGE`, `WM_POWERBROADCAST` and the
  `TaskbarCreated` message (Explorer restart) trigger surface invalidation and
  re-anchoring.

The monitor therefore stays visible for the whole application lifetime unless
the user explicitly chooses **Hide Taskbar Monitor**; an explicit hide is never
overridden. A per-user named mutex enforces a single instance.

## Architecture

```
src/
├── main.rs              Process bootstrap + single-instance guard
├── app.rs               Application controller: tray/worker/overlay lifetime,
│                        message loop, TaskbarCreated + self-healing recovery
├── format.rs            Byte-rate / total / percent formatting (auto units)
├── util.rs              Wide-string + debug-logging helpers
├── monitor/
│   ├── mod.rs           Metrics snapshot + sampler orchestration
│   ├── cpu.rs           GetSystemTimes (kernel/user/idle deltas)
│   ├── memory.rs        GlobalMemoryStatusEx
│   └── network.rs       GetIfTable2, per-interface speed meter + session totals
├── taskbar/
│   ├── mod.rs           Taskbar/tray discovery, DPI scaling, placement geometry
│   └── display.rs       Transparent overlay window: Direct2D/DirectWrite engine
├── tray/
│   ├── mod.rs           Tray icon + event pump
│   └── menu.rs          Context menu and command mapping
├── settings/
│   ├── mod.rs
│   ├── config.rs        Settings model and defaults
│   └── storage.rs       JSON persistence in %APPDATA%\TaskbarMonitor
├── startup/
│   └── windows.rs       HKCU Run-key autostart
└── ui/
    └── settings.rs      Native DPI-aware settings window
```

**Threading** — exactly two threads:

1. The **UI thread** owns the controller window, the overlay, the settings
   window, the tray icon and the Win32 message loop. It only repaints when woken
   by a posted message or the health timer — it never busy-polls.
2. A single **sampler thread** samples CPU/memory/network at the configured
   interval, stores the snapshot under a mutex and wakes the controller with
   `PostMessage(WM_APP_UPDATE)`. It sleeps in short slices so shutdown is
   immediate.

No busy-waiting, no per-metric timers, no extra threads.

## Settings

Stored at `%APPDATA%\TaskbarMonitor\config.json` and applied immediately when
changed. Defaults:

| Setting              | Default |
| -------------------- | ------- |
| Upload Speed         | on      |
| Download Speed       | on      |
| CPU                  | on      |
| Memory               | on      |
| Total Traffic        | off     |
| Update interval      | 1000 ms |
| Theme                | System  |
| Start with Windows   | on      |
| Start minimized      | on      |

The tray right-click menu offers: **Show Taskbar Monitor**, **Hide Taskbar
Monitor**, **Settings**, **Pause Monitoring**, **Start with Windows** and
**Exit**. Closing the settings window only closes the window — monitoring keeps
running in the background.

## Build & run

Requires the Rust stable toolchain with the MSVC target and the Visual Studio
C++ build tools.

```powershell
cargo build --release
.\target\release\taskbar-monitor.exe
```

The release profile is tuned for size (`opt-level = "z"`, LTO, `panic = "abort"`,
stripped). Debug builds keep a console for logs.

## Tests

```powershell
cargo test
```

56 unit tests cover CPU/RAM percentages, upload/download rate maths (including
interface resets, appearing/disappearing interfaces, adapter reconnect and the
first-sample case), unit conversion, layout compaction for enabled/disabled
metrics and Total Traffic, the overlay visibility policy, light/dark theme
resolution, DPI scaling, taskbar edge/auto-hide geometry, settings persistence
and corrupt-file recovery, the autostart registry round-trip (against a sandbox
key), single-instance detection and the tray command mapping. A few tests also
exercise the live system (real taskbar discovery, theme registry, network
interfaces).

## Notes & limitations

- The monitor targets the **primary** taskbar, matching TrafficMonitor's default
  behaviour. Secondary-monitor taskbars (`Shell_SecondaryTrayWnd`) are not drawn
  on.
- The menu and tray icon are provided by the `tray-icon` crate (which already
  re-registers on `TaskbarCreated`); the taskbar overlay is pure Win32/Direct2D.
