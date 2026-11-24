//! SIMO's provider and attachment boundary.
//!
//! The UI owns the conversation state and calls this module from worker
//! threads. Network work is deliberately synchronous here so it never runs on
//! the panel thread.

use std::ffi::c_void;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_URL: &str = "https://api.tokendos.com/v1";
pub const DEFAULT_TTS_BASE_URL: &str = "https://api.tokendos.com/v1";

/// Settings kept in Glance's normal settings.json file. The API key is only
/// read when a request is made and is never included in logs or diagnostics.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub api_base_url: String,
    pub api_key: String,
    pub model: String,
    pub models: Vec<String>,
    pub voice_output: bool,
    pub tts_base_url: String,
    pub tts_model: String,
    pub tts_voice: String,
    pub storage_dir: Option<std::path::PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            api_base_url: DEFAULT_BASE_URL.into(),
            api_key: String::new(),
            model: String::new(),
            models: Vec::new(),
            voice_output: false,
            tts_base_url: DEFAULT_TTS_BASE_URL.into(),
            tts_model: "breeze".into(),
            tts_voice: "alloy".into(),
            storage_dir: None,
        }
    }
}

pub fn storage_dir(settings: &Settings) -> std::path::PathBuf {
    settings.storage_dir.clone().unwrap_or_else(|| crate::settings::local_dir().join("simo"))
}

pub fn move_storage(from: &Path, to: &Path) -> Result<(), String> {
    if from == to || !from.exists() {
        return Ok(());
    }
    if to.starts_with(from) {
        return Err("SIMO storage cannot be moved inside its current directory".into());
    }
    copy_tree(from, to).map_err(|error| format!("could not move SIMO storage: {error}"))?;
    if let Some(repositories) = REPOSITORIES.get() {
        let mut repositories = repositories.lock().unwrap();
        if let Some(repository) = repositories.remove(from) {
            repository.lock().unwrap().path = to.to_path_buf();
            repositories.insert(to.to_path_buf(), repository);
        }
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else if !target.exists() {
            std::fs::copy(source, target)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Waiting,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub sent_at: Option<u64>,
    pub role: Role,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug)]
pub struct ConversationInfo {
    pub id: String,
    pub title: String,
    pub updated: u64,
}

#[derive(Serialize, Deserialize)]
struct StoredAttachment {
    name: String,
    mime: String,
    extracted_text: Option<String>,
    file: String,
}

#[derive(Serialize, Deserialize)]
struct StoredMessage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sent_at: Option<u64>,
    role: Role,
    text: String,
    attachments: Vec<StoredAttachment>,
}

#[derive(Serialize, Deserialize)]
struct StoredConversation {
    id: String,
    title: String,
    updated: u64,
    messages: Vec<StoredMessage>,
}

struct Conversation {
    info: ConversationInfo,
    messages: Arc<Mutex<Vec<ChatMessage>>>,
}

#[derive(Default)]
struct Repository {
    path: PathBuf,
    conversations: Vec<Conversation>,
}

static REPOSITORIES: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<Repository>>>>> = OnceLock::new();

pub struct MessageEdit {
    pub index: usize,
    pub draft: crate::ui::typing::Typing,
    pub reveal: bool,
}

pub struct Chat {
    pub edit: Option<MessageEdit>,
    pub editing_focused: bool,
    pub selecting_text: bool,
    pub edit_rect: Option<(f32, f32, f32, f32)>,
    pub history_rect: (f32, f32, f32, f32),
    pub scroll: f32,
    pub scroll_target: f32,
    pub scroll_max: f32,
    pub scroll_at: Option<std::time::Instant>,
    pub message_layout: crate::ui::simo::MessageLayout,
    pub messages: Arc<Mutex<Vec<ChatMessage>>>,
    pub draft: crate::ui::typing::Typing,
    pub composition: String,
    pub focused: bool,
    pub cursor_epoch: std::time::Instant,
    pending_high_surrogate: Option<u16>,
    pub state: State,
    pub attachments: Vec<Attachment>,
    pub expanded: bool,
    pub conversation_id: String,
    pub history_page: usize,
    pub hovered_attachment: Option<usize>,
    pub hovered_action: Option<u8>,
    pub pointer: Option<(f32, f32)>,
    pub reply_hits: Vec<crate::ui::simo::ReplyHit>,
    pub copy_feedback: Option<(usize, bool, std::time::Instant)>,
    playback: Option<(String, usize, JoinHandle<Result<(), String>>)>,
    playback_error: Option<(String, usize, String)>,
    pub pinned: bool,
    repository: Option<Arc<Mutex<Repository>>>,
    request: Option<(String, Arc<Mutex<Vec<ChatMessage>>>, JoinHandle<Result<(usize, String), String>>)>,
}

impl Default for Chat {
    fn default() -> Self {
        Self { edit: None, editing_focused: false, selecting_text: false, edit_rect: None, history_rect: (0.0, 0.0, 0.0, 0.0), scroll: 0.0, scroll_target: 0.0, scroll_max: 0.0, scroll_at: None, message_layout: Default::default(), messages: Arc::new(Mutex::new(Vec::new())), draft: crate::ui::typing::Typing::most(usize::MAX), composition: String::new(), focused: false, cursor_epoch: std::time::Instant::now(), pending_high_surrogate: None, state: State::Idle, attachments: Vec::new(), expanded: false, conversation_id: String::new(), history_page: 0, hovered_attachment: None, hovered_action: None, pointer: None, reply_hits: Vec::new(), copy_feedback: None, playback: None, playback_error: None, pinned: false, repository: None, request: None }
    }
}

impl Chat {
    pub fn open(settings: &Settings) -> Self {
        let path = storage_dir(settings);
        let repositories = REPOSITORIES.get_or_init(|| Mutex::new(HashMap::new()));
        let repository = repositories.lock().unwrap().entry(path.clone()).or_insert_with(|| Arc::new(Mutex::new(Repository::open(path)))).clone();
        let mut chat = Self::default();
        chat.repository = Some(repository.clone());
        let mut repository = repository.lock().unwrap();
        if repository.conversations.is_empty() {
            let conversation = Conversation::new();
            let id = conversation.info.id.clone();
            repository.conversations.push(conversation);
            repository.save_all(&id);
        }
        let current = repository.conversations.iter().max_by_key(|item| item.info.updated).unwrap();
        chat.conversation_id = current.info.id.clone();
        chat.messages = current.messages.clone();
        chat
    }

    pub fn conversations(&self) -> Vec<ConversationInfo> {
        self.repository.as_ref().map(|repository| {
            let mut conversations = repository.lock().unwrap().conversations.iter().map(|item| item.info.clone()).collect::<Vec<_>>();
            conversations.sort_by_key(|item| std::cmp::Reverse(item.updated));
            conversations
        }).unwrap_or_default()
    }

    pub fn new_conversation(&mut self) {
        let Some(repository) = &self.repository else { return };
        let mut repository = repository.lock().unwrap();
        let conversation = Conversation::new();
        self.conversation_id = conversation.info.id.clone();
        self.messages = conversation.messages.clone();
        let id = self.conversation_id.clone();
        repository.conversations.push(conversation);
        repository.save_all(&id);
        drop(repository);
        self.draft.text.clear();
        self.draft.go(0, false);
        self.attachments.clear();
        self.hovered_attachment = None;
        self.reset_history_view();
        self.copy_feedback = None;
        self.state = if self.request.as_ref().is_some_and(|(id, ..)| id == &self.conversation_id) { State::Waiting } else { State::Idle };
    }

    pub fn select_conversation(&mut self, id: &str) -> bool {
        let Some(repository) = &self.repository else { return false };
        let repository = repository.lock().unwrap();
        let Some(conversation) = repository.conversations.iter().find(|item| item.info.id == id) else { return false };
        self.conversation_id = conversation.info.id.clone();
        self.messages = conversation.messages.clone();
        drop(repository);
        self.draft.text.clear();
        self.draft.go(0, false);
        self.composition.clear();
        self.attachments.clear();
        self.hovered_attachment = None;
        self.reset_history_view();
        self.copy_feedback = None;
        self.state = if self.request.as_ref().is_some_and(|(id, ..)| id == &self.conversation_id) { State::Waiting } else { State::Idle };
        self.blur();
        true
    }

    pub fn delete_conversation(&mut self, id: &str) -> std::io::Result<()> {
        if let Some(repository) = &self.repository {
            repository.lock().unwrap().delete(id)?;
        }
        self.refresh_selection();
        self.history_page = 0;
        Ok(())
    }

    // Another SIMO window can delete this window's selection while it is open.
    fn refresh_selection(&mut self) {
        if self.conversation_id.is_empty() { return; }
        let Some(repository) = &self.repository else { return };
        let (replacement, request_deleted) = {
            let repository = repository.lock().unwrap();
            let exists = |id: &str| repository.conversations.iter().any(|entry| entry.info.id == id);
            let request_deleted = self.request.as_ref().is_some_and(|(id, ..)| !exists(id));
            let replacement = (!exists(&self.conversation_id)).then(|| {
                repository.conversations.iter().max_by_key(|entry| entry.info.updated)
                    .map(|entry| (entry.info.id.clone(), entry.messages.clone()))
            });
            (replacement, request_deleted)
        };
        if request_deleted { self.request = None; }
        if let Some(replacement) = replacement {
            let (id, messages) = replacement.unwrap_or_default();
            self.conversation_id = id;
            self.messages = messages;
            self.draft.text.clear();
            self.draft.go(0, false);
            self.attachments.clear();
            self.hovered_attachment = None;
            self.reset_history_view();
            self.copy_feedback = None;
            self.history_page = 0;
            self.state = if self.request.as_ref().is_some_and(|(id, ..)| id == &self.conversation_id) { State::Waiting } else { State::Idle };
            self.blur();
        }
    }

    fn persist(&self) {
        if let Some(repository) = &self.repository {
            repository.lock().unwrap().save_all(&self.conversation_id);
        }
    }

    pub fn requesting(&self) -> bool {
        self.request.is_some() || self.playback.is_some()
    }

    fn reset_history_view(&mut self) {
        self.cancel_edit();
        self.reply_hits.clear();
        self.scroll = 0.0;
        self.scroll_target = 0.0;
        self.scroll_max = 0.0;
        self.message_layout = Default::default();
    }

    pub fn active_draft(&self) -> &crate::ui::typing::Typing {
        if self.editing_focused { &self.edit.as_ref().unwrap().draft } else { &self.draft }
    }

    pub fn active_draft_mut(&mut self) -> &mut crate::ui::typing::Typing {
        if self.editing_focused { &mut self.edit.as_mut().unwrap().draft } else { &mut self.draft }
    }

    pub fn begin_edit(&mut self, index: usize) -> bool {
        if self.request.is_some() { return false; }
        let messages = self.messages.lock().unwrap();
        let Some(message) = messages.get(index).filter(|message| matches!(message.role, Role::User)) else { return false };
        let mut draft = crate::ui::typing::Typing::most(usize::MAX);
        draft.put(&message.text.encode_utf16().collect::<Vec<_>>());
        drop(messages);
        self.edit = Some(MessageEdit { index, draft, reveal: true });
        self.focus_editor(true);
        true
    }

    pub fn cancel_edit(&mut self) {
        if self.editing_focused { self.blur(); }
        self.editing_focused = false;
        self.edit = None;
        self.edit_rect = None;
    }

    pub fn focus_editor(&mut self, editing: bool) {
        if self.editing_focused != editing {
            self.composition.clear();
            self.pending_high_surrogate = None;
            self.cursor_epoch = std::time::Instant::now();
        }
        self.editing_focused = editing;
        self.focus();
    }

    pub fn send_edit(&mut self, settings: Settings) {
        self.refresh_selection();
        let Some(edit) = &self.edit else { return };
        let text = edit.draft.string();
        if text.trim().is_empty() || self.request.is_some() || (self.editing_focused && !self.composition.is_empty()) { return; }
        let Some(repository) = &self.repository else { return };
        let messages = self.messages.lock().unwrap();
        let Some(original) = messages.get(edit.index) else { return };
        let mut branch = messages[..edit.index].to_vec();
        branch.push(ChatMessage { sent_at: Some(now_ms()), role: Role::User, text, attachments: original.attachments.clone() });
        drop(messages);
        let mut conversation = Conversation::new();
        conversation.messages = Arc::new(Mutex::new(branch));
        self.conversation_id = conversation.info.id.clone();
        self.messages = conversation.messages.clone();
        repository.lock().unwrap().conversations.push(conversation);
        self.history_page = 0;
        self.copy_feedback = None;
        self.reset_history_view();
        self.persist();
        self.request_reply(settings);
    }

    pub fn focus(&mut self) {
        if !self.focused {
            self.focused = true;
            self.cursor_epoch = std::time::Instant::now();
        }
    }

    pub fn blur(&mut self) {
        self.focused = false;
        self.composition.clear();
        self.pending_high_surrogate = None;
    }

    pub fn composer_displayed_text(&self) -> String {
        if self.editing_focused { self.draft.string() } else { self.displayed_text() }
    }

    pub fn displayed_text(&self) -> String {
        if self.composition.is_empty() { return self.active_draft().string(); }
        let selected = self.active_draft().selection();
        let mut text = String::from_utf16_lossy(&self.active_draft().text[..selected.start]);
        text.push_str(&self.composition);
        text.push_str(&String::from_utf16_lossy(&self.active_draft().text[selected.end..]));
        text
    }

    pub fn displayed_caret(&self) -> usize {
        if self.composition.is_empty() { self.active_draft().caret }
        else { self.active_draft().selection().start + self.composition.encode_utf16().count() }
    }

    pub fn insert_text(&mut self, text: &str) {
        self.active_draft_mut().put(&text.encode_utf16().collect::<Vec<_>>());
        self.composition.clear();
        self.pending_high_surrogate = None;
        self.cursor_epoch = std::time::Instant::now();
    }

    pub fn caret_visible(&self, now: std::time::Instant) -> bool {
        self.focused && (now.saturating_duration_since(self.cursor_epoch).as_millis() / 530).is_multiple_of(2)
    }

    pub fn type_utf16(&mut self, unit: u16) {
        if (0xD800..=0xDBFF).contains(&unit) {
            self.pending_high_surrogate = Some(unit);
            return;
        }
        let mut units = Vec::with_capacity(2);
        if (0xDC00..=0xDFFF).contains(&unit) {
            if let Some(high) = self.pending_high_surrogate.take() {
                units.push(high);
                units.push(unit);
            } else {
                return;
            }
        } else {
            self.pending_high_surrogate = None;
            units.push(unit);
        }
        self.active_draft_mut().put(&units);
        self.composition.clear();
        self.cursor_epoch = std::time::Instant::now();
    }

    pub fn update_ime(&mut self, composition: Option<String>, result: Option<String>) {
        if let Some(result) = result {
            self.insert_text(&result);
        }
        if let Some(composition) = composition {
            self.composition = composition;
        }
        self.cursor_epoch = std::time::Instant::now();
    }

    pub fn reply_audio(&self, index: usize) -> (bool, Option<&str>) {
        let busy = self.playback.as_ref().is_some_and(|(id, row, _)| id == &self.conversation_id && *row == index);
        let error = self.playback_error.as_ref().filter(|(id, row, _)| id == &self.conversation_id && *row == index).map(|(_, _, error)| error.as_str());
        (busy, error)
    }

    pub fn speak(&mut self, index: usize, text: String, settings: Settings) {
        if self.playback.is_some() { return; }
        self.playback_error = None;
        let worker = std::thread::Builder::new().name("glance: simo voice".into()).spawn(move || {
            let audio = speech(&settings.tts_base_url, &settings.api_key, &settings.tts_model, &settings.tts_voice, &text)?;
            play_wav(&audio)
        });
        match worker {
            Ok(worker) => self.playback = Some((self.conversation_id.clone(), index, worker)),
            Err(error) => self.playback_error = Some((self.conversation_id.clone(), index, error.to_string())),
        }
    }

    pub fn poll(&mut self) {
        self.refresh_selection();
        if self.playback.as_ref().is_some_and(|(_, _, worker)| worker.is_finished()) {
            let (id, index, worker) = self.playback.take().unwrap();
            if let Err(error) = worker.join().unwrap_or_else(|_| Err("SIMO playback thread stopped".into())) {
                self.playback_error = Some((id, index, error));
            }
        }
        if self.request.as_ref().is_some_and(|(_, _, worker)| worker.is_finished()) {
            let (id, _, worker) = self.request.take().unwrap();
            let active = id == self.conversation_id;
            match worker.join().unwrap_or_else(|_| Err("SIMO request thread stopped".into())) {
                Ok((index, reply)) => {
                    if active {
                        self.state = State::Idle;
                        let settings = crate::app().settings.lock().unwrap().simo.clone();
                        if settings.voice_output { self.speak(index, reply, settings); }
                    }
                }
                Err(error) if active => self.state = State::Error(error),
                Err(_) => {}
            }
        }
    }

    pub fn send(&mut self, settings: Settings) {
        self.refresh_selection();
        let text = self.draft.string();
        if text.trim().is_empty() || self.request.is_some() { return; }
        if self.conversation_id.is_empty() {
            if let Some(repository) = &self.repository {
                let conversation = Conversation::new();
                self.conversation_id = conversation.info.id.clone();
                self.messages = conversation.messages.clone();
                repository.lock().unwrap().conversations.push(conversation);
            }
        }
        self.composition.clear();
        self.pending_high_surrogate = None;
        self.draft.text.clear();
        self.draft.go(0, false);
        let attachments = std::mem::take(&mut self.attachments);
        self.messages.lock().unwrap().push(ChatMessage { sent_at: Some(now_ms()), role: Role::User, text, attachments });
        self.scroll = 0.0;
        self.scroll_target = 0.0;
        self.persist();
        self.request_reply(settings);
    }

    fn request_reply(&mut self, settings: Settings) {
        let history = self.messages.lock().unwrap().iter().map(|message| Message { role: message.role, content: message.text.clone(), attachments: message.attachments.clone() }).collect::<Vec<_>>();
        let messages = self.messages.clone();
        let repository = self.repository.clone();
        let id = self.conversation_id.clone();
        let worker = std::thread::Builder::new().name("glance: simo chat".into()).spawn(move || {
            let reply = complete(&settings.api_base_url, &settings.api_key, &settings.model, &history, &[])?;
            let index = {
                let mut messages = messages.lock().unwrap();
                let index = messages.len();
                messages.push(ChatMessage { sent_at: Some(now_ms()), role: Role::Assistant, text: reply.clone(), attachments: Vec::new() });
                index
            };
            if let Some(repository) = repository {
                repository.lock().unwrap().save_all(&id);
            }
            Ok((index, reply))
        });
        match worker {
            Ok(worker) => {
                self.state = State::Waiting;
                self.request = Some((self.conversation_id.clone(), self.messages.clone(), worker));
            }
            Err(error) => self.state = State::Error(error.to_string()),
        }
    }
}

impl Conversation {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_ID: AtomicU64 = AtomicU64::new(0);
        let stamp = now_ms();
        let previous = LAST_ID.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| Some(stamp.max(last + 1))).unwrap();
        let id = format!("{}-{}", stamp.max(previous + 1), std::process::id());
        Conversation { info: ConversationInfo { id, title: "新对话".into(), updated: stamp }, messages: Arc::new(Mutex::new(Vec::new())) }
    }
}

impl Repository {
    fn open(path: PathBuf) -> Self {
        let folder = path.join("conversations");
        let mut conversations = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&folder) {
            for entry in entries.flatten().filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json")) {
                let Ok(bytes) = std::fs::read(entry.path()) else { continue };
                let Ok(stored) = serde_json::from_slice::<StoredConversation>(&bytes) else { continue };
                let attachment_folder = folder.join(&stored.id).join("attachments");
                let messages = stored.messages.into_iter().map(|message| {
                    let attachments = message.attachments.into_iter().filter_map(|attachment| {
                        let bytes = std::fs::read(attachment_folder.join(&attachment.file)).ok()?;
                        let preview = attachment.mime.starts_with("image/").then(|| crate::ui::picture::from_bytes(&bytes, (172, 132)).map(|picture| (std::sync::Arc::new(picture.small), picture.size))).flatten();
                        Some(Attachment { name: attachment.name, mime: attachment.mime, bytes, extracted_text: attachment.extracted_text, stored_file: Some(attachment.file), preview })
                    }).collect();
                    ChatMessage { sent_at: message.sent_at, role: message.role, text: message.text, attachments }
                }).collect();
                conversations.push(Conversation { info: ConversationInfo { id: stored.id, title: stored.title, updated: stored.updated }, messages: Arc::new(Mutex::new(messages)) });
            }
        }
        conversations.sort_by_key(|item| item.info.updated);
        Repository { path, conversations }
    }

    fn delete(&mut self, id: &str) -> std::io::Result<()> {
        let Some(index) = self.conversations.iter().position(|entry| entry.info.id == id) else { return Ok(()) };
        // Both the JSON and the generated attachment folder must be children of conversations.
        let valid = id.split_once('-').is_some_and(|(stamp, pid)| {
            [stamp, pid].into_iter().all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        });
        if !valid {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid SIMO conversation ID"));
        }
        let folder = self.path.join("conversations");
        match std::fs::remove_file(folder.join(format!("{id}.json"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.conversations.remove(index);
        if let Ok(root) = folder.canonicalize() {
            let target = root.join(id);
            if target.canonicalize().is_ok_and(|resolved| resolved == target) {
                if let Err(error) = std::fs::remove_dir_all(&target) {
                    crate::journal::note(format!("SIMO history: could not remove conversation attachments: {error}"));
                }
            }
        }
        Ok(())
    }

    fn save_all(&mut self, active_id: &str) {
        let folder = self.path.join("conversations");
        if let Err(error) = std::fs::create_dir_all(&folder) {
            crate::journal::note(format!("SIMO history: could not create storage directory: {error}"));
            return;
        }
        for conversation in &mut self.conversations {
            let mut messages = conversation.messages.lock().unwrap();
            let mut stored_messages = Vec::with_capacity(messages.len());
            for (message_index, message) in messages.iter_mut().enumerate() {
                let mut stored_attachments = Vec::with_capacity(message.attachments.len());
                for (attachment_index, attachment) in message.attachments.iter_mut().enumerate() {
                    let filename = attachment.stored_file.get_or_insert_with(|| format!("{message_index}-{attachment_index}.bin")).clone();
                    let attachment_folder = folder.join(&conversation.info.id).join("attachments");
                    if let Err(error) = std::fs::create_dir_all(&attachment_folder).and_then(|_| std::fs::write(attachment_folder.join(&filename), &attachment.bytes)) {
                        crate::journal::note(format!("SIMO history: could not save attachment {}: {error}", attachment.name));
                    }
                    stored_attachments.push(StoredAttachment { name: attachment.name.clone(), mime: attachment.mime.clone(), extracted_text: attachment.extracted_text.clone(), file: filename });
                }
                stored_messages.push(StoredMessage { sent_at: message.sent_at, role: message.role, text: message.text.clone(), attachments: stored_attachments });
            }
            if let Some(user_message) = messages.iter().find(|message| matches!(message.role, Role::User)) {
                let title = user_message.text.lines().next().unwrap_or_default().trim();
                if !title.is_empty() {
                    conversation.info.title = title.chars().take(48).collect();
                }
            }
            if conversation.info.id == active_id { conversation.info.updated = now_ms(); }
            let stored = StoredConversation { id: conversation.info.id.clone(), title: conversation.info.title.clone(), updated: conversation.info.updated, messages: stored_messages };
            let destination = folder.join(format!("{}.json", conversation.info.id));
            let temporary = destination.with_extension("json.tmp");
            let result = serde_json::to_vec(&stored).map_err(std::io::Error::other)
                .and_then(|bytes| std::fs::write(&temporary, bytes))
                .and_then(|_| std::fs::rename(&temporary, &destination));
            if let Err(error) = result {
                crate::journal::note(format!("SIMO history: could not save conversation: {error}"));
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn play_wav(bytes: &[u8]) -> Result<(), String> {
    use windows::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT, SND_SYNC};
    if !bytes.starts_with(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("The speech service did not return WAV audio".into());
    }
    // Synchronous on the voice worker: the WAV buffer stays alive for playback.
    let played = unsafe { PlaySoundW(windows::core::PCWSTR(bytes.as_ptr().cast()), None, SND_MEMORY | SND_NODEFAULT | SND_SYNC) };
    if played.as_bool() { Ok(()) } else { Err("Windows could not play the speech audio".into()) }
}

#[derive(Clone, Debug)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
    pub extracted_text: Option<String>,
    stored_file: Option<String>,
    pub preview: Option<(std::sync::Arc<Vec<u8>>, (u32, u32))>,
}

impl Attachment {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("attachment").to_string();
        let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !SUPPORTED_EXTENSIONS.contains(&extension.as_str()) {
            return Err(format!("unsupported attachment type: .{extension}"));
        }
        let bytes = std::fs::read(path).map_err(|error| format!("{name}: {error}"))?;
        let mime = mime_for(&extension).to_string();
        let extracted_text = text_for(&extension, &bytes);
        let preview = mime.starts_with("image/").then(|| crate::ui::picture::from_file(&path.to_string_lossy(), (172, 132)).map(|picture| (std::sync::Arc::new(picture.small), picture.size))).flatten();
        Ok(Self { name, mime, bytes, extracted_text, stored_file: None, preview })
    }

    pub fn prompt_part(&self) -> String {
        if let Some(text) = &self.extracted_text {
            return format!("\n\nAttachment: {}\n{}", self.name, text);
        }
        if self.mime.starts_with("image/") {
            return format!("\n\nImage attachment: {}", self.name);
        }
        format!("\n\nBinary attachment: {} ({} bytes)", self.name, self.bytes.len())
    }

    pub fn data_url(&self) -> Option<String> {
        self.mime.starts_with("image/").then(|| format!("data:{};base64,{}", self.mime, base64(&self.bytes)))
    }
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "gif", "webp", "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "csv", "md", "markdown", "json", "xml", "html", "htm", "rtf",
];

pub fn choose_files(owner: windows::Win32::Foundation::HWND) -> Vec<std::path::PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OPENFILENAMEW};
    let filter = "图片和文档\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp;*.pdf;*.doc;*.docx;*.xls;*.xlsx;*.ppt;*.pptx;*.txt;*.csv;*.md;*.markdown;*.json;*.xml;*.html;*.htm;*.rtf\0所有文件\0*.*\0\0".encode_utf16().collect::<Vec<_>>();
    let mut paths = vec![0u16; 16 << 10];
    let mut asked = OPENFILENAMEW { lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32, hwndOwner: owner, lpstrFilter: windows::core::PCWSTR(filter.as_ptr()), lpstrFile: windows::core::PWSTR(paths.as_mut_ptr()), nMaxFile: paths.len() as u32, Flags: OFN_EXPLORER | OFN_ALLOWMULTISELECT | OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR, ..Default::default() };
    if !unsafe { GetOpenFileNameW(&mut asked) }.as_bool() { return Vec::new(); }
    let named = paths.split(|part| *part == 0).take_while(|part| !part.is_empty()).map(String::from_utf16_lossy).collect::<Vec<_>>();
    match named.split_first() {
        Some((folder, names)) if !names.is_empty() => names.iter().map(|name| std::path::PathBuf::from(format!("{}\\{name}", folder.trim_end_matches('\\')))).collect(),
        _ => named.into_iter().map(std::path::PathBuf::from).collect(),
    }
}

fn mime_for(extension: &str) -> &'static str {
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "rtf" => "application/rtf",
        _ => "text/plain",
    }
}

fn text_for(extension: &str, bytes: &[u8]) -> Option<String> {
    if matches!(extension, "txt" | "csv" | "md" | "markdown" | "json" | "xml" | "html" | "htm" | "rtf") {
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }
    if extension == "pdf" {
        let text = String::from_utf8_lossy(bytes);
        let mut extracted = String::new();
        for page in text.split("BT").skip(1) {
            if let Some(body) = page.split_once("ET").map(|parts| parts.0) {
                extracted.push_str(body);
                extracted.push('\n');
            }
        }
        return (!extracted.trim().is_empty()).then_some(extracted);
    }
    if matches!(extension, "docx" | "xlsx" | "pptx") {
        let reader = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(reader).ok()?;
        let names: Vec<String> = (0..archive.len()).filter_map(|index| archive.by_index(index).ok().map(|entry| entry.name().to_string())).collect();
        let wanted = names.into_iter().filter(|name| match extension {
            "docx" => name == "word/document.xml",
            "xlsx" => name.starts_with("xl/worksheets/") && name.ends_with(".xml"),
            "pptx" => name.starts_with("ppt/slides/slide") && name.ends_with(".xml"),
            _ => false,
        });
        let mut output = String::new();
        for name in wanted {
            let mut file = archive.by_name(&name).ok()?;
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut file, &mut xml).ok()?;
            let mut in_tag = false;
            for character in xml.chars() {
                match character {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => output.push(character),
                    _ => {}
                }
            }
            output.push('\n');
        }
        let output = output.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">");
        return (!output.trim().is_empty()).then_some(output);
    }
    None
}

pub fn fetch_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let body = request(base_url, "/models", "GET", api_key, &[])?;
    let value: serde_json::Value = serde_json::from_slice(&body).map_err(|error| format!("invalid models response: {error}"))?;
    let mut models = value["data"].as_array().into_iter().flatten().filter_map(|item| item["id"].as_str().map(str::to_owned)).collect::<Vec<_>>();
    models.sort();
    models.dedup();
    Ok(models)
}

pub fn complete(base_url: &str, api_key: &str, model: &str, history: &[Message], attachments: &[Attachment]) -> Result<String, String> {
    if model.trim().is_empty() {
        return Err("no SIMO model selected".into());
    }
    let messages = request_messages(history, attachments);
    let payload = serde_json::json!({ "model": model, "messages": messages });
    let body = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    let response = request(base_url, "/chat/completions", "POST", api_key, &body)?;
    let value: serde_json::Value = serde_json::from_slice(&response).map_err(|error| format!("invalid chat response: {error}"))?;
    value["choices"][0]["message"]["content"].as_str().map(str::to_owned).ok_or_else(|| value["error"]["message"].as_str().unwrap_or("chat response did not contain message content").to_string())
}

fn request_messages(history: &[Message], attachments: &[Attachment]) -> Vec<serde_json::Value> {
    let mut messages = history.iter().map(|message| {
        let mut content = message.content.clone();
        for attachment in &message.attachments { content.push_str(&attachment.prompt_part()); }
        let images = message.attachments.iter().filter_map(Attachment::data_url).collect::<Vec<_>>();
        let content = if images.is_empty() {
            serde_json::Value::String(content)
        } else {
            let mut parts = vec![serde_json::json!({ "type": "text", "text": content })];
            parts.extend(images.into_iter().map(|url| serde_json::json!({ "type": "image_url", "image_url": { "url": url } })));
            serde_json::Value::Array(parts)
        };
        serde_json::json!({ "role": message.role, "content": content })
    }).collect::<Vec<_>>();
    if !attachments.is_empty() {
        if let Some(last) = messages.iter_mut().rev().find(|message| message["role"] == "user") {
            let mut content = last["content"].as_str().unwrap_or_default().to_string();
            for attachment in attachments { content.push_str(&attachment.prompt_part()); }
            let images = attachments.iter().filter_map(Attachment::data_url).collect::<Vec<_>>();
            if images.is_empty() {
                last["content"] = serde_json::Value::String(content);
            } else {
                let mut parts = vec![serde_json::json!({ "type": "text", "text": content })];
                parts.extend(images.into_iter().map(|url| serde_json::json!({ "type": "image_url", "image_url": { "url": url } })));
                last["content"] = serde_json::Value::Array(parts);
            }
        }
    }
    messages
}

pub fn speech(base_url: &str, api_key: &str, model: &str, voice: &str, text: &str) -> Result<Vec<u8>, String> {
    let payload = serde_json::json!({ "model": model, "voice": voice, "input": text, "response_format": "wav" });
    let body = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    request(base_url, "/audio/speech", "POST", api_key, &body)
}

fn request(base_url: &str, endpoint: &str, method: &str, api_key: &str, body: &[u8]) -> Result<Vec<u8>, String> {
    let url = format!("{}/{}", base_url.trim_end_matches('/'), endpoint.trim_start_matches('/'));
    let (host, port, path, secure) = parse_url(&url)?;
    let agent = windows::core::w!("Glance SIMO");
    let session = unsafe { windows::Win32::Networking::WinHttp::WinHttpOpen(agent, windows::Win32::Networking::WinHttp::WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, windows::core::PCWSTR::null(), windows::core::PCWSTR::null(), 0) };
    if session.is_null() {
        return Err("WinHTTP session could not be created".into());
    }
    let session_guard = Handle(session);
    let host = windows::core::HSTRING::from(host);
    let connection = unsafe { windows::Win32::Networking::WinHttp::WinHttpConnect(session_guard.0, &host, port, 0) };
    if connection.is_null() {
        return Err("SIMO provider host could not be reached".into());
    }
    let connection_guard = Handle(connection);
    let verb = windows::core::HSTRING::from(method);
    let path = windows::core::HSTRING::from(path);
    let flags = if secure { windows::Win32::Networking::WinHttp::WINHTTP_FLAG_SECURE } else { windows::Win32::Networking::WinHttp::WINHTTP_OPEN_REQUEST_FLAGS(0) };
    let request = unsafe { windows::Win32::Networking::WinHttp::WinHttpOpenRequest(connection_guard.0, &verb, &path, windows::core::PCWSTR::null(), windows::core::PCWSTR::null(), std::ptr::null(), flags) };
    if request.is_null() {
        return Err("SIMO HTTP request could not be created".into());
    }
    let request_guard = Handle(request);
    let headers = format!("Accept: application/json\r\nContent-Type: application/json\r\nAuthorization: Bearer {api_key}\r\n").encode_utf16().collect::<Vec<_>>();
    let optional = (!body.is_empty()).then_some(body.as_ptr() as *const c_void);
    unsafe { windows::Win32::Networking::WinHttp::WinHttpSendRequest(request_guard.0, Some(&headers), optional, body.len() as u32, body.len() as u32, 0) }.map_err(|error| format!("SIMO request failed: {error}"))?;
    unsafe { windows::Win32::Networking::WinHttp::WinHttpReceiveResponse(request_guard.0, std::ptr::null_mut()) }.map_err(|error| format!("SIMO response failed: {error}"))?;
    let mut output = Vec::new();
    loop {
        let mut available = 0;
        unsafe { windows::Win32::Networking::WinHttp::WinHttpQueryDataAvailable(request_guard.0, &mut available) }.map_err(|error| format!("SIMO response read failed: {error}"))?;
        if available == 0 {
            break;
        }
        let start = output.len();
        output.resize(start + available as usize, 0);
        let mut read = 0;
        unsafe { windows::Win32::Networking::WinHttp::WinHttpReadData(request_guard.0, output[start..].as_mut_ptr() as *mut c_void, available, &mut read) }.map_err(|error| format!("SIMO response read failed: {error}"))?;
        output.truncate(start + read as usize);
        if read == 0 {
            break;
        }
    }
    Ok(output)
}

fn parse_url(url: &str) -> Result<(String, u16, String, bool), String> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") { (true, rest) } else if let Some(rest) = url.strip_prefix("http://") { (false, rest) } else { return Err("SIMO URL must start with http:// or https://".into()) };
    let (authority, path) = rest.split_once('/').map_or((rest, "/"), |(authority, path)| (authority, path));
    let (host, port) = match authority.rsplit_once(':').filter(|(_, port)| port.chars().all(|char| char.is_ascii_digit())) {
        Some((host, port)) => (host, port.parse().map_err(|_| "invalid SIMO port")?),
        None => (authority, if secure { 443 } else { 80 }),
    };
    if host.is_empty() { return Err("SIMO URL has no host".into()); }
    Ok((host.to_string(), port, if path == "/" { "/".into() } else { format!("/{path}") }, secure))
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            let _ = unsafe { windows::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0) };
        }
    }
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        result.push(TABLE[((a >> 2) & 0x3F) as usize] as char);
        result.push(TABLE[(((a << 4) | (b >> 4)) & 0x3F) as usize] as char);
        result.push(if chunk.len() > 1 { TABLE[(((b << 2) | (c >> 6)) & 0x3F) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { TABLE[(c & 0x3F) as usize] as char } else { '=' });
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn simo_draft_shows_ime_preedit_and_blinks_from_focus_epoch() {
        let mut chat = super::Chat::default();
        chat.focus();
        chat.insert_text("need");
        chat.update_ime(Some("ing".into()), None);
        assert_eq!(chat.displayed_text(), "needing");
        assert!(chat.caret_visible(chat.cursor_epoch));
        assert!(!chat.caret_visible(chat.cursor_epoch + std::time::Duration::from_millis(600)));
    }

    #[test]
    fn simo_accepts_surrogate_pairs_as_one_utf16_character() {
        let mut chat = super::Chat::default();
        chat.type_utf16(0xD83D);
        assert!(chat.draft.text.is_empty());
        chat.type_utf16(0xDE80);
        assert_eq!(chat.draft.string(), "🚀");
    }

    #[test]
    fn pasted_text_replaces_the_selected_draft_range() {
        let mut chat = super::Chat::default();
        chat.insert_text("before after");
        chat.draft.go(7, false);
        chat.draft.go(12, true);
        chat.insert_text("SIMO");
        assert_eq!(chat.draft.string(), "before SIMO");
    }

    #[test]
    fn encodes_base64() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }

    #[test]
    fn parses_provider_urls() {
        assert_eq!(super::parse_url("https://example.test/v1/models").unwrap(), ("example.test".into(), 443, "/v1/models".into(), true));
        assert_eq!(super::parse_url("http://localhost:8080").unwrap(), ("localhost".into(), 8080, "/".into(), false));
    }

    #[test]
    fn storage_directory_defaults_locally_and_accepts_a_custom_path() {
        let mut settings = super::Settings::default();
        assert_eq!(super::storage_dir(&settings), crate::settings::local_dir().join("simo"));
        let custom = std::env::temp_dir().join("glance-simo-custom-storage");
        settings.storage_dir = Some(custom.clone());
        assert_eq!(super::storage_dir(&settings), custom);
        let saved = serde_json::to_vec(&settings).unwrap();
        let loaded: super::Settings = serde_json::from_slice(&saved).unwrap();
        assert_eq!(loaded.storage_dir, settings.storage_dir);
    }

    #[test]
    fn history_keeps_messages_and_original_attachment_bytes_on_disk() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("glance-simo-history-{}-{stamp}", std::process::id()));
        let mut repository = super::Repository::open(root.clone());
        let conversation = super::Conversation::new();
        let id = conversation.info.id.clone();
        conversation.messages.lock().unwrap().push(super::ChatMessage {
            sent_at: Some(1_700_000_000_000),
            role: super::Role::User,
            text: "Explain this document".into(),
            attachments: vec![super::Attachment { name: "note.txt".into(), mime: "text/plain".into(), bytes: b"persistent text".to_vec(), extracted_text: Some("persistent text".into()), stored_file: None, preview: None }],
        });
        repository.conversations.push(conversation);
        repository.save_all(&id);

        let loaded = super::Repository::open(root.clone());
        let message = loaded.conversations[0].messages.lock().unwrap();
        assert_eq!(message[0].text, "Explain this document");
        assert_eq!(message[0].sent_at, Some(1_700_000_000_000));
        let legacy: super::StoredMessage = serde_json::from_str(r#"{"role":"assistant","text":"Old reply","attachments":[]}"#).unwrap();
        assert_eq!(legacy.sent_at, None);
        assert_eq!(message[0].attachments[0].bytes, b"persistent text");
        assert_eq!(loaded.conversations[0].info.title, "Explain this document");
        drop(message);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn deleting_history_updates_other_views_and_removes_the_saved_conversation() {
        let root = std::env::temp_dir().join(format!("glance-simo-delete-{}-{}", std::process::id(), super::now_ms()));
        let settings = super::Settings { storage_dir: Some(root.clone()), ..Default::default() };
        let mut chat = super::Chat::open(&settings);
        let original = chat.conversation_id.clone();
        chat.new_conversation();
        let removed = chat.conversation_id.clone();
        assert_ne!(original, removed);
        let mut other_view = super::Chat::open(&settings);
        assert!(other_view.select_conversation(&removed));
        let folder = root.join("conversations");
        let attachments = folder.join(&removed).join("attachments");
        std::fs::create_dir_all(&attachments).unwrap();
        std::fs::write(attachments.join("0-0.bin"), b"local attachment").unwrap();

        chat.delete_conversation(&removed).unwrap();
        other_view.poll();
        assert_eq!(chat.conversation_id, original);
        assert_eq!(other_view.conversation_id, original);
        assert!(!folder.join(&removed).exists());
        let reloaded = super::Repository::open(root.clone());
        assert_eq!(reloaded.conversations.len(), 1);
        assert_eq!(reloaded.conversations[0].info.id, original);

        chat.delete_conversation(&original).unwrap();
        assert!(chat.conversation_id.is_empty());
        assert!(chat.conversations().is_empty());
        assert!(super::Repository::open(root.clone()).conversations.is_empty());
        let _ = std::fs::remove_dir(folder);
        let _ = std::fs::remove_dir(root);
    }

    #[test]
    fn moving_storage_copies_existing_files_without_overwriting_target_files() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("glance-simo-move-{}-{stamp}", std::process::id()));
        let from = root.join("old");
        let to = root.join("new");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("history.json"), "old history").unwrap();
        std::fs::write(to.join("history.json"), "target history").unwrap();
        std::fs::write(from.join("image.png"), "original image").unwrap();
        super::copy_tree(&from, &to).unwrap();
        assert_eq!(std::fs::read_to_string(to.join("history.json")).unwrap(), "target history");
        assert_eq!(std::fs::read(to.join("image.png")).unwrap(), b"original image");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn continued_requests_include_images_from_earlier_user_messages() {
        let image = super::Attachment { name: "photo.png".into(), mime: "image/png".into(), bytes: vec![1, 2, 3], extracted_text: None, stored_file: None, preview: None };
        let history = [super::Message { role: super::Role::User, content: "What is in this picture?".into(), attachments: vec![image] }, super::Message { role: super::Role::Assistant, content: "A test image.".into(), attachments: Vec::new() }, super::Message { role: super::Role::User, content: "Describe its details.".into(), attachments: Vec::new() }];
        let messages = super::request_messages(&history, &[]);
        assert_eq!(messages.len(), 3);
        assert!(messages[0]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(messages[2]["content"], "Describe its details.");
    }

    #[test]
    fn extracts_supported_text_attachments() {
        let dir = std::env::temp_dir();
        let path = dir.join("glance-simo-test.md");
        std::fs::write(&path, "hello SIMO").unwrap();
        let attachment = super::Attachment::from_path(&path).unwrap();
        assert_eq!(attachment.extracted_text.as_deref(), Some("hello SIMO"));
        assert!(attachment.prompt_part().contains("glance-simo-test.md"));
        let _ = std::fs::remove_file(path);
    }
}
