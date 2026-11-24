//! Explorer's live notification registrations. Receive its initial snapshot on a COM worker;
//! old customization records have no live owner and are excluded. Never change tray preferences.
use std::sync::{Arc, Mutex};
use windows::core::{implement, interface, Interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_LOCAL_SERVER};
use windows::Win32::UI::{Accessibility::*, WindowsAndMessaging::*};

#[repr(C)]
struct Item {
    exe: PCWSTR, tip: PCWSTR, icon: HICON, owner: HWND, preference: u32, id: u32, guid: GUID,
}
#[interface("d782ccba-afb0-43f1-94db-fda3779eaccb")]
unsafe trait ITrayCallback: IUnknown {
    fn notify(&self, event: u32, item: *const Item) -> HRESULT;
}
#[interface("d133ce13-3537-48ba-93a7-afcd5d2053b4")]
unsafe trait ITrayNotify: IUnknown {
    fn register(&self, callback: *mut core::ffi::c_void, cookie: *mut u64) -> HRESULT;
    fn unregister(&self, cookie: u64) -> HRESULT;
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry {
    pub owner: isize, pub pid: u32, pub tip: String, id: u32, guid: GUID,
}
#[implement(ITrayCallback)]
struct Callback(Arc<Mutex<Vec<Entry>>>);
impl ITrayCallback_Impl for Callback_Impl {
    unsafe fn notify(&self, event: u32, item: *const Item) -> HRESULT {
        let Some(item) = item.as_ref() else { return HRESULT(0) };
        let mut pid = 0;
        if item.owner.is_invalid() || GetWindowThreadProcessId(item.owner, Some(&mut pid)) == 0 || pid == std::process::id() { return HRESULT(0); }
        let tip = if item.tip.is_null() { String::new() } else { item.tip.to_string().unwrap_or_default() };
        let entry = Entry { owner: item.owner.0 as isize, pid, tip, id: item.id, guid: item.guid };
        let mut list = self.0.lock().unwrap();
        list.retain(|old| !(old.owner == entry.owner && old.id == entry.id && old.guid == entry.guid));
        if event != 2 { list.push(entry); }
        HRESULT(0)
    }
}
pub(super) fn snapshot() -> Vec<Entry> {
    const TRAY: GUID = GUID::from_u128(0x25dead04_1eac_4911_9e3a_ad0a4ab560fd);
    let list = Arc::new(Mutex::new(Vec::new()));
    unsafe {
        let Ok(shell) = CoCreateInstance::<_, ITrayNotify>(&TRAY, None, CLSCTX_LOCAL_SERVER) else { return Vec::new() };
        let callback: ITrayCallback = Callback(list.clone()).into();
        let mut cookie = 0;
        if shell.register(callback.as_raw(), &mut cookie).is_ok() { let _ = shell.unregister(cookie); }
    }
    let rows = list.lock().unwrap().clone();
    rows
}

unsafe fn action(button: &IUIAutomationElement) -> bool {
    if let Ok(invoke) = button.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
        if invoke.Invoke().is_ok() { return true; }
    }
    button.GetCurrentPatternAs::<IUIAutomationLegacyIAccessiblePattern>(UIA_LegacyIAccessiblePatternId)
        .is_ok_and(|legacy| legacy.DoDefaultAction().is_ok())
}

// Search only notification-area buttons. Never press a similarly named taskbar launcher.
unsafe fn find(ui: &IUIAutomation, class: PCWSTR, tip: &str, overflow: bool) -> Option<IUIAutomationElement> {
    let hwnd = FindWindowW(class, None).ok()?;
    let root = ui.ElementFromHandle(hwnd).ok()?;
    let all = root.FindAll(TreeScope_Descendants, &ui.CreateTrueCondition().ok()?).ok()?;
    let mut found = None;
    for i in 0..all.Length().ok()? {
        let Ok(el) = all.GetElement(i) else { continue };
        let class = el.CurrentClassName().unwrap_or_default().to_string();
        let id = el.CurrentAutomationId().unwrap_or_default().to_string();
        let matches = if overflow { id == "SystemTrayIcon" && class.contains("Chevron") || id == "SystemTray.OverflowButton" || class == "SystemTray.ChevronIconView" }
            else { let name = el.CurrentName().unwrap_or_default().to_string();
                (class == "SystemTray.NotifyIconView" || class == "ToolbarWindow32" || class.is_empty() && el.CurrentControlType().ok() == Some(UIA_ButtonControlTypeId)) && !tip.is_empty() && name.trim().eq_ignore_ascii_case(tip.trim()) };
        if matches && !el.CurrentIsOffscreen().is_ok_and(|flag| flag.as_bool()) {
            if found.is_some() { return None; }
            found = Some(el);
        }
    }
    found
}
// Visible tray icons can also be identified without their tooltip (which may be
// empty, localised or change with status). The overflow chevron is never an icon.
unsafe fn at_icon(ui: &IUIAutomation, entry: &Entry) -> Option<IUIAutomationElement> {
    use windows::Win32::UI::Shell::{Shell_NotifyIconGetRect, NOTIFYICONIDENTIFIER};
    let id = NOTIFYICONIDENTIFIER { cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32, hWnd: HWND(entry.owner as *mut _), uID: entry.id, guidItem: entry.guid };
    let rect = Shell_NotifyIconGetRect(&id).ok()?;
    if rect.right <= rect.left || rect.bottom <= rect.top { return None; }
    let pt = POINT { x: (rect.left + rect.right)/2, y: (rect.top + rect.bottom)/2 };
    let mut el = ui.ElementFromPoint(pt).ok()?;
    let walker = ui.ControlViewWalker().ok()?;
    for _ in 0..5 {
        if el.CurrentClassName().ok()?.to_string() == "SystemTray.NotifyIconView" {
            let name = el.CurrentName().unwrap_or_default().to_string();
            if entry.tip.is_empty() || name.trim().eq_ignore_ascii_case(entry.tip.trim()) { return Some(el); }
            return None;
        }
        el = walker.GetParentElement(&el).ok()?;
    }
    None
}
pub(super) fn activate(entry: Entry) {
    use windows::core::w;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED, CLSCTX_INPROC_SERVER};
    std::thread::spawn(move || unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {return;}
        let _ = (|| -> Option<()> {
            let mut pid = 0;
            GetWindowThreadProcessId(HWND(entry.owner as *mut _), Some(&mut pid));
            if pid != entry.pid { return None; }
            let ui: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
            let button = || at_icon(&ui, &entry).or_else(|| [w!("Shell_TrayWnd"), w!("TopLevelWindowForOverflowXamlIsland"), w!("NotifyIconOverflowWindow")].into_iter().find_map(|class| find(&ui, class, &entry.tip, false)));
            if let Some(el) = button() { action(&el); return Some(()); }
            if let Some(chevron) = find(&ui, w!("Shell_TrayWnd"), "", true) {
                if action(&chevron) {
                    for _ in 0..8 { std::thread::sleep(std::time::Duration::from_millis(75)); if let Some(el) = button() { action(&el); break; } }
                }
            }
            Some(())
        })();
        CoUninitialize();
    });
}
#[cfg(test)]
impl Entry {
    pub(super) fn sample(pid: u32) -> Self { Self { owner: 42, pid, tip: "Chat".into(), id: 1, guid: GUID::zeroed() } }
}
