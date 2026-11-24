//! SIMO's compact in-panel chat surface.

use super::canvas::{Align, Canvas, Family, Fill, Font, Point};
use super::gfx::{Frame, Gfx};
use super::theme::Theme;
use crate::simo::{Chat, Role, State};
use std::sync::OnceLock;

pub const WIDTH: f32 = 420.0;
pub const HEIGHT: f32 = 520.0;
pub const SIDEBAR_WIDTH: f32 = 224.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Expand,
    Settings,
    Pin,
    Close,
}

pub fn action_id(action: Action) -> u8 {
    match action { Action::Expand => 0, Action::Settings => 1, Action::Pin => 2, Action::Close => 3 }
}

pub fn action_label(action: Action, expanded: bool, pinned: bool, lang: super::text::Lang) -> &'static str {
    match action {
        Action::Expand if expanded => lang.pick("收起对话", "Collapse conversation"),
        Action::Expand => lang.pick("展开对话", "Expand conversation"),
        Action::Settings => lang.pick("SIMO 设置", "SIMO settings"),
        Action::Pin if pinned => lang.pick("取消固定", "Unpin SIMO"),
        Action::Pin => lang.pick("固定位置", "Pin SIMO"),
        Action::Close => lang.pick("收起 SIMO", "Put SIMO away"),
    }
}

pub fn header_action(size: (f32, f32), x: f32, y: f32) -> Option<Action> {
    if y < 0.0 || y > 48.0 {
        return None;
    }
    let pad = 16.0;
    let step = super::buttons::SIZE + super::buttons::GAP;
    let right = size.0 - pad - super::buttons::SIZE / 2.0;
    [Action::Close, Action::Pin, Action::Settings, Action::Expand]
        .into_iter()
        .enumerate()
        .find(|(index, _)| (x - (right - *index as f32 * step)).abs() <= super::buttons::SIZE / 2.0 + 3.0)
        .map(|(_, action)| action)
}

pub fn header_drag_handle(size: (f32, f32), x: f32, y: f32) -> bool {
    let step = super::buttons::SIZE + super::buttons::GAP;
    (16.0..size.0 - 16.0 - 4.0 * step).contains(&x) && (0.0..56.0).contains(&y)
}

pub fn widget_header_action(size: (f32, f32), x: f32, y: f32) -> Option<Action> {
    if y < 0.0 || y > 48.0 {
        return None;
    }
    let right = size.0 - 16.0 - super::buttons::SIZE / 2.0;
    let expand = right;
    ((x - expand).abs() <= super::buttons::SIZE / 2.0 + 3.0).then_some(Action::Expand)
}

pub fn widget_drag_handle(size: (f32, f32), input: (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    let (ix, iy, iw, ih) = input;
    (0.0..size.0).contains(&x)
        && (0.0..size.1).contains(&y)
        && widget_header_action(size, x, y).is_none()
        && (!(ix..ix + iw).contains(&x) || !(iy..iy + ih).contains(&y))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputAction {
    Text,
    Attach,
    Send,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    New,
    Select(String),
    Previous,
    Next,
}

fn sidebar_width(size: (f32, f32), expanded: bool) -> f32 {
    if expanded { SIDEBAR_WIDTH.min(size.0 * 0.33) } else { 0.0 }
}

const NEW_CHAT_TOP: f32 = 72.0;
const NEW_CHAT_HEIGHT: f32 = 32.0;
const NEW_CHAT_CENTER: f32 = NEW_CHAT_TOP + NEW_CHAT_HEIGHT / 2.0;
const HISTORY_START: f32 = NEW_CHAT_TOP + NEW_CHAT_HEIGHT + 9.0;
const HISTORY_STEP: f32 = 34.0;

fn history_rows(height: f32) -> usize {
    (((height - 52.0 - HISTORY_START) / HISTORY_STEP).floor() as usize).max(1)
}

pub fn history_action(chat: &Chat, size: (f32, f32), x: f32, y: f32) -> Option<HistoryAction> {
    let width = sidebar_width(size, chat.expanded);
    if !(12.0..width - 8.0).contains(&x) { return None; }
    if (NEW_CHAT_TOP..NEW_CHAT_TOP + NEW_CHAT_HEIGHT).contains(&y) { return Some(HistoryAction::New); }
    let conversations = chat.conversations();
    let rows = history_rows(size.1);
    let pages = conversations.len().max(1).div_ceil(rows);
    let page = chat.history_page.min(pages - 1);
    if (size.1 - 48.0..size.1 - 16.0).contains(&y) {
        if x < 42.0 && page > 0 { return Some(HistoryAction::Previous); }
        if x > width - 38.0 && page + 1 < pages { return Some(HistoryAction::Next); }
    }
    if !(HISTORY_START..HISTORY_START + rows as f32 * HISTORY_STEP).contains(&y) { return None; }
    let offset = y - HISTORY_START;
    if offset % HISTORY_STEP >= HISTORY_STEP - 4.0 { return None; }
    let row = (offset / HISTORY_STEP) as usize;
    conversations.get(page * rows + row).map(|conversation| HistoryAction::Select(conversation.id.clone()))
}

pub fn attachment_rect(size: (f32, f32), draft: &str, gfx: &Gfx, expanded: bool, index: usize) -> Option<(f32, f32, f32, f32)> {
    let input = input_rect_with_attachments(size, draft, gfx, expanded, index + 1);
    let x = input.0 + 8.0 + index as f32 * 124.0;
    (x + 116.0 <= input.0 + input.2 - 8.0).then_some((x, input.1 + 6.0, 116.0, 82.0))
}

pub fn attachment_hover(size: (f32, f32), draft: &str, gfx: &Gfx, expanded: bool, count: usize, x: f32, y: f32) -> Option<usize> {
    (0..count).find(|index| attachment_rect(size, draft, gfx, expanded, *index).is_some_and(|(left, top, width, height)| x >= left && x < left + width && y >= top && y < top + height))
}

pub fn attachment_remove_at(size: (f32, f32), draft: &str, gfx: &Gfx, expanded: bool, count: usize, x: f32, y: f32) -> Option<usize> {
    (0..count).find(|index| attachment_rect(size, draft, gfx, expanded, *index).is_some_and(|(left, top, width, _)| {
        let cx = left + width - 12.0;
        let cy = top + 12.0;
        (x - cx).hypot(y - cy) <= 10.0
    }))
}

// DirectWrite takes a line's top. Center its glyphs, including CJK fallback faces.
fn history_label_top(gfx: &Gfx, text: &str, font: Font, center: f32) -> f32 {
    let (top, bottom) = gfx.ink(text, font);
    center - (top + bottom) / 2.0
}

pub fn history_menu(lang: super::text::Lang, look: super::menu::Look, with_panel: bool) -> Option<super::menu::Menu> {
    use super::{icons::Icon, menu::{Item, Mark, Menu}};
    let item = Item::new(lang.pick("删除对话", "Delete conversation"), Some(Mark::Icon(Icon::Trash)), false, false);
    Menu::open(vec![item], look, with_panel)
}

pub fn delete_history(chat: &mut Chat, id: &str, owner: windows::Win32::Foundation::HWND, lang: super::text::Lang) {
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    if let Err(error) = chat.delete_conversation(id) {
        let title = windows::core::HSTRING::from(lang.pick("无法删除对话", "Could not delete conversation"));
        let detail = windows::core::HSTRING::from(error.to_string());
        unsafe { MessageBoxW(Some(owner), &detail, &title, MB_OK | MB_ICONERROR) };
    }
}

fn paint_history_sidebar(frame: &Frame, theme: &Theme, chat: &Chat, size: (f32, f32)) {
    let width = sidebar_width(size, true);
    let lang = super::text::Lang::resolve(super::prefs::Prefs::resolve(&crate::app().settings.lock().unwrap().view, &[]).language);
    frame.fill(theme.rule, width, 70.0, 1.0, (size.1 - 86.0).max(1.0));
    let hovered = chat.pointer.and_then(|(x, y)| history_action(chat, size, x, y));
    frame.fill_rounded(theme.rule.alpha(if hovered == Some(HistoryAction::New) { 0.65 } else { 0.28 }), 12.0, NEW_CHAT_TOP, width - 20.0, NEW_CHAT_HEIGHT, 6.0);
    frame.icon(super::icons::Icon::Add, Point { x: 25.0, y: NEW_CHAT_CENTER }, 14.0, theme.text2);
    let new_chat = lang.pick("新对话", "New chat");
    let new_font = Font::new(Family::Segoe, 12.0, 500.0);
    frame.text(new_chat, new_font, theme.text, 38.0, history_label_top(frame.gfx, new_chat, new_font, NEW_CHAT_CENTER), width - 54.0, Align::Start);
    let conversations = chat.conversations();
    let rows = history_rows(size.1);
    let pages = conversations.len().max(1).div_ceil(rows);
    let page = chat.history_page.min(pages - 1);
    for (row, conversation) in conversations.iter().skip(page * rows).take(rows).enumerate() {
        let top = HISTORY_START + row as f32 * HISTORY_STEP;
        let active = conversation.id == chat.conversation_id;
        let over = matches!(&hovered, Some(HistoryAction::Select(id)) if id == &conversation.id);
        if active || over {
            frame.fill_rounded(theme.rule.alpha(if active { 0.7 } else { 0.4 }), 12.0, top, width - 20.0, HISTORY_STEP - 4.0, 6.0);
        }
        let title = &conversation.title;
        let font = Font::new(Family::Segoe, 12.0, if active { 600.0 } else { 400.0 });
        let height = HISTORY_STEP - 4.0;
        frame.clip(12.0, top, width - 20.0, height);
        frame.text(title, font, if active { theme.text } else { theme.text2 }, 20.0, history_label_top(frame.gfx, title, font, top + height / 2.0), width - 36.0, Align::Start);
        frame.unclip();
    }
    if pages > 1 {
        let y = size.1 - 32.0;
        frame.icon(super::icons::Icon::ChevronLeft, Point { x: 25.0, y }, 15.0, theme.text2.alpha(if page > 0 { 1.0 } else { 0.35 }));
        let page_text = format!("{} / {pages}", page + 1);
        let font = Font::new(Family::Segoe, 11.0, 400.0);
        let text_width = frame.gfx.measure(&page_text, font);
        frame.text(&page_text, font, theme.text2, (width - text_width) / 2.0, history_label_top(frame.gfx, &page_text, font, y), text_width + 2.0, Align::Start);
        frame.icon(super::icons::Icon::ChevronRight, Point { x: width - 24.0, y }, 15.0, theme.text2.alpha(if page + 1 < pages { 1.0 } else { 0.35 }));
    }
}

fn paint_attachments(frame: &Frame, theme: &Theme, chat: &Chat, size: (f32, f32), draft: &str) {
    let quiet = theme.text2;
    for (index, attachment) in chat.attachments.iter().enumerate() {
        let Some((x, y, width, height)) = attachment_rect(size, draft, frame.gfx, chat.expanded, index) else { break };
        frame.fill_rounded(theme.paper.alpha(0.72), x, y, width, height, 7.0);
        if let Some((pixels, image_size)) = &attachment.preview {
            let key = attachment.bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ *byte as u64).wrapping_mul(0x100000001b3));
            let picture = super::canvas::Picture { key, size: *image_size, pixels: pixels.clone() };
            frame.picture(&picture, x + 2.0, y + 2.0, width - 4.0, height - 4.0, 6.0, 1.0);
        } else {
            frame.icon(super::icons::Icon::File, Point { x: x + width / 2.0, y: y + 29.0 }, 24.0, quiet);
            let name = attachment.name.chars().take(18).collect::<String>();
            frame.text(&name, Font::new(Family::Segoe, 10.0, 400.0), theme.text, x + 7.0, y + height - 14.0, width - 14.0, Align::Start);
        }
        if chat.hovered_attachment == Some(index) {
            frame.fill_circle(theme.paper.alpha(0.94), Point { x: x + width - 12.0, y: y + 12.0 }, 10.0);
            frame.icon(super::icons::Icon::Close, Point { x: x + width - 12.0, y: y + 12.0 }, 12.0, theme.text);
        }
    }
}

fn paint_message_attachments(frame: &Frame, theme: &Theme, attachments: &[crate::simo::Attachment], x: f32, y: f32, room: f32) {
    for (index, attachment) in attachments.iter().take(4).enumerate() {
        let left = x + index as f32 * 68.0;
        if left + 62.0 > x + room { break; }
        frame.fill_rounded(theme.paper.alpha(0.62), left, y, 62.0, 48.0, 5.0);
        if let Some((pixels, size)) = &attachment.preview {
            let key = attachment.bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ *byte as u64).wrapping_mul(0x100000001b3));
            let picture = super::canvas::Picture { key, size: *size, pixels: pixels.clone() };
            frame.picture(&picture, left + 1.0, y + 1.0, 60.0, 46.0, 4.0, 1.0);
        } else {
            frame.icon(super::icons::Icon::File, Point { x: left + 31.0, y: y + 18.0 }, 18.0, theme.text2);
            let name = attachment.name.chars().take(9).collect::<String>();
            frame.text(&name, Font::new(Family::Segoe, 8.0, 400.0), theme.text, left + 3.0, y + 42.0, 56.0, Align::Start);
        }
    }
}

type Bounds = (f32, f32, f32, f32);

fn contains((left, top, width, height): Bounds, x: f32, y: f32) -> bool {
    (left..left + width).contains(&x) && (top..top + height).contains(&y)
}

fn input_controls(size: (f32, f32)) -> [(InputAction, Point); 2] {
    let y = size.1 - 36.0;
    [(InputAction::Attach, Point { x: size.0 - 70.0, y }), (InputAction::Send, Point { x: size.0 - 40.0, y })]
}

pub fn input_action(size: (f32, f32), draft: &str, gfx: &Gfx, expanded: bool, attachments: usize, x: f32, y: f32) -> Option<InputAction> {
    let (left, top, width, height) = input_rect_with_attachments(size, draft, gfx, expanded, attachments);
    let strip = attachment_band(attachments);
    if !contains((left, top + strip, width, height - strip), x, y) { return None; }
    Some(input_action_at(size, x, y))
}

fn input_action_at(size: (f32, f32), x: f32, y: f32) -> InputAction {
    input_controls(size).into_iter()
        .find(|(_, point)| (x - point.x).abs() < 14.0 && (y - point.y).abs() < 14.0)
        .map_or(InputAction::Text, |(action, _)| action)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyAction { Copy, Play, Edit }

pub struct ReplyHit {
    index: usize,
    left: f32,
    footer_y: f32,
    user: bool,
}

impl ReplyHit {
    fn action(&self, x: f32, y: f32) -> Option<ReplyAction> {
        if !(self.footer_y..self.footer_y + 26.0).contains(&y) { return None; }
        if (self.left..self.left + 26.0).contains(&x) { Some(ReplyAction::Copy) }
        else if (self.left + 28.0..self.left + 54.0).contains(&x) { Some(if self.user { ReplyAction::Edit } else { ReplyAction::Play }) }
        else { None }
    }
}

pub fn reply_action(chat: &Chat, x: f32, y: f32) -> Option<(usize, ReplyAction)> {
    if !contains(chat.history_rect, x, y) { return None; }
    chat.reply_hits.iter().find_map(|hit| Some((hit.index, hit.action(x, y)?)))
}

pub fn pointer_shape(chat: &Chat, size: (f32, f32), gfx: &Gfx, draft: &str, on_control: bool, x: f32, y: f32) -> windows::core::PCWSTR {
    use windows::Win32::UI::WindowsAndMessaging::{IDC_ARROW, IDC_HAND, IDC_IBEAM};
    if let Some((_, action)) = editor_action(chat, x, y) {
        return if action == EditAction::Text { IDC_IBEAM } else { IDC_HAND };
    }
    if on_control || reply_action(chat, x, y).is_some() || history_action(chat, size, x, y).is_some()
        || attachment_remove_at(size, draft, gfx, chat.expanded, chat.attachments.len(), x, y).is_some()
    {
        return IDC_HAND;
    }
    match input_action(size, draft, gfx, chat.expanded, chat.attachments.len(), x, y) {
        Some(InputAction::Text) => IDC_IBEAM,
        Some(_) => IDC_HAND,
        None => IDC_ARROW,
    }
}

pub fn track_leave(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
    let _ = unsafe { TrackMouseEvent(&mut track) };
}

pub fn press_reply(chat: &mut Chat, hwnd: windows::Win32::Foundation::HWND, x: f32, y: f32) -> bool {
    let Some((index, action)) = reply_action(chat, x, y) else { return false };
    if action == ReplyAction::Edit { return chat.begin_edit(index); }
    let text = chat.messages.lock().unwrap().get(index).map(|message| message.text.clone());
    let Some(text) = text else { return false };
    match action {
        ReplyAction::Copy => {
            let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n");
            let copied = crate::clipboard::put(hwnd, &[(crate::clipboard::TEXT, crate::clipboard::utf16_z(&text))]);
            chat.copy_feedback = Some((index, copied, std::time::Instant::now()));
        }
        ReplyAction::Play => chat.speak(index, text, crate::app().settings.lock().unwrap().simo.clone()),
        ReplyAction::Edit => unreachable!(),
    }
    true
}

#[derive(Clone, Copy, PartialEq)]
enum EditAction { Text, Cancel, Send }

fn edit_buttons((x, y, w, h): Bounds) -> [(EditAction, Bounds); 2] {
    let right = x + w - 10.0;
    let top = y + h - 36.0;
    [(EditAction::Cancel, (right - 126.0, top, 64.0, 27.0)), (EditAction::Send, (right - 56.0, top, 56.0, 27.0))]
}

fn editor_action(chat: &Chat, x: f32, y: f32) -> Option<(Bounds, EditAction)> {
    let bounds = chat.edit_rect?;
    if !contains(bounds, x, y) || !contains(chat.history_rect, x, y) { return None; }
    let action = edit_buttons(bounds).into_iter()
        .find(|(_, button)| contains(*button, x, y)).map_or(EditAction::Text, |(action, _)| action);
    Some((bounds, action))
}

pub fn press_editor(chat: &mut Chat, gfx: &Gfx, window: &super::window::Window, x: f32, y: f32) -> bool {
    let Some((bounds, action)) = editor_action(chat, x, y) else { return false; };
    match action {
        EditAction::Text => {
            chat.focus_editor(true);
            window.take_simo_keys();
            begin_selection(chat, gfx, window.hwnd, bounds, x, y);
        }
        EditAction::Cancel => chat.cancel_edit(),
        EditAction::Send => {
            let settings = crate::app().settings.lock().unwrap().simo.clone();
            chat.send_edit(settings);
        }
    }
    true
}

pub fn scroll_at(chat: &mut Chat, x: f32, y: f32, delta: i16) -> bool {
    if !contains(chat.history_rect, x, y) || chat.scroll_max <= 0.0 { return false; }
    chat.scroll_target = (chat.scroll_target + f32::from(delta) * 0.65).clamp(0.0, chat.scroll_max);
    true
}

pub fn scrolling(chat: &Chat) -> bool {
    (chat.scroll - chat.scroll_target).abs() > 0.25
}

#[derive(Default)]
pub struct MessageLayout {
    width: f32,
    rows: Vec<MessageRow>,
    height: f32,
}

#[derive(Clone, Copy)]
struct MessageRow {
    top: f32,
    width: f32,
    attachments: f32,
    bubble_height: f32,
    height: f32,
}

impl MessageLayout {
    fn measure(&mut self, messages: &[crate::simo::ChatMessage], gfx: &Gfx, font: Font, width: f32) {
        if self.width != width || messages.len() < self.rows.len() {
            *self = Self { width, ..Self::default() };
        }
        for message in messages.iter().skip(self.rows.len()) {
            let mine = matches!(message.role, Role::User);
            let bubble_width = if mine {
                let line = message.text.lines().map(|line| gfx.measure(line, font)).fold(0.0_f32, f32::max);
                let attachment = message.attachments.len().min(4) as f32 * 68.0 + 8.0;
                (line.max(attachment) + 20.0).clamp(76.0, width)
            } else { width };
            let text_height = gfx.wrapped_height(&message.text, font, if mine { bubble_width - 20.0 } else { bubble_width });
            let attachments = if mine && !message.attachments.is_empty() { 58.0 } else { 0.0 };
            let bubble_height = text_height + attachments + if mine { 18.0 } else { 0.0 };
            let height = bubble_height + 39.0;
            self.rows.push(MessageRow { top: self.height, width: bubble_width, attachments, bubble_height, height });
            self.height += height;
        }
    }
}

pub fn paint(frame: &Frame, theme: &Theme, chat: &mut Chat, size: (f32, f32), now: std::time::Instant) {
    paint_with_controls(frame, theme, chat, size, now, true, false);
}

pub fn paint_frosted_panel(frame: &Frame, theme: &Theme, chat: &mut Chat, size: (f32, f32), now: std::time::Instant) {
    paint_with_controls(frame, theme, chat, size, now, true, true);
}

pub fn paint_widget(frame: &Frame, theme: &Theme, chat: &mut Chat, size: (f32, f32), now: std::time::Instant) {
    paint_with_controls(frame, theme, chat, size, now, false, true);
}

fn paint_with_controls(frame: &Frame, theme: &Theme, chat: &mut Chat, size: (f32, f32), now: std::time::Instant, component_controls: bool, glass_backdrop: bool) {
    chat.poll();
    let (w, h) = size;
    let pad = 16.0;
    let text = theme.text;
    let quiet = theme.text2;
    let accent = theme.ink("cpu").trace;
    let model = crate::app().settings.lock().unwrap().simo.model.clone();
    let model = if model.trim().is_empty() { "未选择模型".to_string() } else { model };
    if paints_own_background(theme.skin, glass_backdrop) {
        frame.fill_rounded(surface_background(theme), 0.0, 0.0, w, h, theme.radius);
    }
    let outline = if theme.stroke.a > 0.0 { theme.stroke } else { theme.text3.alpha(0.28) };
    frame.stroke_rounded(Fill::Solid(outline), 0.5, 0.5, w - 1.0, h - 1.0, theme.radius, 1.0);
    if chat.expanded { paint_history_sidebar(frame, theme, chat, size); }
    let button_size = super::buttons::SIZE;
    let step = button_size + super::buttons::GAP;
    let right = w - pad - button_size / 2.0;
    let control_count = if component_controls { 4.0 } else { 1.0 };
    frame.text("SIMO", Font::new(Family::SegoeDisplay, 20.0, 600.0), text, pad, pad + 16.0, w - 2.0 * pad - control_count * step, Align::Start);
    let expand_x = if component_controls { right - 3.0 * step } else { right };
    frame.icon(if chat.expanded { super::icons::Icon::Minimize } else { super::icons::Icon::Maximize }, Point { x: expand_x, y: pad + 14.0 }, 16.0, quiet);
    if component_controls {
        for (index, button) in [super::buttons::Button::Settings, super::buttons::Button::Pin, super::buttons::Button::Close].into_iter().enumerate() {
            let center = Point { x: right - (2 - index) as f32 * step, y: pad + 14.0 };
            super::buttons::paint(frame, center, button, button == super::buttons::Button::Pin && chat.pinned, theme.dark, theme.text3.alpha(0.45), 1.0);
        }
    }
    frame.text(&model, Font::new(Family::Segoe, 12.0, 400.0), quiet, pad, pad + 38.0, w - 2.0 * pad, Align::Start);

    let input_text = chat.composer_displayed_text();
    let content_left = sidebar_width(size, chat.expanded) + pad;
    let (input_x, input_y, input_w, input_total_h) = input_rect_with_attachments(size, &input_text, frame.gfx, chat.expanded, chat.attachments.len());
    let attachment_band = attachment_band(chat.attachments.len());
    let input_y = input_y + attachment_band;
    let input_h = input_total_h - attachment_band;
    let history_bottom = input_y - attachment_band - 12.0;
    let history_top = 68.0;
    let font = Font::new(Family::Segoe, 13.0, 400.0);
    let max_bubble = (w - content_left - pad - 8.0).max(76.0);
    let messages = chat.messages.lock().unwrap();
    chat.message_layout.measure(&messages, frame.gfx, font, max_bubble);
    let edit = chat.edit.as_ref().and_then(|edit| {
        let row = chat.message_layout.rows.get(edit.index)?;
        let draft = if chat.editing_focused { chat.displayed_text() } else { edit.draft.string() };
        let height = (frame.gfx.wrapped_height(&draft, INPUT_FONT, max_bubble - 20.0) + 61.0)
            .max(96.0).min((history_bottom - history_top - 16.0).max(96.0));
        Some((edit.index, draft, height, height + 9.0 - row.height))
    });
    let status_height = if matches!(chat.state, State::Idle) { 0.0 } else { 28.0 };
    let full_height = chat.message_layout.height + edit.as_ref().map_or(0.0, |edit| edit.3) + status_height;
    let viewport_height = (history_bottom - history_top).max(0.0);
    chat.history_rect = (content_left, history_top, w - content_left - pad, viewport_height);
    chat.scroll_max = (full_height - viewport_height).max(0.0);
    chat.scroll_target = chat.scroll_target.clamp(0.0, chat.scroll_max);
    chat.scroll = chat.scroll.clamp(0.0, chat.scroll_max);
    if let (Some(editor), Some((index, _, height, _))) = (chat.edit.as_mut(), edit.as_ref()) {
        if std::mem::take(&mut editor.reveal) {
            let unscrolled_top = history_bottom - full_height + chat.message_layout.rows[*index].top;
            let show_top = history_top - unscrolled_top;
            let show_bottom = (history_bottom - unscrolled_top - height).max(0.0);
            // Keep the action buttons visible even in a cramped viewport.
            let scroll = chat.scroll.clamp(show_top.max(0.0).min(show_bottom), show_bottom);
            chat.scroll = scroll.min(chat.scroll_max);
            chat.scroll_target = chat.scroll;
        }
    }
    let elapsed = chat.scroll_at.replace(now).map_or(0.016, |last| now.saturating_duration_since(last).as_secs_f32());
    if scrolling(chat) { chat.scroll += (chat.scroll_target - chat.scroll) * (1.0 - (-elapsed / 0.06).exp()); }
    else { chat.scroll = chat.scroll_target; }
    let origin = history_bottom - full_height + chat.scroll;
    chat.reply_hits.clear();
    chat.edit_rect = None;
    let mut tip = None;
    frame.clip(content_left, history_top, w - content_left - pad, viewport_height);
    let lang = crate::update::language();
    let rows = &chat.message_layout.rows;
    let (edit_index, edit_shift) = edit.as_ref().map_or((usize::MAX, 0.0), |edit| (edit.0, edit.3));
    let mut skip = 0;
    let mut left = rows.len();
    while left > 0 {
        let step = left / 2;
        let index = skip + step;
        let row = rows[index];
        let bottom_shift = if index >= edit_index { edit_shift } else { 0.0 };
        if origin + row.top + row.height + bottom_shift < history_top {
            skip = index + 1;
            left -= step + 1;
        } else { left = step; }
    }
    for (index, (message, row)) in messages.iter().zip(rows).enumerate().skip(skip) {
        let y = origin + row.top + if index > edit_index { edit_shift } else { 0.0 };
        if y >= history_bottom { break; }
        if let Some((_, draft, height, _)) = edit.as_ref().filter(|edit| edit.0 == index) {
            let bounds = (w - pad - max_bubble, y, max_bubble, *height);
            chat.edit_rect = Some(bounds);
            let hover = chat.pointer.filter(|(x, y)| contains(chat.history_rect, *x, *y));
            frame.fill_rounded(theme.paper, bounds.0, y, bounds.2, bounds.3, 12.0);
            frame.stroke_rounded(Fill::Solid(theme.rule), bounds.0, y, bounds.2, bounds.3, 12.0, 1.0);
            let edit_draft = &chat.edit.as_ref().unwrap().draft;
            draw_editor(frame, theme, chat, draft, edit_draft, bounds, chat.editing_focused, now, accent);
            for (action, button) in edit_buttons(bounds) {
                let send = action == EditAction::Send;
                let over = hover.is_some_and(|(x, y)| contains(button, x, y));
                let color = if send { theme.text.alpha(if draft.trim().is_empty() { 0.4 } else if over { 0.85 } else { 1.0 }) } else { theme.rule.alpha(if over { 0.85 } else { 0.45 }) };
                let label = if send { lang.pick("发送", "Send") } else { lang.pick("取消", "Cancel") };
                frame.fill_rounded(color, button.0, button.1, button.2, button.3, 8.0);
                frame.text(label, INPUT_FONT, if send { theme.paper } else { text }, button.0 + (button.2 - frame.gfx.measure(label, INPUT_FONT)) / 2.0, history_label_top(frame.gfx, label, INPUT_FONT, button.1 + button.3 / 2.0), button.2, Align::Start);
            }
            continue;
        }
        let mine = matches!(message.role, Role::User);
        let x = if mine { w - pad - row.width } else { content_left };
        let footer_y = y + row.bubble_height + 4.0;
        let footer_left = if mine { (w - pad - 54.0).max(content_left) } else { x };
        let hit = ReplyHit { index, left: footer_left, footer_y, user: mine };
        let hovered = chat.pointer.filter(|(x, y)| contains(chat.history_rect, *x, *y));
        let over_row = hovered.is_some_and(|(px, py)| contains((x.min(footer_left - if mine { 54.0 } else { 0.0 }), y, w - pad - x.min(footer_left - if mine { 54.0 } else { 0.0 }), row.bubble_height + 30.0), px, py));
        if mine {
            let bubble = if theme.dark { super::canvas::Color::hex(0x3A3A3C, 0.96) } else { super::canvas::Color::hex(0xE8E8EB, 0.96) };
            frame.fill_rounded(bubble, x, y, row.width, row.bubble_height, 8.0);
            if row.attachments > 0.0 { paint_message_attachments(frame, theme, &message.attachments, x + 8.0, y + 7.0, row.width - 16.0); }
            frame.text_wrapped(&message.text, font, text, x + 10.0, y + 9.0 + row.attachments, row.width - 20.0);
        } else {
            frame.text_wrapped(&message.text, font, text, x, y, row.width);
        }
        if footer_y + 26.0 <= history_top || footer_y >= history_bottom || (mine && !over_row) { continue; }
        let (busy, error) = if mine { (false, None) } else { chat.reply_audio(index) };
        let copied = chat.copy_feedback.filter(|(row, _, at)| *row == index && now.saturating_duration_since(*at).as_secs() < 2).map(|(_, copied, _)| copied);
        let copy_icon = match copied { Some(true) => super::icons::Icon::Check, Some(false) => super::icons::Icon::CircleX, None => super::icons::Icon::Copy };
        let over = hovered.and_then(|(x, y)| hit.action(x, y));
        let secondary = if mine { (ReplyAction::Edit, super::icons::Icon::SquarePen) } else {
            (ReplyAction::Play, if busy { super::icons::Icon::Ellipsis } else { super::icons::Icon::Volume })
        };
        for (slot, (action, icon)) in [(ReplyAction::Copy, copy_icon), secondary].into_iter().enumerate() {
            let left = hit.left + slot as f32 * 28.0;
            let active = over == Some(action);
            if active {
                frame.fill_rounded(theme.rule.alpha(0.48), left, footer_y, 26.0, 26.0, 5.0);
                let label = match action {
                    ReplyAction::Copy => lang.pick("复制消息", "Copy message"),
                    ReplyAction::Edit => lang.pick("编辑消息", "Edit message"),
                    ReplyAction::Play => lang.pick("语音播放", "Read aloud"),
                };
                tip = Some((label, left + 13.0, footer_y));
            }
            let failed = (action == ReplyAction::Play && error.is_some()) || (action == ReplyAction::Copy && copied == Some(false));
            let ink = if failed { theme.signal } else if active { text } else { quiet.alpha(0.76) };
            frame.icon(icon, Point { x: left + 13.0, y: footer_y + 13.0 }, 15.0, ink);
        }
        if over_row {
            let time_font = Font::new(Family::Segoe, 11.0, 400.0);
            if let Some(stamp) = message.sent_at {
                let local = super::text::Local::at(stamp);
                let time = format!("{:02}:{:02}", local.hour, local.minute);
                let left = if mine { footer_left - 52.0 } else { footer_left + 63.0 };
                frame.text(&time, time_font, quiet, left, history_label_top(frame.gfx, &time, time_font, footer_y + 13.0), 48.0, if mine { Align::End } else { Align::Start });
            }
            if let Some(error) = error {
                frame.text(error, time_font, theme.signal, footer_left + 111.0, history_label_top(frame.gfx, error, time_font, footer_y + 13.0), (row.width - 111.0).max(1.0), Align::Start);
            }
        }
        chat.reply_hits.push(hit);
    }
    drop(messages);
    if chat.scroll < 1.0 && matches!(chat.state, State::Waiting) {
        static ANIMATION_EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
        let epoch = *ANIMATION_EPOCH.get_or_init(|| now);
        let pulse = (now.saturating_duration_since(epoch).as_millis() / 220 % 3) as usize;
        for index in 0..3 {
            let color = quiet.alpha(if index == pulse { 1.0 } else { 0.28 });
            frame.text("·", Font::new(Family::Segoe, 18.0, 600.0), color, content_left + index as f32 * 10.0, history_bottom - 18.0, 10.0, Align::Start);
        }
    }
    if let State::Error(error) = &chat.state {
        if chat.scroll < 1.0 { frame.text_wrapped(error, Font::new(Family::Segoe, 12.0, 400.0), theme.signal, content_left, history_bottom - 30.0, w - content_left - pad); }
    }

    frame.unclip();
    if chat.scroll_max > 0.0 {
        let thumb = (viewport_height * viewport_height / full_height).max(20.0).min(viewport_height);
        let top = history_top + (viewport_height - thumb) * (1.0 - chat.scroll / chat.scroll_max);
        frame.fill_rounded(theme.text2.alpha(0.45), w - pad + 4.0, top, 3.0, thumb, 1.5);
    }
    if let Some((label, center, top)) = tip {
        let font = Font::new(Family::Segoe, 11.0, 500.0);
        let width = frame.gfx.measure(label, font) + 16.0;
        let x = (center - width / 2.0).clamp(content_left, (w - pad - width).max(content_left));
        let y = if top - 27.0 >= history_top { top - 27.0 } else { top + 27.0 };
        frame.fill_rounded(theme.paper, x, y, width, 24.0, 5.0);
        frame.stroke_rounded(Fill::Solid(theme.rule), x, y, width, 24.0, 5.0, 1.0);
        frame.text(label, font, text, x + 8.0, history_label_top(frame.gfx, label, font, y + 12.0), width - 16.0, Align::Start);
    }
    frame.fill_rounded(theme.paper, input_x, input_y - attachment_band, input_w, input_total_h, 8.0);
    paint_attachments(frame, theme, chat, size, &input_text);
    frame.stroke_rounded(Fill::Solid(if chat.focused && !chat.editing_focused { accent } else { theme.rule }), input_x, input_y - attachment_band, input_w, input_total_h, 8.0, 1.0);
    draw_editor(frame, theme, chat, &input_text, &chat.draft, (input_x, input_y, input_w, input_h), !chat.editing_focused, now, accent);
    for (action, point) in input_controls(size) {
        let (icon, ink) = if action == InputAction::Attach { (super::icons::Icon::Paperclip, quiet) } else { (super::icons::Icon::Send, accent) };
        frame.icon(icon, point, 15.0, ink);
    }
}

fn draw_editor(frame: &Frame, theme: &Theme, chat: &Chat, input_text: &str, typing: &super::typing::Typing, bounds: Bounds, active: bool, now: std::time::Instant, accent: super::canvas::Color) {
    let text = theme.text;
    let quiet = theme.text2;
    let draft = if input_text.is_empty() && !chat.editing_focused { "Ask SIMO…" } else { &input_text };
    let composer = composer_text(frame.gfx, input_text, if active { chat.displayed_caret() } else { typing.caret }, bounds);
    let (x, y, width, height) = composer.bounds;
    frame.clip(x - 2.0, y, width + 4.0, height);
    let text_y = y - composer.scroll;
    let selection = typing.selection();
    if active && chat.focused && (!selection.is_empty() || !chat.composition.is_empty()) {
        use windows::Win32::Graphics::DirectWrite::DWRITE_HIT_TEST_METRICS;
        let mut count = 0;
        let start = selection.start as u32;
        let composing = !chat.composition.is_empty();
        let length = if composing { chat.composition.encode_utf16().count() } else { selection.len() } as u32;
        let _ = unsafe { composer.layout.HitTestTextRange(start, length, 0.0, 0.0, None, &mut count) };
        let mut spans = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
        if unsafe { composer.layout.HitTestTextRange(start, length, 0.0, 0.0, Some(&mut spans), &mut count) }.is_ok() {
            for span in spans {
                if composing {
                    frame.fill(text.alpha(0.8), x + span.left, text_y + span.top + span.height - 1.0, span.width.max(2.0), 1.0);
                } else {
                    frame.fill(accent.alpha(0.32), x + span.left, text_y + span.top, span.width.max(2.0), span.height);
                }
            }
        }
    }
    frame.text_wrapped(draft, INPUT_FONT, if input_text.is_empty() { quiet } else { text }, x, text_y, width);
    if active && chat.caret_visible(now) && (selection.is_empty() || !chat.composition.is_empty()) {
        let (caret_x, caret_y, caret_height) = composer.caret;
        frame.fill(text, x + caret_x.min(width), text_y + caret_y, 1.0, caret_height);
    }
    frame.unclip();
}

const INPUT_FONT: Font = Font::new(Family::Segoe, 13.0, 400.0);

struct ComposerText {
    bounds: Bounds,
    layout: windows::Win32::Graphics::DirectWrite::IDWriteTextLayout,
    caret: (f32, f32, f32),
    scroll: f32,
}

fn composer_text(gfx: &Gfx, text: &str, caret_index: usize, (x, y, width, height): Bounds) -> ComposerText {
    let width = (width - 20.0).max(1.0);
    let height = (height - 42.0).max(18.0);
    let layout = gfx.wrapped(text, INPUT_FONT, width);
    let caret = if text.is_empty() { (0.0, 0.0, 18.0) } else { super::typing::caret_at(&layout, caret_index) };
    let scroll = (super::typing::height_of(&layout) - height).max(0.0).min(caret.1);
    ComposerText { bounds: (x + 10.0, y + 9.0, width, height), layout, caret, scroll }
}

fn composer_body(size: (f32, f32), chat: &Chat, text: &str, gfx: &Gfx) -> Bounds {
    let strip = attachment_band(chat.attachments.len());
    let (x, y, width, height) = input_rect_with_attachments(size, text, gfx, chat.expanded, chat.attachments.len());
    (x, y + strip, width, height - strip)
}

pub fn select_draft(chat: &mut Chat, size: (f32, f32), gfx: &Gfx, hwnd: windows::Win32::Foundation::HWND, x: f32, y: f32) {
    chat.focus_editor(false);
    let bounds = composer_body(size, chat, &chat.displayed_text(), gfx);
    begin_selection(chat, gfx, hwnd, bounds, x, y);
}

fn begin_selection(chat: &mut Chat, gfx: &Gfx, hwnd: windows::Win32::Foundation::HWND, bounds: Bounds, x: f32, y: f32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, GetKeyState, SetCapture, VK_SHIFT};
    if !chat.composition.is_empty() { return; }
    place_caret(chat, gfx, bounds, x, y, unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0);
    unsafe { SetCapture(hwnd); }
    chat.selecting_text = unsafe { GetCapture() } == hwnd;
}

pub fn follow_selection(chat: &mut Chat, hwnd: windows::Win32::Foundation::HWND, gfx: &Gfx, size: (f32, f32), x: f32, y: f32, message: u32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetCapture;
    use windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP;
    if unsafe { GetCapture() } != hwnd {
        chat.selecting_text = false;
        return;
    }
    if message == super::window::CAPTURE_LOST { return; }
    if chat.focused {
        let bounds = if chat.editing_focused { chat.edit_rect } else { Some(composer_body(size, chat, &chat.displayed_text(), gfx)) };
        if let Some(bounds) = bounds { place_caret(chat, gfx, bounds, x, y, true); }
    }
    if message == WM_LBUTTONUP {
        chat.selecting_text = false;
        super::window::let_go_of_capture(hwnd);
    }
}

fn place_caret(chat: &mut Chat, gfx: &Gfx, bounds: Bounds, x: f32, y: f32, extend: bool) {
    if !chat.composition.is_empty() { return; }
    let text = chat.displayed_text();
    let composer = composer_text(gfx, &text, chat.displayed_caret(), bounds);
    let (left, top, _, _) = composer.bounds;
    let offset = super::typing::at_point(&composer.layout, x - left, y - top + composer.scroll);
    chat.active_draft_mut().go(offset, extend);
    chat.cursor_epoch = std::time::Instant::now();
}

pub fn edit_input(chat: &mut Chat, hwnd: windows::Win32::Foundation::HWND, message: u32, key: u16) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_A, VK_BACK, VK_C, VK_CONTROL, VK_DELETE, VK_LEFT, VK_MENU, VK_RIGHT, VK_SHIFT, VK_V};
    use windows::Win32::UI::WindowsAndMessaging::{WM_COPY, WM_KEYDOWN, WM_PASTE};

    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    let control = held(VK_CONTROL) && !held(VK_MENU);
    let action = match (message, VIRTUAL_KEY(key), control) {
        (WM_KEYDOWN, VK_C, true) | (WM_COPY, _, _) => WM_COPY,
        (WM_KEYDOWN, VK_V, true) | (WM_PASTE, _, _) => WM_PASTE,
        _ => message,
    };
    match action {
        WM_COPY => {
            let selection = chat.active_draft().selection();
            if !selection.is_empty() {
                let text = String::from_utf16_lossy(&chat.active_draft().text[selection]).replace('\n', "\r\n");
                let _ = crate::clipboard::put(hwnd, &[(crate::clipboard::TEXT, crate::clipboard::utf16_z(&text))]);
            }
        }
        WM_PASTE if chat.composition.is_empty() => {
            if let Some(text) = crate::clipboard::text(hwnd).filter(|text| !text.is_empty()) {
                chat.insert_text(&text);
            }
        }
        WM_PASTE => {
            // Close the active native composition before inserting plain text.
            if let Some(text) = crate::clipboard::text(hwnd).filter(|text| !text.is_empty()) {
                use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmNotifyIME, ImmReleaseContext, CPS_CANCEL, NI_COMPOSITIONSTR};
                unsafe {
                    let context = ImmGetContext(hwnd);
                    if !context.is_invalid() {
                        let _ = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
                        let _ = ImmReleaseContext(hwnd, context);
                    }
                }
                chat.insert_text(&text);
            }
        }
        WM_KEYDOWN if chat.composition.is_empty() => {
            let shift = held(VK_SHIFT);
            match VIRTUAL_KEY(key) {
                VK_A if control => {
                    chat.active_draft_mut().go(0, false);
                    let end = chat.active_draft().text.len();
                    chat.active_draft_mut().go(end, true);
                }
                VK_LEFT | VK_RIGHT if !control => {
                    let forward = key == VK_RIGHT.0;
                    let selection = chat.active_draft().selection();
                    let next = if !shift && !selection.is_empty() {
                        if forward { selection.end } else { selection.start }
                    } else {
                        chat.active_draft().step(chat.active_draft().caret, forward)
                    };
                    chat.active_draft_mut().go(next, shift);
                }
                VK_BACK | VK_DELETE => chat.active_draft_mut().erase(key == VK_DELETE.0),
                _ => return false,
            }
            chat.cursor_epoch = std::time::Instant::now();
        }
        _ => return false,
    }
    true
}

pub fn ime_strings(hwnd: windows::Win32::Foundation::HWND, flags: isize) -> (Option<String>, Option<String>) {
    use windows::Win32::UI::Input::Ime::{ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext, GCS_COMPSTR, GCS_RESULTSTR};
    let context = unsafe { ImmGetContext(hwnd) };
    if context.is_invalid() {
        return (None, None);
    }
    let read = |kind| {
        let bytes = unsafe { ImmGetCompositionStringW(context, kind, None, 0) };
        if bytes < 0 {
            return None;
        }
        let mut words = vec![0u16; bytes as usize / 2];
        if !words.is_empty() {
            let read = unsafe { ImmGetCompositionStringW(context, kind, Some(words.as_mut_ptr().cast()), bytes as u32) };
            if read < 0 {
                return None;
            }
            words.truncate(read as usize / 2);
        }
        Some(String::from_utf16_lossy(&words))
    };
    let flags = flags as u32;
    let composition = if flags == 0 { Some(String::new()) }
        else { (flags & GCS_COMPSTR.0 != 0).then(|| read(GCS_COMPSTR)).flatten() };
    let result = (flags & GCS_RESULTSTR.0 != 0).then(|| read(GCS_RESULTSTR)).flatten();
    unsafe { let _ = ImmReleaseContext(hwnd, context); }
    (composition, result)
}

pub fn place_ime(hwnd: windows::Win32::Foundation::HWND, size: (f32, f32), chat: &Chat, gfx: &Gfx, scale: f32, origin: (i32, i32)) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, CANDIDATEFORM, CFS_EXCLUDE};
    let text = chat.displayed_text();
    let bounds = if chat.editing_focused {
        let Some(bounds) = chat.edit_rect else { return; };
        bounds
    } else { composer_body(size, chat, &text, gfx) };
    let composer = composer_text(gfx, &text, chat.displayed_caret(), bounds);
    let (left, top, width, _) = composer.bounds;
    let x = left + composer.caret.0.min(width);
    let y = top + composer.caret.1 - composer.scroll;
    let left = origin.0 + (x * scale).round() as i32;
    let top = origin.1 + (y * scale).round() as i32;
    let bottom = origin.1 + ((bounds.1 + bounds.3).max(y + 18.0) * scale).ceil() as i32;
    unsafe {
        let context = ImmGetContext(hwnd);
        if context.is_invalid() { return; }
        // The composition is already in the draft; only the native candidate list is positioned.
        let _ = ImmSetCandidateWindow(context, &CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: left, y: bottom },
            rcArea: RECT { left, top, right: left + 1, bottom },
        });
        let _ = ImmReleaseContext(hwnd, context);
    }
}

pub fn input_rect_with_attachments(size: (f32, f32), draft: &str, gfx: &Gfx, expanded: bool, attachments: usize) -> Bounds {
    let left = sidebar_width(size, expanded) + 16.0;
    let width = size.0 - left - 16.0;
    let text_width = (width - 20.0).max(1.0);
    let text_height = if draft.is_empty() { 0.0 } else { gfx.wrapped_height(draft, INPUT_FONT, text_width) };
    let (top, height) = input_vertical(size.1, text_height, attachments);
    (left, top, width, height)
}

fn attachment_band(attachments: usize) -> f32 {
    if attachments == 0 { 0.0 } else { 94.0 }
}

fn input_vertical(height: f32, text_height: f32, attachments: usize) -> (f32, f32) {
    let strip = attachment_band(attachments);
    let total = input_height(text_height + 50.0, height - 128.0 - strip) + strip;
    (height - 16.0 - total, total)
}

fn input_height(content_height: f32, max_height: f32) -> f32 {
    74.0_f32.max(content_height).min(max_height.max(74.0))
}

fn paints_own_background(skin: super::theme::Skin, skin_ground: bool) -> bool {
    !skin.sees_backdrop() || !skin_ground
}

fn surface_background(theme: &Theme) -> super::canvas::Color {
    match theme.skin {
        super::theme::Skin::Paper => theme.paper,
        super::theme::Skin::Fluent => theme.luminosity,
        super::theme::Skin::Glass => theme.glass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simo_conversation_control_says_expand_or_collapse() {
        let lang = super::super::text::Lang::Zh;
        assert_eq!(action_label(Action::Expand, false, false, lang), "展开对话");
        assert_eq!(action_label(Action::Expand, true, false, lang), "收起对话");
    }

    #[test]
    fn glass_panel_keeps_its_own_background_but_float_uses_backdrop() {
        use super::super::theme::Skin;
        assert!(paints_own_background(Skin::Glass, false));
        assert!(!paints_own_background(Skin::Glass, true));
        assert!(paints_own_background(Skin::Paper, true));
        assert!(!paints_own_background(Skin::Fluent, true));
    }

    #[test]
    fn header_text_is_a_drag_handle_but_controls_are_not() {
        let size = (WIDTH, HEIGHT);
        assert!(header_drag_handle(size, 120.0, 28.0));
        assert!(!header_drag_handle(size, 315.0, 28.0));
        assert!(!header_drag_handle(size, 380.0, 28.0));
        assert!(!header_drag_handle(size, 120.0, 70.0));
    }

    #[test]
    fn desktop_header_only_hits_its_expand_control() {
        let size = (WIDTH, HEIGHT);
        let step = super::super::buttons::SIZE + super::super::buttons::GAP;
        let right = size.0 - 16.0 - super::super::buttons::SIZE / 2.0;
        assert!(matches!(widget_header_action(size, right, 30.0), Some(Action::Expand)));
        assert!(widget_header_action(size, right - step, 30.0).is_none());
        assert!(widget_header_action(size, right, 60.0).is_none());
    }

    #[test]
    fn desktop_widget_blank_area_can_be_dragged_repeatedly_but_controls_cannot() {
        let size = (WIDTH, HEIGHT);
        let input = (16.0, size.1 - 90.0, size.0 - 32.0, 74.0);
        let expand_x = size.0 - 16.0 - super::super::buttons::SIZE / 2.0;
        for _ in 0..2 {
            assert!(widget_drag_handle(size, input, 100.0, 100.0));
        }
        assert!(!widget_drag_handle(size, input, expand_x, 30.0));
        assert!(!widget_drag_handle(size, input, 100.0, input.1 + 10.0));
        assert!(!widget_drag_handle(size, input, -1.0, 100.0));
    }

    #[test]
    fn attachments_leave_the_composer_inside_the_panel() {
        let (top, height) = input_vertical(HEIGHT, 0.0, 1);
        assert_eq!(top + height, HEIGHT - 16.0);
        assert_eq!(top + attachment_band(1), input_vertical(HEIGHT, 0.0, 0).0);
    }

    #[test]
    fn attachment_previews_stay_inside_the_composer_outline() {
        let gfx = Gfx::new().unwrap();
        let size = (WIDTH, HEIGHT);
        for count in 1..=4 {
            let input = input_rect_with_attachments(size, "draft text", &gfx, false, count);
            for index in 0..count {
                let Some((x, y, width, height)) = attachment_rect(size, "draft text", &gfx, false, index) else {
                    continue;
                };
                assert!(x >= input.0 && y >= input.1);
                assert!(x + width <= input.0 + input.2);
                assert!(y + height <= input.1 + input.3);
            }
        }
    }

    #[test]
    fn input_buttons_do_not_capture_text_above_their_icons() {
        let size = (WIDTH, HEIGHT);
        for (action, point) in input_controls(size) {
            assert_eq!(input_action_at(size, point.x, point.y), action);
            assert_eq!(input_action_at(size, point.x, point.y - 42.0), InputAction::Text);
        }
    }

    #[test]
    fn reply_buttons_take_only_their_footer_targets() {
        let footer = ReplyHit { index: 2, left: 16.0, footer_y: 195.0, user: false };
        assert_eq!(footer.action(29.0, 208.0), Some(ReplyAction::Copy));
        assert_eq!(footer.action(57.0, 208.0), Some(ReplyAction::Play));
        assert_eq!(footer.action(90.0, 208.0), None);
        assert_eq!(footer.action(29.0, 180.0), None);
    }

    #[test]
    fn history_labels_fit_inside_their_rows_with_windows_font_metrics() {
        let gfx = Gfx::new().unwrap();
        for text in ["新对话", "这是一条历史记录", "New chat", "Conversation history"] {
            let font = Font::new(Family::Segoe, 12.0, 600.0);
            let (top, bottom) = gfx.ink(text, font);
            let y = history_label_top(&gfx, text, font, HISTORY_START + 15.0);
            assert!(bottom > top, "missing glyph bounds for {text}");
            assert!(y + top >= HISTORY_START);
            assert!(y + bottom <= HISTORY_START + HISTORY_STEP - 4.0);
        }
    }

    #[test]
    fn reduced_history_sidebar_uses_only_visible_rows() {
        let height = 420.0;
        let rows = history_rows(height);
        assert!(HISTORY_START + rows as f32 * HISTORY_STEP <= height - 52.0);
        assert!(HISTORY_START + (rows + 1) as f32 * HISTORY_STEP > height - 52.0);
        let mut chat = Chat::default();
        assert_eq!(history_action(&chat, (620.0, height), 30.0, NEW_CHAT_CENTER), None);
        chat.expanded = true;
        assert_eq!(history_action(&chat, (620.0, height), 30.0, NEW_CHAT_CENTER), Some(HistoryAction::New));
    }

    #[test]
    fn input_grows_with_wrapped_text_and_respects_available_height() {
        assert_eq!(input_height(0.0, 448.0), 74.0);
        assert_eq!(input_height(92.0, 448.0), 92.0);
        assert_eq!(input_height(500.0, 448.0), 448.0);
    }
}
