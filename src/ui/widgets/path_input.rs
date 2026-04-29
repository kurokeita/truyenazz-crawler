use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn longest_common_prefix(strings: &[String]) -> String {
    let first = match strings.first() {
        Some(s) => s,
        None => return String::new(),
    };
    if strings.len() == 1 {
        return first.clone();
    }
    let mut prefix_len = first.len();
    for s in &strings[1..] {
        let mut shared = 0usize;
        for (a, b) in first.bytes().zip(s.bytes()) {
            if a != b {
                break;
            }
            shared += 1;
        }
        if shared < prefix_len {
            prefix_len = shared;
        }
        if prefix_len == 0 {
            return String::new();
        }
    }
    // Slice on a UTF-8 boundary; back off if mid-codepoint.
    while prefix_len > 0 && !first.is_char_boundary(prefix_len) {
        prefix_len -= 1;
    }
    first[..prefix_len].to_string()
}

/// Return absolute filesystem paths whose names start with the basename of
/// `value`, looking inside the parent directory of `value`. A trailing
/// separator means "list everything under this directory". Returns an empty
/// vector when the parent does not exist or cannot be read.
pub fn path_completions(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    let path = std::path::Path::new(value);
    let (dir, prefix): (std::path::PathBuf, String) = if value.ends_with('/') {
        (path.to_path_buf(), String::new())
    } else {
        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let parent = if parent.as_os_str().is_empty() {
            std::path::PathBuf::from(".")
        } else {
            parent
        };
        let basename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (parent, basename)
    };

    let entries = match std::fs::read_dir(&dir) {
        Ok(it) => it,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        let mut full = dir.join(&name).to_string_lossy().into_owned();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            full.push('/');
        }
        out.push(full);
    }
    out.sort();
    out
}

/// Result of processing a single key event in [`PathInput`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathInputAction {
    /// Repaint and keep waiting for input.
    Continue,
    /// User pressed Enter on the bare value (no suggestion highlighted).
    Submit,
    /// User pressed Esc — semantically "go back to the previous step".
    Cancel,
    /// User pressed Ctrl+C — semantically "exit the wizard".
    Quit,
}

/// Filesystem-aware single-line input with tab-completion and a navigable
/// suggestion list. Designed for picking font / chapter-directory paths.
pub struct PathInput {
    value: String,
    suggestions: Vec<String>,
    highlighted: Option<usize>,
}

impl PathInput {
    /// Empty input with no suggestions.
    pub fn new() -> Self {
        Self {
            value: String::new(),
            suggestions: Vec::new(),
            highlighted: None,
        }
    }

    /// Replace the current value and recompute suggestions.
    pub fn set_value(&mut self, value: impl Into<String>) {
        self.value = value.into();
        self.refresh_completions();
    }

    /// Current text value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Read-only view of the suggestion list (renderer needs this).
    pub fn suggestions(&self) -> &[String] {
        &self.suggestions
    }

    /// Index of the currently highlighted suggestion, if any.
    pub fn highlighted(&self) -> Option<usize> {
        self.highlighted
    }

    /// Re-read the parent directory and refresh the suggestion list. Called
    /// implicitly after every value mutation.
    pub fn refresh_completions(&mut self) {
        self.suggestions = path_completions(&self.value);
        self.highlighted = None;
    }

    /// Apply the longest common prefix of all current suggestions to
    /// `value`. Returns true when the value actually grew.
    fn apply_tab_completion(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }
        let common = longest_common_prefix(&self.suggestions);
        if common.len() > self.value.len() && common.starts_with(&self.value) {
            self.value = common;
            self.refresh_completions();
            true
        } else if self.suggestions.len() == 1 {
            // Already at the common prefix but only one match; jump to it
            // outright.
            self.value = self.suggestions[0].clone();
            self.refresh_completions();
            true
        } else {
            false
        }
    }

    /// Process a key event and return the resulting action.
    pub fn handle_key(&mut self, event: KeyEvent) -> PathInputAction {
        if event.kind == KeyEventKind::Release {
            return PathInputAction::Continue;
        }
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return PathInputAction::Quit;
        }
        match event.code {
            KeyCode::Char(c) => {
                self.value.push(c);
                self.refresh_completions();
                PathInputAction::Continue
            }
            KeyCode::Backspace => {
                self.value.pop();
                self.refresh_completions();
                PathInputAction::Continue
            }
            KeyCode::Tab => {
                self.apply_tab_completion();
                PathInputAction::Continue
            }
            KeyCode::Down => {
                if self.suggestions.is_empty() {
                    return PathInputAction::Continue;
                }
                self.highlighted = Some(match self.highlighted {
                    None => 0,
                    Some(i) if i + 1 >= self.suggestions.len() => 0,
                    Some(i) => i + 1,
                });
                PathInputAction::Continue
            }
            KeyCode::Up => {
                if self.suggestions.is_empty() {
                    return PathInputAction::Continue;
                }
                self.highlighted = Some(match self.highlighted {
                    None => self.suggestions.len() - 1,
                    Some(0) => self.suggestions.len() - 1,
                    Some(i) => i - 1,
                });
                PathInputAction::Continue
            }
            KeyCode::Enter => {
                if let Some(i) = self.highlighted
                    && let Some(choice) = self.suggestions.get(i).cloned()
                {
                    self.value = choice;
                    self.refresh_completions();
                    return PathInputAction::Continue;
                }
                PathInputAction::Submit
            }
            KeyCode::Esc => PathInputAction::Cancel,
            _ => PathInputAction::Continue,
        }
    }
}

impl Default for PathInput {
    /// Same as [`PathInput::new`].
    fn default() -> Self {
        Self::new()
    }
}
