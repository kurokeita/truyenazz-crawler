use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Result of processing a single key event in [`TextInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextInputAction {
    /// No transition — repaint and keep waiting for input.
    Continue,
    /// Validator rejected the value with this message.
    Invalid(String),
    /// User pressed Enter and the value passed validation.
    Submit,
    /// User pressed Esc — semantically "go back to the previous step".
    Cancel,
    /// User pressed Ctrl+C — semantically "exit the wizard".
    Quit,
}

/// Type alias for an optional synchronous validator. A validator returns
/// `Some(message)` to reject the value or `None` to accept it.
pub type Validator = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Single-line text input widget used throughout the interactive flow.
pub struct TextInput {
    value: String,
    error: Option<String>,
    validator: Option<Validator>,
}

impl TextInput {
    /// Create an empty input with no validator.
    pub fn new() -> Self {
        Self {
            value: String::new(),
            error: None,
            validator: None,
        }
    }

    /// Create an input with a validator function.
    pub fn with_validator<F>(validator: F) -> Self
    where
        F: Fn(&str) -> Option<String> + Send + Sync + 'static,
    {
        Self {
            value: String::new(),
            error: None,
            validator: Some(Box::new(validator)),
        }
    }

    /// Create an input from an already boxed validator.
    pub(crate) fn with_boxed_validator(validator: Validator) -> Self {
        Self {
            value: String::new(),
            error: None,
            validator: Some(validator),
        }
    }

    /// Set the current value, clearing any pending error.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.error = None;
    }

    /// Get the current value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Last validation error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Process a key event and return the resulting action.
    pub fn handle_key(&mut self, event: KeyEvent) -> TextInputAction {
        if event.kind == KeyEventKind::Release {
            return TextInputAction::Continue;
        }
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return TextInputAction::Quit;
        }
        match event.code {
            KeyCode::Char(c) => {
                self.value.push(c);
                self.error = None;
                TextInputAction::Continue
            }
            KeyCode::Backspace => {
                self.value.pop();
                self.error = None;
                TextInputAction::Continue
            }
            KeyCode::Enter => {
                if let Some(validator) = &self.validator
                    && let Some(message) = validator(&self.value)
                {
                    self.error = Some(message.clone());
                    return TextInputAction::Invalid(message);
                }
                self.error = None;
                TextInputAction::Submit
            }
            KeyCode::Esc => TextInputAction::Cancel,
            _ => TextInputAction::Continue,
        }
    }
}

impl Default for TextInput {
    /// Same as [`TextInput::new`].
    fn default() -> Self {
        Self::new()
    }
}
