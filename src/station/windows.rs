//! The visible top-level windows that can be brought back to the foreground from the workbench rail.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM};

use crate::ui::canvas::Picture;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    ShowWindowAsync, GWL_EXSTYLE, SW_RESTORE, WS_EX_TOOLWINDOW,
};

mod notification;
use std::sync::Mutex;
use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindow, GetShellWindow, GW_OWNER, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, IsIconic};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    // Stable across window -> tray -> window transitions.
    pub handle: isize,
    pub title: String,
    process: u32,
    executable: Option<PathBuf>,
    hwnd: Option<isize>,
    tray: Option<notification::Entry>,
}

static LIST: Mutex<Vec<Window>> = Mutex::new(Vec::new());
static REFRESH: Mutex<Option<Instant>> = Mutex::new(None);
static BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shown() -> Vec<Window> {
    use std::sync::atomic::Ordering;
    let mut last = REFRESH.lock().unwrap();
    if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) && !BUSY.swap(true, Ordering::AcqRel) {
        *last = Some(Instant::now());
        std::thread::spawn(|| {
            use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let next = collect(if com { notification::snapshot() } else { Vec::new() });
            let changed = { let mut list = LIST.lock().unwrap(); let changed = *list != next; *list = next; changed };
            if com { unsafe { CoUninitialize() }; }
            BUSY.store(false, Ordering::Release);
            if changed { if let Some(app) = crate::APP.get() { app.controller.wake(); } }
        });
    }
    LIST.lock().unwrap().clone()
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}
fn switchable(hwnd: HWND) -> bool {
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if !IsWindowVisible(hwnd).as_bool() || hwnd == GetShellWindow() || style & (WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0) != 0 || (style & WS_EX_APPWINDOW.0 == 0 && GetWindow(hwnd, GW_OWNER).is_ok()) { return false; }
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4);
        cloaked == 0 && !matches!(class_name(hwnd).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "TopLevelWindowForOverflowXamlIsland" | "NotifyIconOverflowWindow" | "TaskSwitcherWnd")
    }
}
// UWP windows can share one ApplicationFrameHost process. Use the app owning the
// CoreWindow child as the identity, while switching to the original outer HWND.
fn app_image(hwnd: HWND, pid: u32) -> Option<PathBuf> {
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
    if class_name(hwnd) == "ApplicationFrameWindow" {
        if let Ok(child) = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) } {
            let mut actual = 0;
            unsafe { GetWindowThreadProcessId(child, Some(&mut actual)) };
            if actual != pid { return process_image(actual); }
        }
        return None;
    }
    process_image(pid)
}
fn append(list: &mut Vec<Window>, mut row: Window) {
    let mut hash = DefaultHasher::new();
    if let Some(path) = &row.executable { path.to_string_lossy().to_lowercase().hash(&mut hash); } else if row.hwnd.is_some_and(|h| class_name(HWND(h as *mut _)) == "ApplicationFrameWindow") { row.hwnd.hash(&mut hash); } else { row.process.hash(&mut hash); }
    row.handle = hash.finish() as isize;
    if let Some(prior) = list.iter_mut().find(|item| item.handle == row.handle) {
        if prior.hwnd.is_none() && row.hwnd.is_some() { row.tray = prior.tray.take(); *prior = row; }
        else if prior.tray.is_none() { prior.tray = row.tray; }
    } else { list.push(row); }
}
fn collect(tray: Vec<notification::Entry>) -> Vec<Window> {
    unsafe extern "system" fn gather(hwnd: HWND, data: LPARAM) -> BOOL {
        let found = &mut *(data.0 as *mut Vec<Window>);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != std::process::id() && switchable(hwnd) {
            let mut title = [0u16; 1024];
            let len = GetWindowTextW(hwnd, &mut title);
            if len > 0 {
                append(found, Window { handle: 0, title: String::from_utf16_lossy(&title[..len as usize]), process: pid, executable: app_image(hwnd, pid), hwnd: Some(hwnd.0 as isize), tray: None });
            }
        }
        BOOL(1)
    }
    let mut list = Vec::new();
    unsafe { let _ = EnumWindows(Some(gather), LPARAM(&mut list as *mut Vec<Window> as isize)); }
    for entry in tray {
        let executable = process_image(entry.pid);
        let title = if entry.tip.trim().is_empty() { executable.as_deref().and_then(|p| p.file_stem()).unwrap_or_default().to_string_lossy().into_owned() } else { entry.tip.clone() };
        append(&mut list, Window { handle: 0, title, process: entry.pid, executable, hwnd: None, tray: Some(entry) });
    }
    list
}

struct WindowIcon {
    hwnd: Option<isize>,
    side: u32,
    process: u32,
    queried: Instant,
    picture: Option<Picture>,
    executable: Option<PathBuf>,
}

thread_local! {
    static ICONS: RefCell<HashMap<isize, WindowIcon>> = RefCell::new(HashMap::new());
}

pub fn keep_icons(windows: &[Window]) {
    ICONS.with(|icons| icons.borrow_mut().retain(|handle, _| windows.iter().any(|window| window.handle == *handle)));
}

pub fn icon(window: &Window, side: u32) -> Option<Picture> {
    let cached = ICONS.with(|icons| {
        let icons = icons.borrow();
        let cached = icons.get(&window.handle)?;
        (cached.process == window.process && cached.hwnd == window.hwnd && cached.side == side && cached.queried.elapsed() < Duration::from_secs(30))
            .then(|| (cached.picture.clone(), cached.executable.clone()))
    });
    let (picture, executable) = cached.unwrap_or_else(|| {
        let picture = window.hwnd.and_then(|hwnd| window_picture(HWND(hwnd as *mut _), side));
        let executable = window.executable.clone();
        ICONS.with(|icons| icons.borrow_mut().insert(window.handle, WindowIcon {
            hwnd: window.hwnd, side, process: window.process, queried: Instant::now(), picture: picture.clone(), executable: executable.clone(),
        }));
        (picture, executable)
    });
    if picture.as_ref().is_some_and(|p| p.size.0.min(p.size.1) >= side) { return picture; }
    executable.as_deref().and_then(|path| super::thumbs::icon(path, side)).or(picture)
}

fn window_picture(window: HWND, side: u32) -> Option<Picture> {
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapPaletteTypeCustom};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, SendMessageTimeoutW, GCLP_HICON, GCLP_HICONSM, HICON, ICON_BIG, ICON_SMALL2, SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_GETICON};

    unsafe {
        let mut icon = 0;
        let sent = SendMessageTimeoutW(window, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM((side * 3) as isize), SMTO_ABORTIFHUNG | SMTO_BLOCK, 15, Some(&mut icon));
        if sent.0 == 0 || icon == 0 {
            icon = GetClassLongPtrW(window, GCLP_HICON);
            if icon == 0 { let _ = SendMessageTimeoutW(window, WM_GETICON, WPARAM(ICON_SMALL2 as usize), LPARAM(0), SMTO_ABORTIFHUNG | SMTO_BLOCK, 15, Some(&mut icon)); }
            if icon == 0 { icon = GetClassLongPtrW(window, GCLP_HICONSM); }
        }
        if icon == 0 { return None; }
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let bitmap = factory.CreateBitmapFromHICON(HICON(icon as *mut _)).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter.Initialize(&bitmap, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let (mut width, mut height) = (0, 0);
        converter.GetSize(&mut width, &mut height).ok()?;
        if width == 0 || height == 0 || width > 512 || height > 512 { return None; }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        converter.CopyPixels(std::ptr::null(), width * 4, &mut pixels).ok()?;
        if !pixels.chunks_exact(4).any(|pixel| pixel[3] > 0) { return None; }
        let mut key = DefaultHasher::new();
        ("workbench-window-icon", width, height, &pixels).hash(&mut key);
        Some(Picture { key: key.finish(), size: (width, height), pixels: Arc::new(pixels) })
    }
}

fn process_image(process: u32) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process).ok()?;
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut length);
        let _ = CloseHandle(process);
        result.ok()?;
        Some(std::ffi::OsString::from_wide(&path[..length as usize]).into())
    }
}

pub fn title(key: isize) -> Option<String> {
    LIST.lock().unwrap().iter().find(|w| w.handle == key).map(|w| w.title.clone())
}

/// AutoCAD owns separate palette and WPF windows alongside its drawing frame.
/// Resolve the frame for this instance when clicked, since loading a drawing can
/// replace the window stored in the rail snapshot.
fn acad_frame(pid: u32) -> Option<HWND> {
    struct Search { pid: u32, frame: Option<HWND> }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = &mut *(data.0 as *mut Search);
        let mut owner_pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner_pid));
        if owner_pid == search.pid && class_name(hwnd).starts_with("AfxMDIFrame") && switchable(hwnd) {
            search.frame = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut search = Search { pid, frame: None };
    unsafe { let _ = EnumWindows(Some(visit), LPARAM((&mut search as *mut Search) as isize)); }
    search.frame
}

fn focus_acad(frame: HWND) {
    use windows::Win32::Foundation::WPARAM;
    use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
    use windows::Win32::UI::WindowsAndMessaging::{GetLastActivePopup, GetForegroundWindow, SendMessageTimeoutW, SwitchToThisWindow, GW_ENABLEDPOPUP, SMTO_ABORTIFHUNG};
    unsafe {
        if IsIconic(frame).as_bool() { let _ = ShowWindowAsync(frame, SW_RESTORE); }
        let target = if IsWindowEnabled(frame).as_bool() { frame } else {
            [GetLastActivePopup(frame), GetWindow(frame, GW_ENABLEDPOPUP).unwrap_or_default()]
                .into_iter().find(|&h| h != frame && IsWindowVisible(h).as_bool() && IsWindowEnabled(h).as_bool()).unwrap_or(frame)
        };
        #[cfg(test)] eprintln!("activating frame={frame:?}, target={target:?}, before={:?}", GetForegroundWindow());
        let _ = SetForegroundWindow(target);
        // Foreground activation across input queues is asynchronous. Synchronize
        // with the drawing thread before checking whether the handoff took effect.
        use windows::Win32::UI::WindowsAndMessaging::{IsHungAppWindow, WM_NULL};
        let _ = SendMessageTimeoutW(target, WM_NULL, WPARAM(0), LPARAM(0), SMTO_ABORTIFHUNG, 120, None);
        let front = GetForegroundWindow();
        if front == target { return; }
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        let caller = GetCurrentThreadId();
        let foreground_thread = GetWindowThreadProcessId(front, None);
        if foreground_thread != 0 && caller != foreground_thread && !IsHungAppWindow(front).as_bool() && !IsHungAppWindow(target).as_bool()
            && AttachThreadInput(caller, foreground_thread, true).as_bool() {
            let _ = SetForegroundWindow(target);
            let _ = AttachThreadInput(caller, foreground_thread, false);
        }
        if GetForegroundWindow() != target { SwitchToThisWindow(target, true); }
    }
}

pub fn activate(key: isize) {
    let row = LIST.lock().unwrap().iter().find(|w| w.handle == key).cloned();
    let Some(row) = row else { return };
    let autocad = row.executable.as_deref().and_then(|path| path.file_name()).is_some_and(|name| name.eq_ignore_ascii_case("acad.exe"));
    if autocad || (row.executable.is_none() && row.hwnd.is_some_and(|h| class_name(HWND(h as *mut _)).starts_with("AfxMDIFrame"))) {
        if let Some(frame) = acad_frame(row.process) {
            focus_acad(frame);
            return;
        }
    }
    if let Some(handle) = row.hwnd {
        let hwnd = HWND(handle as *mut _);
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == row.process && switchable(hwnd) {
                if IsIconic(hwnd).as_bool() { let _ = ShowWindowAsync(hwnd, SW_RESTORE); }
                let _ = SetForegroundWindow(hwnd);
                return;
            }
        }
    }
    if let Some(entry) = row.tray { notification::activate(entry); }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn app(path: &str, hwnd: Option<isize>, pid: u32) -> Window {
        Window { handle: 0, title: path.into(), process: pid, executable: Some(path.into()), hwnd, tray: None }
    }
    // Give the process a real input message under the rail's WS_EX_NOACTIVATE policy.
    unsafe fn rail_click(key: isize) -> bool {
        use windows::core::w;
        use windows::Win32::Foundation::POINT;
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::UI::Input::KeyboardAndMouse::{mouse_event, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP};
        let mut point = POINT::default();
        GetCursorPos(&mut point).unwrap();
        let rail = CreateWindowExW(WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, w!("STATIC"), w!("Glance CAD test"), WS_POPUP | WS_VISIBLE, point.x-40, point.y-20, 80, 40, None, None, None, None).unwrap();
        let mut current = POINT::default();
        GetCursorPos(&mut current).unwrap();
        let mut handled = false;
        if WindowFromPoint(current) == rail {
            mouse_event(MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
            let start = Instant::now();
            let mut msg = MSG::default();
            while !handled && start.elapsed() < Duration::from_secs(2) {
                if PeekMessageW(&mut msg, Some(rail), 0, 0, PM_REMOVE).as_bool() {
                    if msg.message == WM_LBUTTONUP { handled = crate::station::pressed(crate::ui::view::Hit::RunningWindow(key)); }
                    let _ = DispatchMessageW(&msg);
                } else { std::thread::sleep(Duration::from_millis(5)); }
            }
        }
        let _ = DestroyWindow(rail);
        handled
    }
    /// Opt-in desktop regression: exercise the actual workbench click handler on a running AutoCAD.
    #[test]
    #[ignore = "temporarily minimizes and switches to AutoCAD on the interactive desktop"]
    fn wake_live_autocad_from_minimized() {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowPlacement, SetWindowPlacement, WINDOWPLACEMENT, SW_MINIMIZE};
        let rows = collect(Vec::new());
        let cad = rows.iter().find(|w| w.executable.as_deref().and_then(|p| p.file_name()).is_some_and(|p| p.eq_ignore_ascii_case("acad.exe"))).expect("running AutoCAD");
        let key = cad.handle;
        let frame = HWND(cad.hwnd.expect("CAD window") as *mut _);
        assert!(class_name(frame).starts_with("AfxMDIFrame"), "expected the CAD drawing frame");
        let wait = |ready: &dyn Fn() -> bool| {
            let start = Instant::now();
            while !ready() && start.elapsed() < Duration::from_secs(3) { std::thread::sleep(Duration::from_millis(20)); }
            ready()
        };
        unsafe {
            use windows::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
            if !IsWindowEnabled(frame).as_bool() {
                use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOTOWNER};
                let previous = GetForegroundWindow();
                *LIST.lock().unwrap() = rows;
                rail_click(key);
                let ok = wait(&|| {
                    let front = GetForegroundWindow();
                    front != frame && GetAncestor(front, GA_ROOTOWNER) == frame && IsWindowEnabled(front).as_bool()
                });
                eprintln!("modal CAD frame={frame:?} foreground={:?} activated={ok}", GetForegroundWindow());
                if !previous.is_invalid() { let _ = SetForegroundWindow(previous); }
                assert!(ok, "CAD modal dialog receives focus");
                return;
            }
            let previous = GetForegroundWindow();
            let mut placement = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
            GetWindowPlacement(frame, &mut placement).unwrap();
            let mut rows = rows;
            // A previously enumerated UI helper or document window has just gone away.
            rows.iter_mut().find(|r| r.handle == key).unwrap().hwnd = Some(0);
            *LIST.lock().unwrap() = rows;
            let _ = ShowWindowAsync(frame, SW_MINIMIZE);
            let minimized = wait(&|| IsIconic(frame).as_bool() && GetForegroundWindow() != frame && !GetForegroundWindow().is_invalid());
            let handled = rail_click(key);
            let woke = wait(&|| !IsIconic(frame).as_bool() && GetForegroundWindow() == frame);
            eprintln!("frame={:?} minimized={minimized} handled={handled} restored={} foreground={:?}", frame, !IsIconic(frame).as_bool(), GetForegroundWindow());
            let _ = SetWindowPlacement(frame, &placement);
            if !previous.is_invalid() { let _ = SetForegroundWindow(previous); }
            assert!(minimized && handled && woke);
        }
    }
    #[test]
    fn running_apps_keep_one_identity_across_windows_and_tray() {
        let mut list = Vec::new();
        append(&mut list, app(r"C:\Apps\Chat.exe", Some(1), 7));
        let key = list[0].handle;
        append(&mut list, app(r"c:\apps\chat.exe", Some(2), 8));
        let entry = notification::Entry::sample(8);
        let tray = Window { tray: Some(entry.clone()), ..app(r"C:\Apps\Chat.exe", None, 8) };
        append(&mut list, tray.clone());
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].hwnd, &list[0].tray), (Some(1), &Some(entry)));
        list.clear();
        append(&mut list, tray);
        assert_eq!((list[0].handle, list[0].hwnd), (key, None));
        append(&mut list, app(r"C:\Apps\Chat.exe", Some(3), 9));
        assert_eq!((list.len(), list[0].hwnd), (1, Some(3)));
        append(&mut list, app(r"D:\Other\Chat.exe", Some(4), 10));
        assert_eq!(list.len(), 2);
    }
}
