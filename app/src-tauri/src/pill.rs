//! Explorer pill: one small glassy toggle per File Explorer window showing how
//! many items in that window's current folder carry a note (E2, count mode).
//!
//! Clicking it is logged and otherwise does nothing yet — drawing dots over the
//! annotated items is E3.
//!
//! **Rendering is GDI, not a webview.** A `WebviewWindowBuilder` pill would be
//! the cheap way to reuse the app's acrylic styling, but each one starts a
//! WebView2 process tree (tens of MB) and there is one pill per Explorer
//! window, which a user can easily have five of — that alone would blow the
//! whole-app RAM budget in docs/ARCHITECTURE.md. So each pill is a layered
//! window pushed with `UpdateLayeredWindow`, exactly like `badges.rs`, and the
//! cost is hand-drawn everything: the rounded rect, its border, the accent dot
//! and the digits are composited per pixel here. A pill's bitmap is ~60×28 px,
//! so a pill costs a few KB plus one HWND rather than a browser.
//!
//! The "glass" is a translucent fill, not real acrylic: `UpdateLayeredWindow`
//! and DWM's blur-behind do not compose, and buying real blur means buying a
//! webview back. The fill alpha, border and accent match the overlay panel's
//! tokens so the two read as the same surface.
//!
//! **Z-order is ownership, not TOPMOST** (E0 verdict C, and the taskbar lesson
//! from PR #31): `SetWindowLongPtrW(GWLP_HWNDPARENT)` makes the Explorer window
//! the pill's owner, so the pill floats above that window only, auto-hides when
//! it is minimized, and stays below any unrelated app raised over Explorer.
//! An owned popup does NOT follow its owner's moves and does NOT die with it,
//! so a `LOCATIONCHANGE` WinEvent hook repositions it and a liveness check
//! destroys it once the owner is gone.
//!
//! **Idle cost.** With no Explorer window and none foreground there is no pill
//! and no timer at all. New windows are discovered from
//! `EVENT_SYSTEM_FOREGROUND`; while one is foreground the tick runs at
//! `FAST_MS` and re-enumerates the shell (folder and tab changes are poll-only
//! — no event reports either), and it drops to `SLOW_MS` when focus leaves,
//! where it only checks the owners are still alive and re-places the pills.
//! Pause and badges-off destroy every pill and stop the tick outright; the next
//! Explorer foreground event brings them back.
//!
//! The foreground tick stays armed even when a pass produced no pill: a window
//! opened from our own main-window "Open" button is already foreground before
//! its shell view exists, so the first sync sees nothing and the pill would
//! never appear until the user clicked away and back.
//!
//! Window moves are a separate, cheaper path (`MOVE_TIMER` → `cheap_pass`):
//! nothing about a drag can change which windows exist or what is in their
//! folders, and running the shell enumeration at drag cadence is what made the
//! pill visibly trail the window.
//!
//! The count itself is `storage::count_notes_in_folder` — a read of that one
//! folder. No UIA and no item enumeration is involved in count mode; that is
//! E3's cost, paid on click.
//!
//! Accessibility: font-size preset, panel scale, theme and high contrast come
//! from settings on every redraw, and per-monitor DPI from the owner window.
//! Reduced Motion needs nothing — the pill has no animation by construction.
//!
//! Windows-only (cfg-gated in main.rs). The macOS equivalent of Explorer badges
//! is Finder tags (MEMORY.md decision 2), which need no window at all.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};

use tauri::AppHandle;
use windows::core::*;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Accessibility::{
    SetWinEventHook, HCF_HIGHCONTRASTON, HIGHCONTRASTW, HWINEVENTHOOK,
};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::appstate::Paused;
use crate::desktop::{self, ExplorerWindow};
use crate::{settings, storage};

/// Poll cadence while an Explorer window is foreground (folder + tab changes
/// have no event to subscribe to) and while none is (liveness only).
const FAST_MS: u32 = 700;
const SLOW_MS: u32 = 2000;
const TICK_TIMER: usize = 1;
/// One-shot timer for `LOCATIONCHANGE` bursts (a window drag fires dozens per
/// second). It does placement only — never a shell call.
const MOVE_TIMER: usize = 2;
/// How closely a pill tracks a window being dragged. Re-arming an already-armed
/// timer would just reset it, so a continuous drag would keep pushing the fire
/// out and the pill would only catch up when the drag paused — hence
/// `MOVE_ARMED` below, which lets it fire at this cadence throughout.
const MOVE_DELAY_MS: u32 = 30;
/// One-shot timer for events that need the full shell re-enumeration: a
/// foreground switch, or a note being saved.
const FULL_TIMER: usize = 3;
const FULL_DELAY_MS: u32 = 60;

/// Gap between the pill and the content area's bottom/right edges, in logical
/// px (scaled by the owner's DPI). The scrollbar width is subtracted on top.
const CONTENT_INSET: i32 = 12;

const PILL_CLASS: PCWSTR = w!("TofuNuggetsExplorerPill");
const MGR_CLASS: PCWSTR = w!("TofuNuggetsPillManager");

/// Manager window handle, for the WinEvent callbacks (single instance).
static MGR_HWND: AtomicIsize = AtomicIsize::new(0);
/// Whether `MOVE_TIMER` is already armed. Without this the reposition timer
/// starves during a drag (see `MOVE_DELAY_MS`).
static MOVE_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How many pills exist, readable from the hook threads. The move hook is
/// pure overhead when there is nothing to re-place, and `EVENT_OBJECT_LOCATIONCHANGE`
/// fires for every window on the desktop, not just Explorer's.
static PILL_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Set by the foreground hook: the next sync must re-enumerate the shell
/// instead of taking the cheap unfocused path. A window that just closed or
/// just opened is only visible to the enumeration.
static FORCE_FULL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn spawn(app: AppHandle, paused: Paused, settings: settings::Shared) {
    std::thread::Builder::new()
        .name("explorer-pill".into())
        .spawn(move || {
            // The shell chain (IShellWindows -> IShellBrowser -> IFolderView2)
            // is STA-only; from an MTA thread it fails with 0x8001010D and
            // resolves nothing (E1, the hard way).
            desktop::init_com_for_thread();
            if let Err(e) = run(app, paused, settings) {
                eprintln!("explorer pill layer failed: {e}");
            }
        })
        .expect("spawn explorer pill layer");
}

// ---------------------------------------------------------------- state ----

struct Pill {
    /// Owner: the top-level CabinetWClass window.
    top: HWND,
    /// Active tab's shell view; its screen rect is the content area.
    view: HWND,
    hwnd: HWND,
    folder: Option<PathBuf>,
    count: usize,
    /// Last pushed appearance + placement; a tick that changes neither skips
    /// the redraw entirely.
    drawn: Option<(usize, Style, RECT)>,
}

struct Mgr {
    app: AppHandle,
    paused: Paused,
    settings: settings::Shared,
    pills: Vec<Pill>,
    /// Whether the tick timer is currently armed, and at which cadence.
    tick: Option<u32>,
    /// Ticks of grace left after a foreground change. A window that just opened
    /// is not immediately enumerable — the shell needs a moment to build its
    /// browser and view — so a sync fired right after the event legitimately
    /// finds nothing. Without this the layer concluded "nothing to watch",
    /// disarmed, and the pill only turned up if another foreground event
    /// happened to land later (owner-reported: no pill after the main window's
    /// Open button until you clicked away and back).
    settle: u8,
}

/// Foreground-change grace: how many `FAST_MS` ticks to keep looking.
const SETTLE_TICKS: u8 = 5;

fn run(app: AppHandle, paused: Paused, settings: settings::Shared) -> Result<()> {
    unsafe {
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(mgr_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: MGR_CLASS,
            ..Default::default()
        });
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(pill_wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PILL_CLASS,
            hCursor: LoadCursorW(None, IDC_HAND).unwrap_or_default(),
            ..Default::default()
        });

        // Message-only window: it exists to own the timers and the pill list.
        let mgr_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            MGR_CLASS,
            w!("Tofu Nuggets pill manager"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )?;
        MGR_HWND.store(mgr_hwnd.0 as isize, Ordering::Release);

        let mgr = Box::into_raw(Box::new(Mgr {
            app,
            paused,
            settings,
            pills: Vec::new(),
            tick: None,
            settle: SETTLE_TICKS,
        }));
        SetWindowLongPtrW(mgr_hwnd, GWLP_USERDATA, mgr as isize);

        // Hooks get their own thread, matching badges.rs: a WinEvent callback
        // on a thread that also makes cross-process COM calls invites
        // reentrancy while the shell chain is marshalling.
        std::thread::Builder::new()
            .name("pill-hooks".into())
            .spawn(|| {
                let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
                let _fg: HWINEVENTHOOK = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(fg_event),
                    0,
                    0,
                    flags,
                );
                let _loc: HWINEVENTHOOK = SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(move_event),
                    0,
                    0,
                    flags,
                );
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            })
            .expect("spawn pill hooks");

        sync(mgr_hwnd, &mut *mgr);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

// --------------------------------------------------------------- events ----

/// Foreground changes are how a brand-new Explorer window is discovered (it
/// always becomes foreground when it opens) and how the tick cadence is chosen.
unsafe extern "system" fn fg_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    idobject: i32,
    idchild: i32,
    _thread: u32,
    _time: u32,
) {
    if idobject != OBJID_WINDOW.0 || idchild != 0 || hwnd.is_invalid() {
        return;
    }
    // Filter by class here rather than in the sync. Foreground changes are
    // constant on a busy desktop, and an unfiltered hook opened the
    // shell-enumeration grace window (see `settle`) on every one of them —
    // measurably burning CPU with no Explorer window open anywhere.
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    if String::from_utf16_lossy(&buf[..n]) == "CabinetWClass" {
        // Explorer gained focus: it may be a window that opened this instant
        // and is not enumerable yet, so ask for the grace window.
        FORCE_FULL.store(true, Ordering::Release);
        arm_full();
    } else if PILL_COUNT.load(Ordering::Acquire) > 0 {
        // Focus went elsewhere while pills exist: worth one pass to drop the
        // tick cadence and re-place them, but nothing needs re-enumerating.
        arm_full();
    }
}

/// Ask for a full sync shortly. Coalescing here is plain timer reset: bursts of
/// foreground changes should collapse into one enumeration.
fn arm_full() {
    let mgr = MGR_HWND.load(Ordering::Acquire);
    if mgr != 0 {
        unsafe {
            SetTimer(
                Some(HWND(mgr as *mut core::ffi::c_void)),
                FULL_TIMER,
                FULL_DELAY_MS,
                None,
            );
        }
    }
}

/// An owned popup does not follow its owner's moves (E0 verdict C), so every
/// top-level move/resize re-places the pills.
unsafe extern "system" fn move_event(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    idobject: i32,
    idchild: i32,
    _thread: u32,
    _time: u32,
) {
    if idobject != OBJID_WINDOW.0 || idchild != 0 || hwnd.is_invalid() {
        return;
    }
    if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
        return;
    }
    if PILL_COUNT.load(Ordering::Acquire) == 0 {
        return;
    }
    let mgr = MGR_HWND.load(Ordering::Acquire);
    if mgr != 0 && !MOVE_ARMED.swap(true, Ordering::AcqRel) {
        unsafe {
            SetTimer(
                Some(HWND(mgr as *mut core::ffi::c_void)),
                MOVE_TIMER,
                MOVE_DELAY_MS,
                None,
            );
        }
    }
}

unsafe extern "system" fn mgr_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == TICK_TIMER => {
            with_mgr(hwnd, |m| sync(hwnd, m));
            LRESULT(0)
        }
        // A window moved or resized. Placement only: nothing about a drag can
        // change which windows exist or what is in their folders, and running
        // the shell enumeration at drag cadence is what made the pill lag
        // behind the window.
        WM_TIMER if wp.0 == MOVE_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), MOVE_TIMER);
            }
            MOVE_ARMED.store(false, Ordering::Release);
            with_mgr(hwnd, cheap_pass);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == FULL_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), FULL_TIMER);
            }
            with_mgr(hwnd, |m| sync(hwnd, m));
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn with_mgr(hwnd: HWND, f: impl FnOnce(&mut Mgr)) {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Mgr;
    if !p.is_null() {
        f(unsafe { &mut *p });
    }
}

/// E2 is count mode only: a click is acknowledged in the log and nothing else
/// happens. E3 turns this into the dots snapshot.
unsafe extern "system" fn pill_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_LBUTTONUP => {
            let mgr = MGR_HWND.load(Ordering::Acquire);
            if mgr != 0 {
                with_mgr(HWND(mgr as *mut core::ffi::c_void), |m| {
                    let folder = m
                        .pills
                        .iter()
                        .find(|p| p.hwnd == hwnd)
                        .and_then(|p| p.folder.as_ref())
                        .map(|f| f.display().to_string())
                        .unwrap_or_else(|| "<unknown>".into());
                    crate::logfile::log(
                        &m.app,
                        &format!("pill clicked for '{folder}' (dots land in E3)"),
                    );
                });
            }
            LRESULT(0)
        }
        // Owned + WS_EX_NOACTIVATE already keeps focus in Explorer; this stops
        // the click from raising the pill's owner as a side effect.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

// ----------------------------------------------------------------- sync ----

/// Reconcile the pill set with the live Explorer windows, then redraw whatever
/// changed. The single entry point for every trigger (tick, move, foreground).
fn sync(mgr_hwnd: HWND, m: &mut Mgr) {
    let badges_on = m.settings.lock().map(|s| s.badges).unwrap_or(true);
    if m.paused.is_paused() || !badges_on {
        // Nothing to show and nothing to watch: tear down completely rather
        // than idle a timer. The next Explorer foreground event restarts us,
        // which is also the first moment a pill could be seen again.
        destroy_all(m);
        PILL_COUNT.store(0, Ordering::Release);
        set_tick(mgr_hwnd, m, None);
        return;
    }

    let fg = desktop::explorer_is_foreground();
    // Cheap class-only scan (no COM): what Explorer windows exist right now.
    let tops = desktop::explorer_top_windows();

    // A foreground change (or a note being saved) opens a grace window: the
    // thing it is telling us about may not be enumerable yet.
    if FORCE_FULL.swap(false, Ordering::AcqRel) {
        m.settle = SETTLE_TICKS;
    }
    // The expensive shell enumeration is worth running when the user is looking
    // at Explorer, when a window exists that we have no pill for yet, when a
    // pill outlived its window, or while a grace window is open. Otherwise
    // nothing it could discover has changed: navigation and tab switches both
    // need focus.
    let full = fg
        || m.settle > 0
        || tops.iter().any(|t| !m.pills.iter().any(|p| p.top == *t))
        || m.pills.iter().any(|p| !tops.contains(&p.top));
    // The grace window is spent by looking, not by the clock, so this counts
    // down here and nowhere else — an earlier version decremented only on one
    // of two exit paths and the tick then never disarmed at all.
    m.settle = m.settle.saturating_sub(1);

    if full {
        let windows = desktop::explorer_windows();
        // Drop pills whose owner is gone or no longer reports a folder. An
        // owned popup survives its owner's death (E0), so this is what prevents
        // orphans.
        let live: Vec<isize> = windows.iter().map(|w| w.top.0 as isize).collect();
        m.pills.retain(|p| {
            let keep =
                unsafe { IsWindow(Some(p.top)).as_bool() } && live.contains(&(p.top.0 as isize));
            if !keep {
                unsafe {
                    let _ = DestroyWindow(p.hwnd);
                }
            }
            keep
        });
        for w in &windows {
            upsert(m, w);
        }
    } else {
        cheap_pass(m);
    }

    // Cadence keys off Explorer windows EXISTING, not off pills existing. A
    // window opened from our own main-window "Open" button is listed by the
    // class scan immediately but has no shell view for a moment, so the first
    // sync produces no pill; keying the timer off pills left it that way until
    // the user clicked away and back (owner-reported). Once no Explorer window
    // is open at all there is again no timer whatsoever, which is what the idle
    // budget asks for.
    PILL_COUNT.store(m.pills.len(), Ordering::Release);
    let cadence = if fg || m.settle > 0 {
        Some(FAST_MS)
    } else if tops.is_empty() && m.pills.is_empty() {
        None
    } else {
        Some(SLOW_MS)
    };
    set_tick(mgr_hwnd, m, cadence);
}

fn set_tick(mgr_hwnd: HWND, m: &mut Mgr, ms: Option<u32>) {
    if m.tick == ms {
        return;
    }
    unsafe {
        match ms {
            Some(ms) => {
                SetTimer(Some(mgr_hwnd), TICK_TIMER, ms, None);
            }
            None => {
                let _ = KillTimer(Some(mgr_hwnd), TICK_TIMER);
            }
        }
    }
    m.tick = ms;
}

/// A note was written or deleted: counts may be stale even though nothing about
/// the windows changed. Called from the editor, which by definition took focus
/// away from the Explorer window whose pill needs updating, so the unfocused
/// path would otherwise not notice until the user clicked back.
pub fn notes_changed() {
    FORCE_FULL.store(true, Ordering::Release);
    arm_full();
}

/// Unfocused upkeep, deliberately free of shell calls and of disk reads: drop
/// pills whose owner died and re-place the rest. Counts do not change here —
/// every way they can change (navigation, tab switch, a note being saved) has
/// its own trigger back into the full path.
fn cheap_pass(m: &mut Mgr) {
    m.pills.retain(|p| {
        let keep = unsafe { IsWindow(Some(p.top)).as_bool() };
        if !keep {
            unsafe {
                let _ = DestroyWindow(p.hwnd);
            }
        }
        keep
    });
    PILL_COUNT.store(m.pills.len(), Ordering::Release);
    for idx in 0..m.pills.len() {
        render(m, idx);
    }
}

fn destroy_all(m: &mut Mgr) {
    for p in m.pills.drain(..) {
        unsafe {
            let _ = DestroyWindow(p.hwnd);
        }
    }
}

/// Create or refresh the pill for one Explorer window.
fn upsert(m: &mut Mgr, w: &ExplorerWindow) {
    let idx = match m.pills.iter().position(|p| p.top == w.top) {
        Some(i) => i,
        None => {
            let Some(hwnd) = create_pill_window(w.top) else {
                return;
            };
            m.pills.push(Pill {
                top: w.top,
                view: w.view,
                hwnd,
                folder: None,
                count: 0,
                drawn: None,
            });
            m.pills.len() - 1
        }
    };

    // Navigation and tab switches both surface here as a folder change (no
    // event exists for either — hence the poll). The count is re-read every
    // time regardless: notes also appear and vanish under a folder that never
    // moved.
    m.pills[idx].folder = w.folder.clone();
    // A non-filesystem tab (This PC, and that is what Ctrl+T opens on) counts
    // as nothing to show, not as "keep the last number".
    m.pills[idx].count = w
        .folder
        .as_ref()
        .map(|f| storage::count_notes_in_folder(f))
        .unwrap_or(0);
    m.pills[idx].view = w.view;
    render(m, idx);
}

/// Push one pill's current count to screen, or hide it when there is nothing
/// to show. Skips the composite entirely when count, style and placement are
/// all unchanged, which is the common case on every tick.
fn render(m: &mut Mgr, idx: usize) {
    let style = Style::current(&m.settings, m.pills[idx].top);
    let count = m.pills[idx].count;
    let hwnd = m.pills[idx].hwnd;

    // Empty folder: no pill. A "0" chip in every Explorer window is noise, and
    // in count mode there is nothing behind it to reveal.
    if count == 0 {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        m.pills[idx].drawn = None;
        return;
    }

    let Some(anchor) = content_rect(m.pills[idx].view) else {
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        m.pills[idx].drawn = None;
        return;
    };

    if m.pills[idx].drawn == Some((count, style, anchor)) {
        return;
    }
    if draw_pill(hwnd, count, style, anchor) {
        m.pills[idx].drawn = Some((count, style, anchor));
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

/// Bare owned popup, per E0 verdict C. Not TOPMOST — ownership alone puts it
/// above its Explorer window and nowhere else.
fn create_pill_window(owner: HWND) -> Option<HWND> {
    unsafe {
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()?;
        let hwnd = CreateWindowExW(
            // No WS_EX_TRANSPARENT: unlike the badge layer this one is a
            // toggle and must take its own clicks (E3).
            WS_EX_LAYERED | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            PILL_CLASS,
            w!("Tofu Nuggets pill"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
        .ok()?;
        SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner.0 as isize);
        Some(hwnd)
    }
}

/// Screen rect of the active tab's content area — the shell view window, which
/// already excludes the toolbar, the navigation pane and the status bar.
fn content_rect(view: HWND) -> Option<RECT> {
    unsafe {
        if !IsWindow(Some(view)).as_bool() || IsIconic(GetAncestor(view, GA_ROOT)).as_bool() {
            return None;
        }
        let mut rc = RECT::default();
        GetWindowRect(view, &mut rc).ok()?;
        if rc.right - rc.left < 40 || rc.bottom - rc.top < 40 {
            return None;
        }
        Some(rc)
    }
}

// ---------------------------------------------------------------- style ----

#[derive(Clone, Copy, PartialEq)]
struct Style {
    dark: bool,
    high_contrast: bool,
    /// Owner window's DPI, kept raw for system metrics (the scrollbar width).
    dpi: u32,
    /// Font-size preset × panel scale × DPI, folded into one multiplier.
    scale_milli: u32,
}

impl Style {
    fn current(settings: &settings::Shared, owner: HWND) -> Self {
        let s = settings.lock().ok().map(|g| g.clone()).unwrap_or_default();
        let font = match s.font_size.as_str() {
            "s" => 0.9,
            "l" => 1.15,
            "xl" => 1.35,
            _ => 1.0,
        };
        let dpi = unsafe { GetDpiForWindow(owner) }.max(96);
        let scale = font * s.panel_scale * (dpi as f64 / 96.0);
        Style {
            dark: match s.theme.as_str() {
                "dark" => true,
                "light" => false,
                _ => system_prefers_dark(),
            },
            high_contrast: s.high_contrast || system_high_contrast(),
            dpi,
            // Quantized so float noise can't defeat the "unchanged, skip the
            // redraw" comparison.
            scale_milli: (scale * 1000.0).round().clamp(500.0, 5000.0) as u32,
        }
    }

    fn scale(&self) -> f32 {
        self.scale_milli as f32 / 1000.0
    }

    fn px(&self, logical: f32) -> i32 {
        (logical * self.scale()).round().max(1.0) as i32
    }

    /// (fill, border, text, accent) as RGBA, straight (not premultiplied).
    fn colors(&self) -> ([u8; 4], [u8; 4], [u8; 4], [u8; 4]) {
        if self.high_contrast {
            // High contrast means system colors at full opacity — no glass, no
            // translucency, nothing the user's scheme did not choose.
            let fill = sys_rgba(COLOR_WINDOW);
            let text = sys_rgba(COLOR_WINDOWTEXT);
            let accent = sys_rgba(COLOR_HIGHLIGHT);
            return (fill, text, text, accent);
        }
        let accent = [0xF5, 0x8F, 0x3C, 0xFF]; // same warm accent as the badges
        if self.dark {
            (
                [0x1C, 0x1C, 0x20, 0xD6],
                [0xFF, 0xFF, 0xFF, 0x2E],
                [0xF0, 0xF0, 0xF5, 0xFF],
                accent,
            )
        } else {
            (
                [0xFA, 0xFA, 0xFC, 0xD6],
                [0x00, 0x00, 0x00, 0x24],
                [0x20, 0x20, 0x24, 0xFF],
                accent,
            )
        }
    }
}

fn sys_rgba(idx: SYS_COLOR_INDEX) -> [u8; 4] {
    let c = unsafe { GetSysColor(idx) };
    [
        (c & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        ((c >> 16) & 0xFF) as u8,
        0xFF,
    ]
}

/// The shell's app theme. `ShouldAppsUseDarkMode` is undocumented ordinal-only
/// API; the registry value it reads is the stable way to ask.
fn system_prefers_dark() -> bool {
    use windows::Win32::System::Registry::*;
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut _ as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    ok.is_ok() && value == 0
}

fn system_high_contrast() -> bool {
    let mut hc = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            Some(&mut hc as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_ok() && (hc.dwFlags.0 & HCF_HIGHCONTRASTON.0) != 0
}

// ----------------------------------------------------------------- draw ----

/// Composite the pill and push it with `UpdateLayeredWindow`, which sets the
/// window's position and size in the same call. Returns whether it succeeded.
fn draw_pill(hwnd: HWND, count: usize, st: Style, anchor: RECT) -> bool {
    let text: Vec<u16> = count
        .to_string()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let screen_dc = GetDC(None);
        let mem_dc = CreateCompatibleDC(Some(screen_dc));

        let font = CreateFontW(
            -st.px(13.0),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            // Grayscale AA, not ClearType: subpixel rendering assumes an opaque
            // known background, and this bitmap is translucent.
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(mem_dc, font.into());

        let mut ext = SIZE::default();
        let _ = GetTextExtentPoint32W(mem_dc, &text[..text.len() - 1], &mut ext);

        let pad = st.px(9.0);
        let gap = st.px(6.0);
        let dot_r = st.px(3.5);
        let h = st.px(24.0).max(ext.cy + st.px(6.0));
        let w = pad * 2 + dot_r * 2 + gap + ext.cx;

        // Bottom-right of the content area, clear of the vertical scrollbar.
        let inset = st.px(CONTENT_INSET as f32);
        let sb = GetSystemMetricsForDpi(SM_CXVSCROLL, st.dpi);
        let x = anchor.right - sb - inset - w;
        let y = anchor.bottom - inset - h;
        if x < anchor.left || y < anchor.top {
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return false;
        }

        // Target bitmap (premultiplied BGRA, top-down) and a second one used
        // purely as a text coverage mask: GDI writes no alpha, so the glyphs
        // are rendered white-on-black and their brightness read back as
        // coverage to composite with.
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let Some(bmp) = dib(mem_dc, w, h, &mut bits) else {
            SelectObject(mem_dc, old_font);
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            return false;
        };

        let mask_dc = CreateCompatibleDC(Some(screen_dc));
        let mut mask_bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let mask_bmp = dib(mask_dc, w, h, &mut mask_bits);
        let mut mask: Vec<u8> = vec![0; (w * h) as usize];
        if let Some(mb) = mask_bmp {
            let old = SelectObject(mask_dc, mb.into());
            let old_f = SelectObject(mask_dc, font.into());
            std::slice::from_raw_parts_mut(mask_bits as *mut u32, (w * h) as usize).fill(0);
            SetBkMode(mask_dc, TRANSPARENT);
            SetTextColor(mask_dc, COLORREF(0x00FF_FFFF));
            let tx = pad + dot_r * 2 + gap;
            let ty = (h - ext.cy) / 2;
            let _ = TextOutW(mask_dc, tx, ty, &text[..text.len() - 1]);
            let src = std::slice::from_raw_parts(mask_bits as *const u32, (w * h) as usize);
            for (i, p) in src.iter().enumerate() {
                let (r, g, b) = ((p >> 16) & 0xFF, (p >> 8) & 0xFF, p & 0xFF);
                mask[i] = r.max(g).max(b) as u8;
            }
            SelectObject(mask_dc, old_f);
            SelectObject(mask_dc, old);
            let _ = DeleteObject(mb.into());
        }
        let _ = DeleteDC(mask_dc);

        let old_bmp = SelectObject(mem_dc, bmp.into());
        let px = std::slice::from_raw_parts_mut(bits as *mut u32, (w * h) as usize);
        compose(px, &mask, w, h, dot_r, pad, st);

        let ok = UpdateLayeredWindow(
            hwnd,
            Some(screen_dc),
            Some(&POINT { x, y }),
            Some(&SIZE { cx: w, cy: h }),
            Some(mem_dc),
            Some(&POINT { x: 0, y: 0 }),
            COLORREF(0),
            Some(&BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
                ..Default::default()
            }),
            ULW_ALPHA,
        )
        .is_ok();

        SelectObject(mem_dc, old_bmp);
        SelectObject(mem_dc, old_font);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(None, screen_dc);
        ok
    }
}

fn dib(dc: HDC, w: i32, h: i32, bits: &mut *mut core::ffi::c_void) -> Option<HBITMAP> {
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe { CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, bits, None, 0) }.ok()
}

/// Rounded rect + 1 px border + accent dot + the digits, all composited by
/// hand into premultiplied BGRA. Coverage comes from a signed distance to the
/// rounded rect, which gives the same 1 px anti-aliased rim the badge dots use.
fn compose(px: &mut [u32], mask: &[u8], w: i32, h: i32, dot_r: i32, pad: i32, st: Style) {
    let (fill, border, text, accent) = st.colors();
    let radius = h as f32 / 2.0;
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let bw = st.px(1.0) as f32;
    let dot_cx = pad as f32 + dot_r as f32;
    let dot_cy = hh;

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 + 0.5 - hw;
            let fy = y as f32 + 0.5 - hh;
            let qx = (fx.abs() - (hw - radius)).max(0.0);
            let qy = (fy.abs() - (hh - radius)).max(0.0);
            let d = (qx * qx + qy * qy).sqrt() - radius;

            let outer = (0.5 - d).clamp(0.0, 1.0);
            if outer <= 0.0 {
                px[(y * w + x) as usize] = 0;
                continue;
            }
            let inner = (0.5 - (d + bw)).clamp(0.0, 1.0);

            let mut acc = [0f32; 4]; // premultiplied RGBA
            over(&mut acc, fill, fill[3] as f32 / 255.0 * inner);
            over(&mut acc, border, border[3] as f32 / 255.0 * (outer - inner));

            let dd = ((fx + hw - dot_cx).powi(2) + (fy + hh - dot_cy).powi(2)).sqrt();
            let dot_cov = (dot_r as f32 + 0.5 - dd).clamp(0.0, 1.0);
            if dot_cov > 0.0 {
                over(&mut acc, accent, dot_cov);
            }

            let tc = mask[(y * w + x) as usize] as f32 / 255.0;
            if tc > 0.0 {
                over(&mut acc, text, tc);
            }

            // Clip everything to the pill's anti-aliased silhouette.
            let clip = outer;
            let a = (acc[3] * clip * 255.0).round().clamp(0.0, 255.0) as u32;
            let r = (acc[0] * clip * 255.0).round().clamp(0.0, 255.0) as u32;
            let g = (acc[1] * clip * 255.0).round().clamp(0.0, 255.0) as u32;
            let b = (acc[2] * clip * 255.0).round().clamp(0.0, 255.0) as u32;
            px[(y * w + x) as usize] = (a << 24) | (r << 16) | (g << 8) | b;
        }
    }
}

/// Source-over of a straight-alpha color onto a premultiplied accumulator.
fn over(dst: &mut [f32; 4], src: [u8; 4], alpha: f32) {
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let inv = 1.0 - a;
    for i in 0..3 {
        dst[i] = (src[i] as f32 / 255.0) * a + dst[i] * inv;
    }
    dst[3] = a + dst[3] * inv;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pill's silhouette must be fully transparent at the corners (so the
    /// rounded shape does not eat clicks meant for Explorer) and opaque enough
    /// in the middle to read as a surface.
    #[test]
    fn composed_pill_is_rounded_and_translucent() {
        let st = Style {
            dark: true,
            high_contrast: false,
            dpi: 96,
            scale_milli: 1000,
        };
        let (w, h) = (60, 24);
        let mut px = vec![0u32; (w * h) as usize];
        let mask = vec![0u8; (w * h) as usize];
        compose(&mut px, &mask, w, h, 4, 9, st);

        let alpha = |x: i32, y: i32| (px[(y * w + x) as usize] >> 24) & 0xFF;
        assert_eq!(alpha(0, 0), 0, "corner must be cut away");
        assert_eq!(alpha(w - 1, h - 1), 0, "corner must be cut away");
        let mid = alpha(w / 2, h / 2);
        assert!(mid > 128 && mid < 255, "translucent glass, got {mid}");
    }

    /// High contrast drops the translucency entirely: users on that scheme get
    /// their own opaque system colors, not our glass.
    #[test]
    fn high_contrast_is_opaque_system_colors() {
        let st = Style {
            dark: false,
            high_contrast: true,
            dpi: 96,
            scale_milli: 1000,
        };
        let (fill, _, text, _) = st.colors();
        assert_eq!(fill[3], 0xFF);
        assert_eq!(text[3], 0xFF);
    }

    /// Every accessibility knob has to reach the geometry, or the pill would
    /// stay 24 px tall at font-size XL.
    #[test]
    fn scale_drives_pixel_sizes() {
        let small = Style {
            dark: true,
            high_contrast: false,
            dpi: 96,
            scale_milli: 900,
        };
        let large = Style {
            dark: true,
            high_contrast: false,
            dpi: 96,
            scale_milli: 2025, // xl (1.35) x panel scale 1.5
        };
        assert!(large.px(24.0) > small.px(24.0));
        assert!(small.px(1.0) >= 1, "hairlines never round to nothing");
    }
}
