use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub struct SelectOption<T> {
    /// Display label.
    pub label: String,
    /// Underlying value returned on submit.
    pub value: T,
    /// Optional hint shown next to the label.
    pub hint: Option<String>,
}

/// Result of processing a single key event in [`Select`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectAction<T> {
    /// No transition.
    Continue,
    /// User pressed Enter; carries the chosen value.
    Submit(T),
    /// User pressed Esc — semantically "go back to the previous step".
    Cancel,
    /// User pressed Ctrl+C — semantically "exit the wizard".
    Quit,
}

/// Vertical list selector widget.
pub struct Select<T> {
    options: Vec<SelectOption<T>>,
    cursor: usize,
    list_title: String,
}

impl<T: Clone> Select<T> {
    /// Create a new select with no preselected value (cursor starts at 0).
    pub fn new(options: Vec<SelectOption<T>>) -> Self {
        Self {
            options,
            cursor: 0,
            list_title: "Options".to_string(),
        }
    }

    /// Create a new select preselecting `initial` (falling back to index 0
    /// when no option's value equals `initial`).
    pub fn with_initial(options: Vec<SelectOption<T>>, initial: &T) -> Self
    where
        T: PartialEq,
    {
        let cursor = options
            .iter()
            .position(|o| &o.value == initial)
            .unwrap_or(0);
        Self {
            options,
            cursor,
            list_title: "Options".to_string(),
        }
    }

    /// Override the title shown on the list block (defaults to "Options").
    /// Builder-style so it can chain off `Select::new` / `with_initial`.
    pub fn with_list_title(mut self, list_title: impl Into<String>) -> Self {
        self.list_title = list_title.into();
        self
    }

    /// Read-only access to the list block title (for renderers).
    pub fn list_title(&self) -> &str {
        &self.list_title
    }

    /// Index of the currently highlighted option.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Read-only access to all options (for renderers).
    pub fn options(&self) -> &[SelectOption<T>] {
        &self.options
    }

    /// Currently highlighted value (None if there are no options).
    pub fn selected_value(&self) -> Option<&T> {
        self.options.get(self.cursor).map(|o| &o.value)
    }

    /// Process a key event and return the resulting action.
    pub fn handle_key(&mut self, event: KeyEvent) -> SelectAction<T> {
        if event.kind == KeyEventKind::Release {
            return SelectAction::Continue;
        }
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return SelectAction::Quit;
        }
        let len = self.options.len();
        if len == 0 {
            return SelectAction::Continue;
        }
        match event.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.cursor = (self.cursor + 1) % len;
                SelectAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.cursor = if self.cursor == 0 {
                    len - 1
                } else {
                    self.cursor - 1
                };
                SelectAction::Continue
            }
            KeyCode::Home => {
                self.cursor = 0;
                SelectAction::Continue
            }
            KeyCode::End => {
                self.cursor = len - 1;
                SelectAction::Continue
            }
            KeyCode::Enter => SelectAction::Submit(self.options[self.cursor].value.clone()),
            KeyCode::Esc => SelectAction::Cancel,
            _ => SelectAction::Continue,
        }
    }
}
