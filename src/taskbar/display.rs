//! The taskbar overlay: a transparent, borderless, click-through window that
//! draws the two text lines directly over the taskbar background.
//!
//! Rendering is done with **Direct2D + DirectWrite** into a WIC bitmap with
//! premultiplied alpha, which is then handed to `UpdateLayeredWindow`. This
//! gives smooth anti-aliased glyphs and thin vector arrows that sit on the
//! taskbar exactly like the native clock text — there is no background, card,
//! border or shadow of any kind.

use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_LINE_JOIN_ROUND, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_STROKE_STYLE_PROPERTIES,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget,
    ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GetDC, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICBitmap, IWICImagingFactory,
    WICBitmapCacheOnLoad, WICBitmapLockRead,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GWLP_USERDATA, GetWindowLongPtrW, HWND_TOPMOST, IDC_ARROW,
    IsIconic, IsWindow, IsWindowVisible, LoadCursorW, RegisterClassW, SW_HIDE, SW_RESTORE,
    SW_SHOWNA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_SHOWWINDOW,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::PCWSTR;

use windows_numerics::Vector2;

use crate::Shared;
use crate::format::{format_percent, format_total, split_speed};
use crate::monitor::Metrics;
use crate::settings::{Settings, ThemeMode};
use crate::taskbar;
use crate::util::{dlog, wide};

/// Posted by the monitor thread (and menu handlers) to request a repaint.
pub const WM_APP_UPDATE: u32 = 0x8001;
/// Posted when settings changed and the layout must be rebuilt.
pub const WM_APP_SETTINGS: u32 = 0x8002;

const WINDOW_CLASS: &str = "TbMonOverlay";

/// Font size in DIPs, tuned to match the Windows 11 taskbar clock.
const FONT_SIZE: f32 = 12.0;
/// Horizontal inset of the whole block.
const MARGIN_X: f32 = 1.0;
/// Vertical inset of the whole block.
const MARGIN_Y: f32 = 1.0;
/// Stroke width of the vector arrows, in DIPs.
const ARROW_STROKE: f32 = 1.15;

const COLOR_TEXT_LIGHT: (u8, u8, u8) = (26, 26, 26);
const COLOR_TEXT_DARK: (u8, u8, u8) = (242, 242, 242);

/// One cell in the layout. Arrows are drawn as thin vector strokes, never as a
/// Unicode glyph, so their shape and weight are fully under our control.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    Text(String),
    ArrowUp,
    ArrowDown,
}

/// Spacing that follows a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    /// No trailing space (last column).
    None,
    /// Tight gap used *inside* a metric group (icon/label -> value).
    Intra,
    /// Wider gap used *between* metric groups.
    Inter,
}

/// What a column contains (drives alignment and width reservation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColKind {
    Arrow,
    Speed,
    Label,
    Percent,
    Total,
}

impl ColKind {
    /// Numeric columns are right-aligned so their unit/`%` stays put as values change.
    fn right_aligned(self) -> bool {
        matches!(self, ColKind::Speed | ColKind::Percent)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub gap: Gap,
    pub kind: ColKind,
}

/// Two rows of optional cells plus the column definitions, computed purely from
/// settings and metrics so it can be unit tested without a display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Option<Cell>>>,
}

impl Table {
    /// Rows that actually contain a cell (used to compact the layout).
    pub fn visible_rows(&self) -> impl Iterator<Item = &Vec<Option<Cell>>> {
        self.rows
            .iter()
            .filter(|row| row.iter().any(|c| c.is_some()))
    }
}

/// Builds the taskbar layout from the current settings and metrics.
///
/// Four independent metric groups are produced: upload, download, CPU and
/// memory (plus an optional total-traffic group). Each group keeps a consistent
/// internal spacing between its icon/label and its value.
pub fn build_table(settings: &Settings, metrics: &Metrics) -> Table {
    let show_up = settings.show_upload;
    let show_dn = settings.show_download;
    let speed = show_up || show_dn;
    let cpu_mem = settings.show_cpu || settings.show_memory;
    let total = settings.show_total_traffic;

    let mut columns = Vec::new();
    if speed {
        columns.push(Column {
            gap: Gap::Intra,
            kind: ColKind::Arrow,
        });
        columns.push(Column {
            gap: if cpu_mem || total {
                Gap::Inter
            } else {
                Gap::None
            },
            kind: ColKind::Speed,
        });
    }
    if cpu_mem {
        columns.push(Column {
            gap: Gap::Intra,
            kind: ColKind::Label,
        });
        columns.push(Column {
            gap: if total { Gap::Inter } else { Gap::None },
            kind: ColKind::Percent,
        });
    }
    if total {
        columns.push(Column {
            gap: Gap::None,
            kind: ColKind::Total,
        });
    }

    let (up_num, up_unit) = split_speed(metrics.upload_bps);
    let (dn_num, dn_unit) = split_speed(metrics.download_bps);

    let mut row0: Vec<Option<Cell>> = Vec::new();
    let mut row1: Vec<Option<Cell>> = Vec::new();

    if speed {
        row0.push(show_up.then_some(Cell::ArrowUp));
        row0.push(show_up.then(|| Cell::Text(format!("{up_num} {up_unit}"))));
        row1.push(show_dn.then_some(Cell::ArrowDown));
        row1.push(show_dn.then(|| Cell::Text(format!("{dn_num} {dn_unit}"))));
    }
    if cpu_mem {
        row0.push(settings.show_cpu.then(|| Cell::Text("CPU".to_string())));
        row0.push(
            settings
                .show_cpu
                .then(|| Cell::Text(format_percent(metrics.cpu_percent))),
        );
        row1.push(settings.show_memory.then(|| Cell::Text("MEM".to_string())));
        row1.push(
            settings
                .show_memory
                .then(|| Cell::Text(format_percent(metrics.mem_percent))),
        );
    }
    if total {
        row0.push(Some(Cell::Text(format!(
            "Total {}",
            format_total(metrics.total_bytes())
        ))));
        row1.push(None);
    }

    Table {
        columns,
        rows: vec![row0, row1],
    }
}

/// Resolves whether light-theme (dark text) colors should be used.
pub fn resolve_light_theme(mode: ThemeMode, system_uses_light: Option<bool>) -> bool {
    match mode {
        ThemeMode::Light => true,
        ThemeMode::Dark => false,
        ThemeMode::System => system_uses_light.unwrap_or(true),
    }
}

/// Reads the Windows taskbar theme from the registry.
fn system_uses_light_theme() -> Option<bool> {
    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("SystemUsesLightTheme");
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            PCWSTR(value.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    (result.0 == 0).then_some(data != 0)
}

fn text_color(light: bool) -> (u8, u8, u8) {
    if light {
        COLOR_TEXT_LIGHT
    } else {
        COLOR_TEXT_DARK
    }
}

/// Picks the best available taskbar font: Segoe UI Variable (the Windows 11 UI
/// face) with graceful fallbacks, verified against the installed font set.
fn pick_font_family(dwrite: &IDWriteFactory) -> &'static str {
    const CANDIDATES: [&str; 3] = ["Segoe UI Variable Text", "Segoe UI Variable", "Segoe UI"];
    const FALLBACK: &str = "Segoe UI";
    unsafe {
        let mut collection: Option<IDWriteFontCollection> = None;
        if dwrite
            .GetSystemFontCollection(&mut collection, false)
            .is_err()
        {
            return FALLBACK;
        }
        let collection = match collection {
            Some(collection) => collection,
            None => return FALLBACK,
        };
        for name in CANDIDATES {
            let wide_name = wide(name);
            let mut index = 0u32;
            let mut exists = windows::core::BOOL(0);
            if collection
                .FindFamilyName(PCWSTR(wide_name.as_ptr()), &mut index, &mut exists)
                .is_ok()
                && exists.as_bool()
            {
                return name;
            }
        }
    }
    FALLBACK
}

/// A device independent bitmap plus its raw pixel memory (top-down BGRA).
struct Dib {
    bitmap: HBITMAP,
    bits: *mut u8,
    width: i32,
    height: i32,
}

struct OverlayState {
    hwnd: HWND,
    shared: Shared,
    mem_dc: HDC,
    dib: Option<Dib>,

    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    wic: IWICImagingFactory,
    text_format: IDWriteTextFormat,
    stroke: ID2D1StrokeStyle,

    wic_bitmap: Option<IWICBitmap>,
    target: Option<ID2D1RenderTarget>,
    target_pixels: (i32, i32),
    target_dpi: u32,

    /// Cleared when the OS destroys the window, so the controller knows to
    /// recreate it instead of rendering into a dead HWND.
    alive: bool,
}

/// Owns the overlay window and its rendering resources.
pub struct Overlay {
    hwnd: HWND,
    state: *mut OverlayState,
}

impl Overlay {
    /// Creates the overlay window (must be called on the UI thread).
    pub fn create(shared: Shared) -> Option<Overlay> {
        unsafe {
            let instance = GetModuleHandleW(None).ok()?;

            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            let wic: IWICImagingFactory =
                CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;

            let family = pick_font_family(&dwrite);
            let family = wide(family);
            let locale = wide("en-us");
            let text_format = dwrite
                .CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(400),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    FONT_SIZE,
                    PCWSTR(locale.as_ptr()),
                )
                .ok()?;
            let _ = text_format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);

            let stroke_properties = D2D1_STROKE_STYLE_PROPERTIES {
                startCap: D2D1_CAP_STYLE_ROUND,
                endCap: D2D1_CAP_STYLE_ROUND,
                dashCap: D2D1_CAP_STYLE_ROUND,
                lineJoin: D2D1_LINE_JOIN_ROUND,
                miterLimit: 1.0,
                dashStyle: D2D1_DASH_STYLE_SOLID,
                dashOffset: 0.0,
            };
            let stroke = d2d.CreateStrokeStyle(&stroke_properties, None).ok()?;

            let screen_dc = GetDC(None);
            let mem_dc = CreateCompatibleDC(Some(screen_dc));
            ReleaseDC(None, screen_dc);
            if mem_dc.is_invalid() {
                return None;
            }

            let class_name = wide(WINDOW_CLASS);
            let class = WNDCLASSW {
                lpfnWndProc: Some(overlay_wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassW(&class);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(class_name.as_ptr()),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;

            let state = Box::new(OverlayState {
                hwnd,
                shared,
                mem_dc,
                dib: None,
                d2d,
                dwrite,
                wic,
                text_format,
                stroke,
                wic_bitmap: None,
                target: None,
                target_pixels: (0, 0),
                target_dpi: 0,
                alive: true,
            });
            let state_ptr = Box::into_raw(state);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);

            let mut overlay = Overlay {
                hwnd,
                state: state_ptr,
            };
            overlay.render();
            Some(overlay)
        }
    }

    /// True while the OS window still exists and has not been destroyed.
    pub fn is_alive(&self) -> bool {
        unsafe {
            !self.state.is_null() && (*self.state).alive && IsWindow(Some(self.hwnd)).as_bool()
        }
    }

    /// Forces the Direct2D surface to be rebuilt on the next render (used after
    /// display/DPI changes, which can invalidate the composited surface).
    pub fn invalidate_surface(&mut self) {
        unsafe {
            if !self.state.is_null() {
                invalidate_surface(&mut *self.state);
            }
        }
    }

    /// Restores the window if Windows hid or iconified it, and re-asserts
    /// topmost z-order. Cheap, idempotent, and safe to call frequently.
    pub fn ensure_shown(&mut self) {
        unsafe {
            if !self.state.is_null() {
                ensure_shown(&mut *self.state);
            }
        }
    }

    /// Rebuilds the display from the current shared state.
    pub fn render(&mut self) {
        unsafe {
            if !self.state.is_null() {
                render(&mut *self.state);
            }
        }
    }

    /// Hides the overlay (the user explicitly chose Hide) without destroying it.
    pub fn hide(&mut self) {
        unsafe {
            if !self.state.is_null() {
                hide_window(&mut *self.state);
            }
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            if !self.state.is_null() {
                // Detach the state pointer *before* destroying the window, so the
                // resulting WM_DESTROY cannot dereference freed memory.
                SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                let state = Box::from_raw(self.state);
                self.state = ptr::null_mut();
                let _ = DeleteDC(state.mem_dc);
                if let Some(dib) = &state.dib {
                    let _ = DeleteObject(HGDIOBJ(dib.bitmap.0));
                }
            }
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.hwnd);
        }
    }
}

unsafe fn hide_window(state: &mut OverlayState) {
    unsafe {
        if state.alive {
            let _ = ShowWindow(state.hwnd, SW_HIDE);
        }
    }
}

/// Re-shows the overlay and re-asserts topmost z-order.
///
/// This is the heart of the self-healing behaviour: the window's *real* state
/// (`IsWindowVisible` / `IsIconic`) is checked every time instead of trusting a
/// cached flag, so any hide or z-order demotion Windows performs — Explorer
/// taskbar recreation, Show-Desktop, session/fullscreen transitions or another
/// topmost window — is corrected automatically. `UpdateLayeredWindow` cannot
/// un-hide a hidden window, so this must run before/after presenting.
unsafe fn ensure_shown(state: &mut OverlayState) {
    unsafe {
        if !state.alive {
            return;
        }
        let hwnd = state.hwnd;
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOWNA);
        }
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
        );
    }
}

/// Drops the Direct2D/WIC surface so the next render rebuilds it from scratch.
unsafe fn invalidate_surface(state: &mut OverlayState) {
    state.target = None;
    state.wic_bitmap = None;
    state.target_pixels = (0, 0);
    state.target_dpi = 0;
}

/// Whether the overlay should currently be shown.
///
/// Kept pure so the visibility policy is unit-testable: the monitor is shown
/// only when the user wants it, a taskbar exists and is visible, and it is not
/// auto-hidden.
pub fn should_show(
    overlay_visible: bool,
    taskbar_found: bool,
    taskbar_visible: bool,
    auto_hidden: bool,
) -> bool {
    overlay_visible && taskbar_found && taskbar_visible && !auto_hidden
}

unsafe fn render(state: &mut OverlayState) {
    unsafe {
        if !state.alive {
            return;
        }

        let (settings, metrics, overlay_visible) = match state.shared.lock() {
            Ok(guard) => (guard.settings.clone(), guard.metrics, guard.overlay_visible),
            Err(_) => return,
        };

        let anchor = taskbar::find_primary();
        let show = should_show(
            overlay_visible,
            anchor.is_some(),
            anchor
                .as_ref()
                .is_some_and(|a| taskbar::taskbar_visible(a.taskbar_hwnd)),
            anchor
                .as_ref()
                .is_some_and(|a| taskbar::is_auto_hidden(a.edge, a.taskbar, a.monitor)),
        );
        if !show {
            hide_window(state);
            return;
        }
        let anchor = match anchor {
            Some(anchor) => anchor,
            None => {
                hide_window(state);
                return;
            }
        };

        let table = build_table(&settings, &metrics);
        if table.columns.is_empty() || table.visible_rows().next().is_none() {
            hide_window(state);
            return;
        }

        let line_height = text_height(state, "Ag").max(FONT_SIZE);
        let intra = FONT_SIZE * 0.32;
        let inter = FONT_SIZE * 0.95;
        let icon_width = FONT_SIZE * 0.55;

        let gap = |gap: Gap| match gap {
            Gap::None => 0.0,
            Gap::Intra => intra,
            Gap::Inter => inter,
        };

        let mut column_widths = vec![0f32; table.columns.len()];
        for (index, column) in table.columns.iter().enumerate() {
            // Reserve a stable width for numeric columns so the layout does not
            // jitter as digit counts and units change from sample to sample.
            let reserved = match column.kind {
                ColKind::Speed => text_width(state, "888.8 KB/s"),
                ColKind::Percent => text_width(state, "100%"),
                _ => 0.0,
            };
            let mut width = reserved;
            for row in &table.rows {
                if let Some(cell) = &row[index] {
                    let cell_width = match cell {
                        Cell::Text(text) => text_width(state, text),
                        Cell::ArrowUp | Cell::ArrowDown => icon_width,
                    };
                    width = width.max(cell_width);
                }
            }
            column_widths[index] = width;
        }

        let visible_rows = table.visible_rows().count() as f32;
        let mut content_width = MARGIN_X * 2.0;
        for (index, width) in column_widths.iter().enumerate() {
            content_width += width + gap(table.columns[index].gap);
        }
        let content_height = MARGIN_Y * 2.0 + line_height * visible_rows;

        let dpi = anchor.dpi.max(96);
        let pixel_width = pixels_for_dip(content_width, dpi);
        let pixel_height = pixels_for_dip(content_height, dpi);

        if !ensure_target(state, pixel_width, pixel_height, dpi) {
            dlog(&format!(
                "ensure_target failed ({pixel_width}x{pixel_height})"
            ));
            return;
        }

        let light = resolve_light_theme(settings.theme, system_uses_light_theme());
        let (red, green, blue) = text_color(light);
        let color = D2D1_COLOR_F {
            r: red as f32 / 255.0,
            g: green as f32 / 255.0,
            b: blue as f32 / 255.0,
            a: 1.0,
        };

        if !draw(state, &table, &column_widths, line_height, gap, color) {
            dlog("draw failed");
            return;
        }
        if !copy_to_dib(state) {
            dlog("copy_to_dib failed");
            return;
        }
        dlog(&format!(
            "rendered {pixel_width}x{pixel_height} dpi={dpi} dip={content_width:.1}x{content_height:.1} line={line_height:.1}"
        ));

        let (x, y) = taskbar::overlay_position(
            anchor.edge,
            anchor.taskbar,
            anchor.tray,
            pixel_width,
            pixel_height,
            taskbar::scale(4, dpi),
        );
        present(state, x, y, pixel_width, pixel_height);
    }
}

/// (Re)creates the WIC bitmap, Direct2D render target and DIB when the size or
/// DPI changes.
unsafe fn ensure_target(state: &mut OverlayState, width: i32, height: i32, dpi: u32) -> bool {
    unsafe {
        if state.target.is_some()
            && state.target_pixels == (width, height)
            && state.target_dpi == dpi
        {
            return true;
        }

        let bitmap = match state.wic.CreateBitmap(
            width as u32,
            height as u32,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapCacheOnLoad,
        ) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                dlog("wic CreateBitmap failed");
                return false;
            }
        };

        let properties = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };

        let target = match state.d2d.CreateWicBitmapRenderTarget(&bitmap, &properties) {
            Ok(target) => target,
            Err(error) => {
                dlog(&format!(
                    "d2d CreateWicBitmapRenderTarget failed: 0x{:08X}",
                    error.code().0 as u32
                ));
                return false;
            }
        };

        let dib = match create_dib(width, height) {
            Some(dib) => dib,
            None => {
                dlog("create_dib failed");
                return false;
            }
        };
        let _ = SelectObject(state.mem_dc, HGDIOBJ(dib.bitmap.0));
        if let Some(old) = state.dib.take() {
            let _ = DeleteObject(HGDIOBJ(old.bitmap.0));
        }

        state.wic_bitmap = Some(bitmap);
        state.target = Some(target);
        state.dib = Some(dib);
        state.target_pixels = (width, height);
        state.target_dpi = dpi;
        true
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw(
    state: &OverlayState,
    table: &Table,
    widths: &[f32],
    line_height: f32,
    gap: impl Fn(Gap) -> f32,
    color: D2D1_COLOR_F,
) -> bool {
    unsafe {
        let target = match state.target.as_ref() {
            Some(target) => target,
            None => return false,
        };

        target.BeginDraw();
        let transparent = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };
        target.Clear(Some(&transparent));
        target.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
        target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

        let brush = match target.CreateSolidColorBrush(&color, None) {
            Ok(brush) => brush,
            Err(_) => {
                let _ = target.EndDraw(None, None);
                return false;
            }
        };

        let icon_height = (FONT_SIZE * 0.78).min(line_height);
        let mut row_index = 0f32;
        for row in &table.rows {
            if row.iter().all(|cell| cell.is_none()) {
                continue;
            }
            let y = MARGIN_Y + row_index * line_height;
            let mut x = MARGIN_X;
            for (index, cell) in row.iter().enumerate() {
                let width = widths[index];
                if let Some(cell) = cell {
                    match cell {
                        Cell::Text(text) => {
                            let text_pixels = text_width(state, text);
                            let draw_x = if table.columns[index].kind.right_aligned() {
                                x + width - text_pixels
                            } else {
                                x
                            };
                            let rect = D2D_RECT_F {
                                left: draw_x,
                                top: y,
                                right: draw_x + text_pixels + 6.0,
                                bottom: y + line_height,
                            };
                            target.DrawText(
                                &units(text),
                                &state.text_format,
                                &rect,
                                &brush,
                                D2D1_DRAW_TEXT_OPTIONS_NONE,
                                DWRITE_MEASURING_MODE_NATURAL,
                            );
                        }
                        Cell::ArrowUp => draw_arrow(
                            target,
                            &brush,
                            &state.stroke,
                            x,
                            y + (line_height - icon_height) / 2.0,
                            widths[index],
                            icon_height,
                            true,
                        ),
                        Cell::ArrowDown => draw_arrow(
                            target,
                            &brush,
                            &state.stroke,
                            x,
                            y + (line_height - icon_height) / 2.0,
                            widths[index],
                            icon_height,
                            false,
                        ),
                    }
                }
                x += width + gap(table.columns[index].gap);
            }
            row_index += 1.0;
        }

        target.EndDraw(None, None).is_ok()
    }
}

/// Draws a thin, anti-aliased up/down arrow (stem plus a two-stroke head).
#[allow(clippy::too_many_arguments)]
unsafe fn draw_arrow(
    target: &ID2D1RenderTarget,
    brush: &ID2D1SolidColorBrush,
    stroke: &ID2D1StrokeStyle,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    up: bool,
) {
    unsafe {
        let center = x + width / 2.0;
        let pad = height * 0.05;
        let (tip, tail) = if up {
            (y + pad, y + height - pad)
        } else {
            (y + height - pad, y + pad)
        };
        let head_width = width * 0.44;
        let head_height = height * 0.40;
        let base = if up {
            tip + head_height
        } else {
            tip - head_height
        };

        let line = |ax: f32, ay: f32, bx: f32, by: f32| {
            target.DrawLine(
                Vector2::new(ax, ay),
                Vector2::new(bx, by),
                brush,
                ARROW_STROKE,
                Some(stroke),
            );
        };
        line(center, tail, center, tip);
        line(center - head_width, base, center, tip);
        line(center, tip, center + head_width, base);
    }
}

/// Copies the premultiplied WIC pixels into our DIB (source for
/// `UpdateLayeredWindow`).
unsafe fn copy_to_dib(state: &mut OverlayState) -> bool {
    unsafe {
        let (bitmap, dib) = match (state.wic_bitmap.as_ref(), state.dib.as_mut()) {
            (Some(bitmap), Some(dib)) => (bitmap, dib),
            _ => return false,
        };
        let lock = match bitmap.Lock(std::ptr::null(), WICBitmapLockRead.0 as u32) {
            Ok(lock) => lock,
            Err(_) => return false,
        };
        let source_stride = match lock.GetStride() {
            Ok(stride) => stride,
            Err(_) => return false,
        };
        let mut buffer_size = 0u32;
        let mut source: *mut u8 = ptr::null_mut();
        if lock.GetDataPointer(&mut buffer_size, &mut source).is_err() || source.is_null() {
            return false;
        }

        let destination_stride = (dib.width * 4) as usize;
        let row_bytes = destination_stride.min(source_stride as usize);
        let rows = dib.height as usize;
        for row in 0..rows {
            let src = source.add(row * source_stride as usize);
            let dst = dib.bits.add(row * destination_stride);
            ptr::copy_nonoverlapping(src, dst, row_bytes);
        }
        true
    }
}

unsafe fn present(state: &mut OverlayState, x: i32, y: i32, width: i32, height: i32) {
    unsafe {
        let screen_dc = GetDC(None);
        let destination = POINT { x, y };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = windows::Win32::Graphics::Gdi::BLENDFUNCTION {
            BlendOp: windows::Win32::Graphics::Gdi::AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: windows::Win32::Graphics::Gdi::AC_SRC_ALPHA as u8,
        };
        let result = UpdateLayeredWindow(
            state.hwnd,
            Some(screen_dc),
            Some(&destination),
            Some(&size),
            Some(state.mem_dc),
            Some(&source),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );
        ReleaseDC(None, screen_dc);
        if result.is_ok() {
            ensure_shown(state);
        }
    }
}

unsafe fn text_width(state: &OverlayState, text: &str) -> f32 {
    unsafe { text_metrics(state, text).map(|m| m.0).unwrap_or(0.0) }
}

unsafe fn text_height(state: &OverlayState, text: &str) -> f32 {
    unsafe {
        text_metrics(state, text)
            .map(|m| m.1)
            .unwrap_or(FONT_SIZE * 1.3)
    }
}

unsafe fn text_metrics(state: &OverlayState, text: &str) -> Option<(f32, f32)> {
    unsafe {
        let layout = state
            .dwrite
            .CreateTextLayout(&units(text), &state.text_format, 4096.0, 256.0)
            .ok()?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        layout.GetMetrics(&mut metrics).ok()?;
        Some((metrics.width.max(0.0), metrics.height.max(0.0)))
    }
}

fn units(text: &str) -> Vec<u16> {
    let mut wide = wide(text);
    wide.pop(); // drop the NUL terminator
    wide
}

/// Converts a size in DIPs to device pixels for the given DPI (rounded up).
pub fn pixels_for_dip(dip: f32, dpi: u32) -> i32 {
    let dpi = dpi.max(96);
    (dip * dpi as f32 / 96.0).ceil().max(1.0) as i32
}

unsafe fn create_dib(width: i32, height: i32) -> Option<Dib> {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen_dc = GetDC(None);
        let mut bits: *mut c_void = ptr::null_mut();
        let bitmap = CreateDIBSection(Some(screen_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, screen_dc);
        let bitmap = bitmap.ok()?;
        Some(Dib {
            bitmap,
            bits: bits as *mut u8,
            width,
            height,
        })
    }
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        HTTRANSPARENT, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_NCDESTROY,
        WM_NCHITTEST,
    };
    unsafe {
        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut OverlayState;
        match msg {
            WM_NCHITTEST => return LRESULT(HTTRANSPARENT as isize),
            WM_ERASEBKGND => return LRESULT(1),
            WM_APP_UPDATE | WM_APP_SETTINGS => {
                if !state_ptr.is_null() {
                    render(&mut *state_ptr);
                }
                return LRESULT(0);
            }
            WM_DPICHANGED | WM_DISPLAYCHANGE => {
                if !state_ptr.is_null() {
                    invalidate_surface(&mut *state_ptr);
                    render(&mut *state_ptr);
                }
                return LRESULT(0);
            }
            // The OS is tearing this window down: flag it so the controller
            // recreates it instead of rendering into a dead HWND. Application,
            // tray and worker lifetimes are unaffected.
            WM_DESTROY | WM_NCDESTROY => {
                if !state_ptr.is_null() {
                    (*state_ptr).alive = false;
                }
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
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

    fn metrics() -> Metrics {
        Metrics {
            cpu_percent: 59.0,
            mem_percent: 48.0,
            upload_bps: 835.7 * 1024.0,
            download_bps: 41.6 * 1024.0,
            total_up_bytes: 100 * 1024 * 1024,
            total_down_bytes: 149 * 1024 * 1024,
            ..Default::default()
        }
    }

    fn text_of(cell: &Option<Cell>) -> Option<&str> {
        match cell {
            Some(Cell::Text(text)) => Some(text),
            _ => None,
        }
    }

    #[test]
    fn default_layout_groups_metrics() {
        let table = build_table(&Settings::default(), &metrics());
        assert_eq!(table.rows.len(), 2);
        // Four columns: arrow, speed value, CPU/MEM label, percentage.
        assert_eq!(table.columns.len(), 4);

        let row0 = &table.rows[0];
        let row1 = &table.rows[1];
        assert_eq!(row0[0], Some(Cell::ArrowUp));
        assert_eq!(text_of(&row0[1]), Some("835.7 KB/s"));
        assert_eq!(text_of(&row0[2]), Some("CPU"));
        assert_eq!(text_of(&row0[3]), Some("59%"));

        assert_eq!(row1[0], Some(Cell::ArrowDown));
        assert_eq!(text_of(&row1[1]), Some("41.6 KB/s"));
        assert_eq!(text_of(&row1[2]), Some("MEM"));
        assert_eq!(text_of(&row1[3]), Some("48%"));
    }

    #[test]
    fn total_traffic_off_leaves_no_blank_column() {
        let table = build_table(&Settings::default(), &metrics());
        assert!(
            table
                .rows
                .iter()
                .all(|row| row.iter().all(|c| text_of(c) != Some("Total 249 MB")))
        );
    }

    #[test]
    fn total_traffic_on_expands_the_layout() {
        let settings = Settings {
            show_total_traffic: true,
            ..Default::default()
        };
        let table = build_table(&settings, &metrics());
        assert_eq!(table.columns.len(), 5);
        assert_eq!(text_of(&table.rows[0][4]), Some("Total 249 MB"));
        assert_eq!(table.rows[1][4], None);
    }

    #[test]
    fn disabling_a_metric_removes_its_cell() {
        let settings = Settings {
            show_cpu: false,
            ..Default::default()
        };
        let table = build_table(&settings, &metrics());
        assert_eq!(text_of(&table.rows[0][2]), None);
        assert_eq!(text_of(&table.rows[0][3]), None);
        // Memory still present on the second row.
        assert_eq!(text_of(&table.rows[1][2]), Some("MEM"));
    }

    #[test]
    fn disabling_all_speeds_drops_the_speed_columns() {
        let settings = Settings {
            show_upload: false,
            show_download: false,
            ..Default::default()
        };
        let table = build_table(&settings, &metrics());
        assert_eq!(table.columns.len(), 2); // CPU/MEM label + value
        assert_eq!(text_of(&table.rows[0][0]), Some("CPU"));
    }

    #[test]
    fn single_speed_row_is_compacted() {
        let settings = Settings {
            show_download: false,
            show_memory: false,
            ..Default::default()
        };
        let table = build_table(&settings, &metrics());
        assert_eq!(table.visible_rows().count(), 1);
    }

    #[test]
    fn every_metric_can_be_disabled() {
        let settings = Settings {
            show_upload: false,
            show_download: false,
            show_cpu: false,
            show_memory: false,
            show_total_traffic: false,
            ..Default::default()
        };
        let table = build_table(&settings, &metrics());
        assert!(table.columns.is_empty());
    }

    #[test]
    fn theme_resolution_prefers_override() {
        assert!(resolve_light_theme(ThemeMode::Light, Some(false)));
        assert!(!resolve_light_theme(ThemeMode::Dark, Some(true)));
        assert!(resolve_light_theme(ThemeMode::System, Some(true)));
        assert!(!resolve_light_theme(ThemeMode::System, Some(false)));
        assert!(resolve_light_theme(ThemeMode::System, None));
    }

    #[test]
    fn light_theme_uses_dark_text() {
        assert_eq!(text_color(true), COLOR_TEXT_LIGHT);
        assert_eq!(text_color(false), COLOR_TEXT_DARK);
    }

    #[test]
    fn pixel_size_scales_with_dpi() {
        assert_eq!(pixels_for_dip(100.0, 96), 100);
        assert_eq!(pixels_for_dip(100.0, 120), 125);
        assert_eq!(pixels_for_dip(100.0, 144), 150);
        assert_eq!(pixels_for_dip(100.0, 168), 175);
        assert_eq!(pixels_for_dip(0.1, 96), 1);
    }

    #[test]
    fn visibility_policy_only_shows_when_wanted_and_taskbar_ready() {
        assert!(should_show(true, true, true, false));
        assert!(!should_show(false, true, true, false), "user hid it");
        assert!(!should_show(true, false, true, false), "no taskbar");
        assert!(!should_show(true, true, false, false), "taskbar invisible");
        assert!(!should_show(true, true, true, true), "auto-hidden");
    }

    #[test]
    fn reads_the_windows_theme_from_the_registry() {
        assert!(
            system_uses_light_theme().is_some(),
            "SystemUsesLightTheme should be readable on Windows 11"
        );
    }
}
