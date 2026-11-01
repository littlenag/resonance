//! The TUI's state and key handling, without a terminal (MVP-041). Calls go through a
//! [`Client`]; editors are [`Effect`]s the run loop carries out, so tests drive everything with
//! key events.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use resonance_core::changes::Entry;
use resonance_core::error::Result;
use resonance_core::model::NodeType;
use resonance_core::query::SpecItemView;
use resonance_core::service::{Backend, Call};
use serde_json::{Value, json};

/// How the TUI reaches the graph: the daemon, or this process.
pub trait Client {
    fn call(&mut self, method: &str, args: Value) -> Result<Value>;
}

/// A [`Backend`] as a client; the TUI's changes carry `origin = tui` (MVP-040).
pub struct BackendClient(pub Box<dyn Backend>);

impl Client for BackendClient {
    fn call(&mut self, method: &str, args: Value) -> Result<Value> {
        let mut c = Call::new(method, args);
        c.origin = Some("tui".into());
        self.0.call(c)
    }
}

/// Something the run loop does outside the TUI, then reports back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Edit a long field in `$VISUAL`/`$EDITOR`; the result goes to [`App::finish_text`].
    EditText {
        field: String,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    List,
    Detail,
    Form,
    Confirm,
    Log,
    Help,
}

/// A field in the edit or add form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub value: String,
    /// Edited in `$EDITOR` rather than inline.
    pub long: bool,
    /// The value when the form opened, to send only what changed.
    pub original: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// The item being edited, or `None` when adding.
    pub editing: Option<String>,
    pub label: String,
    pub ntype: NodeType,
    /// Adding only: the key, prefilled with a suggestion (FR-273).
    pub key: String,
    pub fields: Vec<Field>,
    /// The focused row: when adding, 0 is the type and 1 the key; then the fields.
    pub cursor: usize,
}

impl Form {
    fn header_rows(&self) -> usize {
        if self.editing.is_some() { 0 } else { 2 }
    }

    pub fn rows(&self) -> usize {
        self.header_rows() + self.fields.len()
    }

    /// The focused field, if the cursor is on one.
    pub fn field_mut(&mut self) -> Option<&mut Field> {
        let i = self.cursor.checked_sub(self.header_rows())?;
        self.fields.get_mut(i)
    }

    pub fn focus(&self) -> FormRow {
        match (self.editing.is_some(), self.cursor) {
            (false, 0) => FormRow::Type,
            (false, 1) => FormRow::Key,
            _ => FormRow::Field(self.cursor - self.header_rows()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormRow {
    Type,
    Key,
    Field(usize),
}

/// What needs confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Confirm {
    /// Retiring an item, with its link count (FR-271).
    Retire {
        id: String,
        label: String,
        links: usize,
    },
    /// Clearing the whole change log (FR-276).
    ClearLog { count: usize },
}

/// A one-line text input (the `/` filter).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    pub buffer: String,
}

const STATUSES: &[&str] = &[
    "active",
    "draft",
    "proposed",
    "accepted",
    "superseded",
    "deprecated",
    "retired",
];

/// Fields per type, and which are long (MVP-041).
pub fn fields_of(t: NodeType) -> &'static [(&'static str, bool)] {
    match t {
        NodeType::Requirement => &[("text", true), ("kind", false), ("status", false)],
        NodeType::Decision => &[
            ("title", false),
            ("decision", true),
            ("context", true),
            ("consequences", true),
            ("status", false),
        ],
        NodeType::Design => &[("title", false), ("text", true), ("status", false)],
        NodeType::Constraint => &[
            ("title", false),
            ("text", true),
            ("scope", false),
            ("status", false),
        ],
        _ => &[],
    }
}

pub struct App {
    client: Box<dyn Client>,
    pub screen: Screen,
    /// Where `esc` from help, the log or a form goes back to.
    back: Screen,
    pub items: Vec<SpecItemView>,
    pub table: TableState,
    pub filter_text: String,
    pub filter_type: Option<NodeType>,
    pub filter_status: Option<String>,
    /// The `/` filter being typed.
    pub input: Option<Input>,
    /// `show` for the selected item.
    pub detail: Option<Value>,
    pub form: Option<Form>,
    pub confirm: Option<Confirm>,
    pub log: Vec<Entry>,
    /// First visible change-log row. `j`/`k` move it; the log is read-only.
    pub log_offset: usize,
    /// The selected change-log entry.
    pub log_cursor: usize,
    /// Lines scrolled off the top of the detail screen.
    pub detail_scroll: u16,
    /// Item whose detail is open, so a reload of the same item keeps the scroll position.
    detail_for: Option<String>,
    /// The last message: (text, is an error).
    pub message: Option<(String, bool)>,
    pub quit: bool,
}

impl App {
    pub fn new(mut client: Box<dyn Client>) -> Result<App> {
        // Fails early, with a clear message, when Resonance is not set up.
        client.call("status", json!({}))?;
        let mut app = App {
            client,
            screen: Screen::List,
            back: Screen::List,
            items: Vec::new(),
            table: TableState::default(),
            filter_text: String::new(),
            filter_type: None,
            filter_status: None,
            input: None,
            detail: None,
            form: None,
            confirm: None,
            log: Vec::new(),
            log_offset: 0,
            log_cursor: 0,
            detail_scroll: 0,
            detail_for: None,
            message: None,
            quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    #[cfg(test)]
    pub fn client_mut(&mut self) -> &mut dyn Client {
        &mut *self.client
    }

    /// Reload the list (and the open screen's data), keeping the selection where possible.
    pub fn reload(&mut self) -> Result<()> {
        let keep = self.selected().map(|i| i.id.clone());
        let v = self.client.call(
            "spec_list",
            json!({
                "type": self.filter_type.map(|t| t.to_string()),
                "status": self.filter_status,
                "text": (!self.filter_text.is_empty()).then_some(&self.filter_text),
            }),
        )?;
        self.items = serde_json::from_value(v)?;
        let idx = keep
            .and_then(|id| self.items.iter().position(|i| i.id == id))
            .unwrap_or(0);
        self.table.select(
            (!self.items.is_empty()).then_some(idx.min(self.items.len().saturating_sub(1))),
        );
        if self.screen == Screen::Detail {
            self.load_detail()?;
        }
        if self.screen == Screen::Log {
            self.load_log()?;
        }
        Ok(())
    }

    /// Reload, keeping errors as a message.
    pub fn refresh(&mut self) {
        if let Err(e) = self.reload() {
            self.error(e.to_string());
        }
    }

    pub fn selected(&self) -> Option<&SpecItemView> {
        self.table.selected().and_then(|i| self.items.get(i))
    }

    fn select(&mut self, id: &str) {
        if let Some(i) = self.items.iter().position(|x| x.id == id) {
            self.table.select(Some(i));
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    fn load_detail(&mut self) -> Result<()> {
        let id = self.selected().map(|i| i.id.clone());
        // A different item starts at the top. Reloading the same item keeps the place.
        if id != self.detail_for {
            self.detail_scroll = 0;
            self.detail_for = id.clone();
        }
        self.detail = match id {
            Some(id) => {
                let mut d = self.client.call("show", json!({"ref": id}))?;
                // Where the item came from, if an agent imported it (FR-124).
                if let Some(src) = self.selected().and_then(|i| i.source.clone()) {
                    d["source"] = src;
                }
                if let Some(p) = self.selected().and_then(|i| i.pending_change.clone()) {
                    d["pendingChange"] = p;
                }
                Some(d)
            }
            None => None,
        };
        Ok(())
    }

    fn load_log(&mut self) -> Result<()> {
        self.log = serde_json::from_value(self.client.call("changes", json!({}))?)?;
        let max = self.log.len().saturating_sub(1);
        self.log_offset = self.log_offset.min(max);
        self.log_cursor = self.log_cursor.min(max);
        Ok(())
    }

    fn scroll_detail(&mut self, delta: i32) {
        let next = i32::from(self.detail_scroll).saturating_add(delta).max(0);
        self.detail_scroll = u16::try_from(next).unwrap_or(u16::MAX);
    }

    fn scroll_log(&mut self, delta: isize) {
        if self.log.is_empty() {
            self.log_cursor = 0;
            return;
        }
        let max = self.log.len() as isize - 1;
        self.log_cursor = (self.log_cursor as isize)
            .saturating_add(delta)
            .clamp(0, max) as usize;
    }

    /// Clear change-log entries as handled (FR-276): the person decides nothing more is needed.
    fn clear_log_entries(&mut self, seqs: Vec<u64>) -> Result<()> {
        let n = seqs.len();
        self.client.call(
            "changes_done",
            json!({"seqs": seqs, "note": "cleared in the TUI"}),
        )?;
        self.load_log()?;
        self.reload()?;
        self.info(format!(
            "Cleared {n} entr{} from the change log; your agent will not see {}.",
            if n == 1 { "y" } else { "ies" },
            if n == 1 { "it" } else { "them" }
        ));
        Ok(())
    }

    /// Handle one key; returns what the run loop must do next.
    pub fn on_key(&mut self, key: KeyEvent) -> Effect {
        match self.handle(key) {
            Ok(e) => e,
            Err(e) => {
                self.error(e.to_string());
                Effect::None
            }
        }
    }

    fn handle(&mut self, key: KeyEvent) -> Result<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return Ok(Effect::None);
        }
        if self.input.is_some() {
            return self.on_filter_key(key);
        }
        match self.screen {
            Screen::List => self.on_list_key(key),
            Screen::Detail => self.on_detail_key(key),
            Screen::Form => self.on_form_key(key),
            Screen::Confirm => self.on_confirm_key(key),
            Screen::Log => self.on_log_key(key),
            Screen::Help => {
                self.screen = self.back;
                Ok(Effect::None)
            }
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) -> Result<Effect> {
        let input = self.input.as_mut().expect("an input");
        match key.code {
            KeyCode::Esc => self.input = None,
            KeyCode::Enter => {
                self.filter_text = input.buffer.trim().to_string();
                self.input = None;
                self.reload()?;
            }
            KeyCode::Backspace => {
                input.buffer.pop();
            }
            KeyCode::Char(c) => input.buffer.push(c),
            _ => {}
        }
        Ok(Effect::None)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.items.is_empty() {
            return;
        }
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, self.items.len() as isize - 1);
        self.table.select(Some(next as usize));
    }

    /// Keys shared by the list and the detail screen.
    fn common_key(&mut self, key: KeyEvent) -> Result<Option<Effect>> {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => {
                self.back = self.screen;
                self.screen = Screen::Help;
            }
            KeyCode::Char('l') => {
                self.log_offset = 0;
                self.log_cursor = 0;
                self.load_log()?;
                self.back = self.screen;
                self.screen = Screen::Log;
            }
            KeyCode::Char('a') => self.open_add(None)?,
            KeyCode::Char('e') => return self.open_edit().map(Some),
            KeyCode::Char('r') => return self.open_retire().map(Some),
            KeyCode::Char('g') => {
                self.reload()?;
                self.info("Reloaded.");
            }
            _ => return Ok(None),
        }
        Ok(Some(Effect::None))
    }

    fn on_list_key(&mut self, key: KeyEvent) -> Result<Effect> {
        if let Some(e) = self.common_key(key)? {
            return Ok(e);
        }
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Enter => {
                if self.selected().is_some() {
                    self.load_detail()?;
                    self.screen = Screen::Detail;
                }
            }
            KeyCode::Char('/') => {
                self.input = Some(Input {
                    buffer: self.filter_text.clone(),
                })
            }
            KeyCode::Char('t') => {
                let types = NodeType::SPEC_ITEMS;
                self.filter_type = match self.filter_type {
                    None => Some(types[0]),
                    Some(t) => types
                        .iter()
                        .position(|x| *x == t)
                        .and_then(|i| types.get(i + 1).copied()),
                };
                self.reload()?;
            }
            KeyCode::Char('s') => {
                self.filter_status = match &self.filter_status {
                    None => Some(STATUSES[0].to_string()),
                    Some(s) => STATUSES
                        .iter()
                        .position(|x| x == s)
                        .and_then(|i| STATUSES.get(i + 1))
                        .map(|s| s.to_string()),
                };
                self.reload()?;
            }
            KeyCode::Esc => {
                self.filter_text.clear();
                self.filter_type = None;
                self.filter_status = None;
                self.reload()?;
            }
            _ => {}
        }
        Ok(Effect::None)
    }

    fn on_detail_key(&mut self, key: KeyEvent) -> Result<Effect> {
        if let Some(e) = self.common_key(key)? {
            return Ok(e);
        }
        match key.code {
            KeyCode::Esc | KeyCode::Backspace => self.screen = Screen::List,
            // Scroll the open item. `n`/`p` move between items, which `j`/`k` used to do.
            KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
            KeyCode::PageDown => self.scroll_detail(10),
            KeyCode::PageUp => self.scroll_detail(-10),
            KeyCode::Char('n') => {
                self.move_selection(1);
                self.load_detail()?;
            }
            KeyCode::Char('p') => {
                self.move_selection(-1);
                self.load_detail()?;
            }
            _ => {}
        }
        Ok(Effect::None)
    }

    fn on_log_key(&mut self, key: KeyEvent) -> Result<Effect> {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => {
                self.back = Screen::Log;
                self.screen = Screen::Help;
            }
            KeyCode::Esc | KeyCode::Backspace => {
                self.screen = self.back;
                // Clearing the log reloads the list, not the detail that was open underneath.
                if self.screen == Screen::Detail {
                    self.load_detail()?;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll_log(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_log(-1),
            KeyCode::PageDown => self.scroll_log(10),
            KeyCode::PageUp => self.scroll_log(-10),
            KeyCode::Char('d') if self.log_cursor < self.log.len() => {
                let seq = self.log[self.log_cursor].seq;
                self.clear_log_entries(vec![seq])?;
            }
            KeyCode::Char('D') if !self.log.is_empty() => {
                self.confirm = Some(Confirm::ClearLog {
                    count: self.log.len(),
                });
                self.screen = Screen::Confirm;
            }
            _ => {}
        }
        Ok(Effect::None)
    }

    fn open_edit(&mut self) -> Result<Effect> {
        let Some(item) = self.selected().cloned() else {
            return Ok(Effect::None);
        };
        let fields = fields_of(item.ntype)
            .iter()
            .map(|(name, long)| {
                let value = match item.props.get(*name) {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Null) | None => String::new(),
                    Some(v) => v.to_string(),
                };
                Field {
                    name: name.to_string(),
                    original: value.clone(),
                    value,
                    long: *long,
                }
            })
            .collect();
        self.form = Some(Form {
            editing: Some(item.id.clone()),
            label: item.key.clone().unwrap_or(item.title.clone()),
            ntype: item.ntype,
            key: String::new(),
            fields,
            cursor: 0,
        });
        self.back = self.screen;
        self.screen = Screen::Form;
        Ok(Effect::None)
    }

    /// Open the add form for `ntype` (default: the type filter, or requirements).
    fn open_add(&mut self, ntype: Option<NodeType>) -> Result<()> {
        let ntype = ntype.or(self.filter_type).unwrap_or(NodeType::Requirement);
        let key = self
            .client
            .call("spec_suggest_key", json!({"type": ntype.to_string()}))?;
        let fields = fields_of(ntype)
            .iter()
            .map(|(name, long)| Field {
                name: name.to_string(),
                value: String::new(),
                original: String::new(),
                long: *long,
            })
            .collect();
        if self.screen != Screen::Form {
            self.back = self.screen;
        }
        self.form = Some(Form {
            editing: None,
            label: format!("new {ntype}"),
            ntype,
            key: key["key"].as_str().unwrap_or_default().to_string(),
            fields,
            cursor: 1,
        });
        self.screen = Screen::Form;
        Ok(())
    }

    fn open_retire(&mut self) -> Result<Effect> {
        let Some(item) = self.selected().cloned() else {
            return Ok(Effect::None);
        };
        if item.status == "retired" {
            self.info("Already retired.");
            return Ok(Effect::None);
        }
        self.confirm = Some(Confirm::Retire {
            id: item.id.clone(),
            label: item.key.clone().unwrap_or(item.title.clone()),
            links: item.links,
        });
        self.back = self.screen;
        self.screen = Screen::Confirm;
        Ok(Effect::None)
    }

    fn on_confirm_key(&mut self, key: KeyEvent) -> Result<Effect> {
        let yes = matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y'));
        match self.confirm.take().expect("a confirmation") {
            Confirm::Retire { id, label, links } => {
                self.screen = self.back;
                if yes {
                    self.client.call("spec_retire", json!({"ref": id}))?;
                    self.reload()?;
                    self.info(format!("Retired {label} (its {links} link(s) were kept)."));
                } else {
                    self.info("Not retired.");
                }
            }
            Confirm::ClearLog { .. } => {
                self.screen = Screen::Log;
                if yes {
                    let seqs = self.log.iter().map(|e| e.seq).collect();
                    self.clear_log_entries(seqs)?;
                } else {
                    self.info("The change log is unchanged.");
                }
            }
        }
        Ok(Effect::None)
    }

    fn on_form_key(&mut self, key: KeyEvent) -> Result<Effect> {
        let save = key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s');
        if save || key.code == KeyCode::F(2) {
            return self.save_form().map(|_| Effect::None);
        }
        let form = self.form.as_mut().expect("a form");
        let rows = form.rows();
        match key.code {
            KeyCode::Esc => {
                self.form = None;
                self.screen = self.back;
                self.info("Cancelled.");
            }
            KeyCode::Down | KeyCode::Tab => form.cursor = (form.cursor + 1) % rows,
            KeyCode::Up | KeyCode::BackTab => form.cursor = (form.cursor + rows - 1) % rows,
            _ => match form.focus() {
                FormRow::Type => {
                    if matches!(
                        key.code,
                        KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                    ) {
                        let types = NodeType::SPEC_ITEMS;
                        let i = types.iter().position(|t| *t == form.ntype).unwrap_or(0);
                        let next = if key.code == KeyCode::Left {
                            types[(i + types.len() - 1) % types.len()]
                        } else {
                            types[(i + 1) % types.len()]
                        };
                        self.open_add(Some(next))?;
                        if let Some(f) = self.form.as_mut() {
                            f.cursor = 0;
                        }
                    }
                }
                FormRow::Key => match key.code {
                    KeyCode::Backspace => {
                        form.key.pop();
                    }
                    KeyCode::Char(c) if !c.is_whitespace() => form.key.push(c),
                    KeyCode::Enter => form.cursor = (form.cursor + 1) % rows,
                    _ => {}
                },
                FormRow::Field(_) => {
                    let field = form.field_mut().expect("a field");
                    if field.long {
                        if key.code == KeyCode::Enter {
                            return Ok(Effect::EditText {
                                field: field.name.clone(),
                                text: field.value.clone(),
                            });
                        }
                    } else {
                        match key.code {
                            KeyCode::Backspace => {
                                field.value.pop();
                            }
                            KeyCode::Char(c) => field.value.push(c),
                            KeyCode::Enter => form.cursor = (form.cursor + 1) % rows,
                            _ => {}
                        }
                    }
                }
            },
        }
        Ok(Effect::None)
    }

    /// The result of an [`Effect::EditText`].
    pub fn finish_text(&mut self, field: &str, text: &str) {
        if let Some(f) = self
            .form
            .as_mut()
            .and_then(|f| f.fields.iter_mut().find(|x| x.name == field))
        {
            f.value = text.trim_end_matches('\n').to_string();
        }
    }

    fn save_form(&mut self) -> Result<()> {
        let form = self.form.clone().expect("a form");
        let changed: serde_json::Map<String, Value> = form
            .fields
            .iter()
            .filter(|f| match &form.editing {
                Some(_) => f.value != f.original,
                None => !f.value.trim().is_empty(),
            })
            .map(|f| (f.name.clone(), json!(f.value)))
            .collect();
        let view = match &form.editing {
            Some(id) => {
                if changed.is_empty() {
                    self.form = None;
                    self.screen = self.back;
                    self.info("Nothing changed.");
                    return Ok(());
                }
                self.client
                    .call("spec_edit", json!({"ref": id, "fields": changed}))?
            }
            None => {
                let key = form.key.trim();
                self.client.call(
                    "spec_add",
                    json!({
                        "type": form.ntype.to_string(),
                        "key": (!key.is_empty()).then_some(key),
                        "fields": changed,
                    }),
                )?
            }
        };
        let id = view["id"].as_str().unwrap_or_default().to_string();
        self.form = None;
        self.screen = self.back;
        self.reload()?;
        self.select(&id);
        if self.screen == Screen::Detail {
            self.load_detail()?;
        }
        self.info(match &form.editing {
            Some(_) => format!("Saved {}.", form.label),
            None => format!(
                "Added {}.",
                view["key"]
                    .as_str()
                    .unwrap_or(view["title"].as_str().unwrap_or("the item"))
            ),
        });
        Ok(())
    }
}
