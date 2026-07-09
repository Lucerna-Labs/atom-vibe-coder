use crate::model::{NativeApp, CAPTURE_PROOF, EXEC_PROVIDER, MARK_DRIFT, RUN_LOOP};
use crate::ui;
use core::ffi::c_void;
use pmre_kit::ux::UxNode;
use pmre_orchestrator::{handle_event, render_ui_quality, Quality, UiEvent, UiState};
use std::cell::RefCell;

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hmenu = *mut c_void;
type Hdc = *mut c_void;
type Hicon = *mut c_void;
type Hcursor = *mut c_void;
type Hbrush = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
type WndProc = Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult>;

#[repr(C)]
struct WndClassW {
    style: u32,
    proc_: WndProc,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Hinstance,
    icon: Hicon,
    cursor: Hcursor,
    background: Hbrush,
    menu_name: *const u16,
    class_name: *const u16,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    pt: Point,
}

#[repr(C)]
struct PaintStruct {
    hdc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

#[repr(C)]
struct TrackMouseEventOpts {
    size: u32,
    flags: u32,
    hwnd: Hwnd,
    hover_time: u32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    fn SetProcessDpiAwarenessContext(ctx: isize) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn SetCapture(hwnd: Hwnd) -> Hwnd;
    fn ReleaseCapture() -> i32;
    fn TrackMouseEvent(t: *mut TrackMouseEventOpts) -> i32;
    fn RegisterClassW(c: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: Hmenu,
        inst: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, msg: u32, wp: Wparam, lp: Lparam) -> Lresult;
    fn GetMessageW(msg: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> Lresult;
    fn PostQuitMessage(code: i32);
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn BeginPaint(hwnd: Hwnd, ps: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, ps: *const PaintStruct) -> i32;
    fn LoadCursorW(inst: Hinstance, name: *const u16) -> Hcursor;
}

#[link(name = "gdi32")]
extern "system" {
    fn StretchDIBits(
        hdc: Hdc,
        xd: i32,
        yd: i32,
        wd: i32,
        hd: i32,
        xs: i32,
        ys: i32,
        ws: i32,
        hs: i32,
        bits: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        rop: u32,
    ) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Hinstance;
}

const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
const CS_HREDRAW: u32 = 0x0002;
const CS_VREDRAW: u32 = 0x0001;
const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000F;
const WM_SIZE: u32 = 0x0005;
const WM_KEYDOWN: u32 = 0x0100;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_CHAR: u32 = 0x0102;
const WM_DPICHANGED: u32 = 0x02E0;
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_MATH_ATOMS_COMMAND: u32 = 0x804A;
const TME_LEAVE: u32 = 0x0000_0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_NOMOVE: u32 = 0x0002;
const DPI_PER_MONITOR_V2: isize = -4;
const BI_RGB: u32 = 0;
const DIB_RGB_COLORS: u32 = 0;
const SRCCOPY: u32 = 0x00CC_0020;
const IDC_ARROW: usize = 32512;

struct App {
    width: u32,
    height: u32,
    ui: UiState,
    model: NativeApp,
    cursor: (f32, f32),
    quality: Quality,
    tracking_leave: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_PER_MONITOR_V2);
        let class_name = wide("math_atoms_native_window");
        let title = wide("Math Atoms Coder - Native PMRE");
        let hinst = GetModuleHandleW(std::ptr::null());
        let wc = WndClassW {
            style: CS_HREDRAW | CS_VREDRAW,
            proc_: Some(wndproc),
            cls_extra: 0,
            wnd_extra: 0,
            instance: hinst,
            icon: std::ptr::null_mut(),
            cursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW as *const u16),
            background: std::ptr::null_mut(),
            menu_name: std::ptr::null(),
            class_name: class_name.as_ptr(),
        };
        RegisterClassW(&wc);

        APP.with(|cell| {
            let model = NativeApp::from_process_env();
            let mut state = UiState::new(1240, 760);
            model.seed_input(&mut state);
            *cell.borrow_mut() = Some(App {
                width: 1240,
                height: 760,
                ui: state,
                model,
                cursor: (0.0, 0.0),
                quality: Quality::Fast,
                tracking_leave: false,
            });
        });

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1280,
            840,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            eprintln!("CreateWindowExW failed");
            return;
        }
        let dpi = GetDpiForWindow(hwnd);
        let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                app.ui.scale = scale;
            }
        });
        if (scale - 1.0).abs() > 1e-3 {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                (1280.0 * scale) as i32,
                (840.0 * scale) as i32,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE,
            );
        }
        InvalidateRect(hwnd, std::ptr::null(), 0);
        let mut msg: Msg = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: Hwnd, msg: u32, wp: Wparam, lp: Lparam) -> Lresult {
    match msg {
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        WM_SIZE => {
            let w = (lp & 0xFFFF) as u32;
            let h = ((lp >> 16) & 0xFFFF) as u32;
            APP.with(|cell| {
                if let Some(app) = cell.borrow_mut().as_mut() {
                    app.width = w.max(1);
                    app.height = h.max(1);
                }
            });
            dispatch(UiEvent::Resize(w.max(1), h.max(1)));
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_LBUTTONUP => {
            if msg == WM_LBUTTONDOWN {
                SetCapture(hwnd);
            } else if msg == WM_LBUTTONUP {
                ReleaseCapture();
            }
            let scale = APP
                .with(|cell| {
                    cell.borrow()
                        .as_ref()
                        .map(|app| app.ui.scale)
                        .unwrap_or(1.0)
                })
                .max(0.1);
            let x = lo(lp) / scale;
            let y = hi(lp) / scale;
            if arm_mouse_leave(x, y) {
                let mut tme = TrackMouseEventOpts {
                    size: std::mem::size_of::<TrackMouseEventOpts>() as u32,
                    flags: TME_LEAVE,
                    hwnd,
                    hover_time: 0,
                };
                TrackMouseEvent(&mut tme);
            }
            let dirty = dispatch(match msg {
                WM_LBUTTONDOWN => UiEvent::PointerDown(x, y),
                WM_LBUTTONUP => UiEvent::PointerUp(x, y),
                _ => UiEvent::PointerMove(x, y),
            });
            if dirty {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_MOUSELEAVE => {
            APP.with(|cell| {
                if let Some(app) = cell.borrow_mut().as_mut() {
                    app.tracking_leave = false;
                }
            });
            if dispatch(UiEvent::PointerMove(-1e6, -1e6)) {
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_MOUSEWHEEL => {
            let delta = (((wp >> 16) & 0xFFFF) as i16) as f32 / 120.0;
            let cursor = APP.with(|cell| {
                cell.borrow()
                    .as_ref()
                    .map(|app| app.cursor)
                    .unwrap_or((0.0, 0.0))
            });
            dispatch(UiEvent::Wheel(cursor.0, cursor.1, -delta * 48.0));
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_DPICHANGED => {
            let dpi = (wp & 0xFFFF) as f32;
            APP.with(|cell| {
                if let Some(app) = cell.borrow_mut().as_mut() {
                    app.ui.scale = (dpi / 96.0).max(0.1);
                }
            });
            let r = lp as *const Rect;
            if !r.is_null() {
                let r = &*r;
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_CHAR => {
            let code = wp as u32;
            let ev = match code {
                8 => Some(UiEvent::Backspace),
                13 => Some(UiEvent::Enter),
                _ => char::from_u32(code)
                    .filter(|ch| !ch.is_control())
                    .map(UiEvent::Char),
            };
            if let Some(ev) = ev {
                dispatch(ev);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
            0
        }
        WM_MATH_ATOMS_COMMAND => {
            dispatch_command(wp as u32);
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_KEYDOWN => {
            APP.with(|cell| {
                if let Some(app) = cell.borrow_mut().as_mut() {
                    if app.ui.focused.is_some() {
                        return;
                    }
                    app.quality = match wp {
                        0x31 => Quality::Fast,
                        0x32 => Quality::Balanced,
                        0x33 => Quality::Full,
                        0x38 => Quality::TiledBalanced,
                        0x39 => Quality::TiledFull,
                        _ => app.quality,
                    };
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_PAINT => {
            paint(hwnd);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn dispatch(ev: UiEvent) -> bool {
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            let was_move = matches!(ev, UiEvent::PointerMove(..));
            let before = (app.ui.hover, app.ui.pressed, app.ui.drag);
            {
                let build = |state: &UiState| ui::build(&app.model, state);
                handle_event(&mut app.ui, &build, ev);
            }
            let mut dirty = !was_move || before != (app.ui.hover, app.ui.pressed, app.ui.drag);
            if app.ui.drag.is_some() {
                dirty = true;
            }
            if let Some(id) = app.ui.take_click() {
                dispatch_model_command(app, id);
                dirty = true;
            }
            if app.ui.take_submit().is_some() {
                app.model.run_current_intent(&app.ui);
                dirty = true;
            }
            dirty
        } else {
            false
        }
    })
}

fn dispatch_command(id: u32) {
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            dispatch_model_command(app, id);
        }
    });
}

fn dispatch_model_command(app: &mut App, id: u32) {
    match id {
        RUN_LOOP => app.model.run_current_intent(&app.ui),
        EXEC_PROVIDER => app.model.execute_provider(),
        CAPTURE_PROOF => app.model.capture_current_proof(),
        MARK_DRIFT => app.model.mark_drift(),
        _ => {}
    }
}

fn paint(hwnd: Hwnd) {
    unsafe {
        let mut ps: PaintStruct = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        APP.with(|cell| {
            if let Some(app) = cell.borrow_mut().as_mut() {
                let started = std::time::Instant::now();
                let build: &dyn Fn(&UiState) -> UxNode = &|state| ui::build(&app.model, state);
                let fb = render_ui_quality(build, &app.ui, ui::background(), app.quality);
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                let px = fb.to_u32(ui::background());
                let width = app.width as i32;
                let height = app.height as i32;
                let bmi = BitmapInfo {
                    header: BitmapInfoHeader {
                        size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                        width,
                        height: -height,
                        planes: 1,
                        bit_count: 32,
                        compression: BI_RGB,
                        size_image: 0,
                        x_ppm: 0,
                        y_ppm: 0,
                        clr_used: 0,
                        clr_important: 0,
                    },
                    colors: [0],
                };
                StretchDIBits(
                    hdc,
                    0,
                    0,
                    width,
                    height,
                    0,
                    0,
                    width,
                    height,
                    px.as_ptr() as *const c_void,
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                let title = format!(
                    "Math Atoms Coder - Native PMRE [{:.1}ms {} {} {}]",
                    ms,
                    app.model.status().as_str(),
                    app.model.provider_title_state(),
                    app.model.runtime.state().selected_recipe
                );
                SetWindowTextW(hwnd, wide(&title).as_ptr());
            }
        });
        EndPaint(hwnd, &ps);
    }
}

fn arm_mouse_leave(x: f32, y: f32) -> bool {
    APP.with(|cell| {
        if let Some(app) = cell.borrow_mut().as_mut() {
            app.cursor = (x, y);
            if !app.tracking_leave {
                app.tracking_leave = true;
                return true;
            }
        }
        false
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn lo(value: Lparam) -> f32 {
    ((value & 0xFFFF) as i16) as f32
}

fn hi(value: Lparam) -> f32 {
    (((value >> 16) & 0xFFFF) as i16) as f32
}
