use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Gauge, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use std::sync::Arc;

use crate::cli::CliOptions;
use crate::crawler::{CrawlStatus, ExistingFilePolicy};
use crate::runner::ProgressEvent;

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
        if event.code == KeyCode::Char('c')
            && event.modifiers.contains(KeyModifiers::CONTROL)
        {
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

/// Compute the longest common prefix shared by every entry in `strings`.
/// Returns the empty string for an empty slice.
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
        if event.code == KeyCode::Char('c')
            && event.modifiers.contains(KeyModifiers::CONTROL)
        {
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

/// One option in a [`Select`] widget.
#[derive(Debug, Clone)]
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
        if event.code == KeyCode::Char('c')
            && event.modifiers.contains(KeyModifiers::CONTROL)
        {
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

/// Top-level operating mode chosen during the interactive flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrawlMode {
    /// Download chapters but do not build an EPUB.
    Crawl,
    /// Download chapters and then build an EPUB.
    CrawlEpub,
    /// Skip downloading and only build the EPUB from existing files.
    EpubOnly,
}

/// Outcome of the interactive flow when the user confirms the plan.
#[derive(Debug, Clone)]
pub struct InteractivePlan {
    /// Novel base URL.
    pub base_url: String,
    /// Operating mode chosen by the user.
    pub mode: CrawlMode,
    /// Resolved output root directory.
    pub output_root: PathBuf,
    /// Chapter range, or `None` for `EpubOnly` mode.
    pub chapter_numbers: Option<Vec<u32>>,
    /// Sleep between successful chapter writes.
    pub delay: f64,
    /// Concurrency level.
    pub workers: usize,
    /// Whether an EPUB should be built.
    pub epub: bool,
    /// Existing chapter directory override (used by `EpubOnly`).
    pub chapter_dir: Option<PathBuf>,
    /// Optional embedded font override.
    pub font_path: Option<PathBuf>,
    /// Existing-file policy.
    pub if_exists: ExistingFilePolicy,
    /// Fast-skip flag.
    pub fast_skip: bool,
    /// Discovered novel title (for fast-skip path resolution).
    pub novel_title: Option<String>,
}

/// Holds the ratatui terminal and ensures the alternate screen + raw mode
/// state are restored on drop, even on panic.
pub struct TerminalGuard {
    /// Wrapped ratatui terminal handle.
    pub terminal: Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    /// Enter raw mode and switch to the alternate screen, returning a guard.
    pub fn enter() -> Result<Self> {
        enable_raw_mode().context("failed to enable raw mode")?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen).context("failed to switch to alt screen")?;
        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fullscreen,
            },
        )?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    /// Restore normal terminal state.
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

/// Project-wide TUI color palette. Centralised so every screen feels coherent.
mod palette {
    use ratatui::style::Color;
    /// Primary accent color — chrome, focused borders, app banner.
    pub const PRIMARY: Color = Color::Rgb(99, 195, 255);
    /// Secondary accent — selected list rows, action highlights.
    pub const ACCENT: Color = Color::Rgb(255, 159, 88);
    /// Muted text — placeholders, secondary labels, hints.
    pub const MUTED: Color = Color::Rgb(140, 140, 140);
    /// Success / positive feedback color.
    pub const SUCCESS: Color = Color::Rgb(118, 224, 144);
    /// Error / validation feedback color.
    pub const DANGER: Color = Color::Rgb(255, 105, 130);
}

/// Build a rounded, primary-coloured block with the supplied title.
fn styled_block(title: &str) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(palette::PRIMARY))
        .title(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ))
}

/// Build the app banner shown at the top of every screen.
fn header_paragraph() -> Paragraph<'static> {
    let banner = Line::from(vec![
        Span::styled(
            " truyenazz-crawl ",
            Style::default()
                .fg(Color::Black)
                .bg(palette::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "interactive setup",
            Style::default()
                .fg(palette::MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
    ]);
    Paragraph::new(banner).alignment(Alignment::Left)
}

/// Build the standard footer hint shown on most screens.
fn footer_hint(text: &str) -> Paragraph<'_> {
    Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(palette::MUTED),
    )))
    .alignment(Alignment::Center)
}

/// Read the next key press, ignoring everything else (resize, mouse, paste).
/// Ctrl+C is passed through unchanged so the run_* layer can distinguish it
/// from Esc (which means "back to previous step").
fn next_key_event() -> Result<KeyEvent> {
    loop {
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(event) = event::read()?
        {
            if event.kind == KeyEventKind::Release {
                continue;
            }
            return Ok(event);
        }
    }
}

/// Returns true when `event` represents a Ctrl+C key press.
fn is_ctrl_c(event: &KeyEvent) -> bool {
    event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL)
}

/// Outcome of any wizard-style prompt screen. Lets the caller wire up
/// step-by-step navigation: `Submitted(value)` advances, `Back` returns to
/// the previous step, and `Quit` exits the wizard entirely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOutcome<T> {
    /// The user submitted a value (Enter pressed and validation passed).
    Submitted(T),
    /// The user pressed Esc — treat as "back".
    Back,
    /// The user pressed Ctrl+C — treat as "exit the program".
    Quit,
}

/// Render a one-shot text prompt: title at top, value below, optional error.
fn draw_text_prompt(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    title: &str,
    message: &str,
    input: &TextInput,
    placeholder: Option<&str>,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area().inner(Margin::new(2, 1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(prompt_block_height(message)),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

        frame.render_widget(header_paragraph(), chunks[0]);

        let prompt = Paragraph::new(message)
            .block(styled_block(title))
            .wrap(Wrap { trim: false });
        frame.render_widget(prompt, chunks[2]);

        let value_style = Style::default()
            .fg(palette::ACCENT)
            .add_modifier(Modifier::BOLD);
        let display: Line = if input.value().is_empty() {
            match placeholder {
                Some(p) => Line::from(Span::styled(
                    p.to_string(),
                    Style::default()
                        .fg(palette::MUTED)
                        .add_modifier(Modifier::ITALIC),
                )),
                None => Line::from(""),
            }
        } else {
            Line::from(vec![Span::styled(input.value().to_string(), value_style)])
        };
        let mut input_block = styled_block("Input");
        if input.error().is_some() {
            input_block = input_block.border_style(Style::default().fg(palette::DANGER));
        }
        let value = Paragraph::new(display).block(input_block);
        frame.render_widget(value, chunks[3]);

        let status_text = match input.error() {
            Some(err) => Line::from(vec![
                Span::styled(
                    "✗ ",
                    Style::default()
                        .fg(palette::DANGER)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(err.to_string(), Style::default().fg(palette::DANGER)),
            ]),
            None => Line::from(vec![
                Span::styled(
                    "↵",
                    Style::default()
                        .fg(palette::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" confirm   ", Style::default().fg(palette::MUTED)),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(palette::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" back   ", Style::default().fg(palette::MUTED)),
                Span::styled(
                    "Ctrl+C",
                    Style::default()
                        .fg(palette::DANGER)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" quit", Style::default().fg(palette::MUTED)),
            ]),
        };
        let status = Paragraph::new(status_text).block(styled_block("Status"));
        frame.render_widget(status, chunks[4]);

        frame.render_widget(
            footer_hint("type to edit  •  Backspace to delete"),
            chunks[6],
        );
    })?;
    Ok(())
}

/// Run the text prompt synchronously and return the resulting
/// [`PromptOutcome`]: `Submitted(value)` on Enter, `Back` on Esc,
/// `Quit` on Ctrl+C.
pub fn run_text_prompt(
    title: &str,
    message: &str,
    initial: Option<String>,
    placeholder: Option<&str>,
    validator: Option<Validator>,
) -> Result<PromptOutcome<String>> {
    let mut guard = TerminalGuard::enter()?;
    let mut input = match validator {
        Some(v) => TextInput {
            value: String::new(),
            error: None,
            validator: Some(v),
        },
        None => TextInput::new(),
    };
    if let Some(value) = initial {
        input.set_value(value);
    }
    loop {
        draw_text_prompt(&mut guard.terminal, title, message, &input, placeholder)?;
        let event = next_key_event()?;
        match input.handle_key(event) {
            TextInputAction::Submit => {
                return Ok(PromptOutcome::Submitted(input.value().to_string()));
            }
            TextInputAction::Cancel => return Ok(PromptOutcome::Back),
            TextInputAction::Quit => return Ok(PromptOutcome::Quit),
            TextInputAction::Invalid(_) | TextInputAction::Continue => continue,
        }
    }
}

/// Render the path-picker prompt: header, title block, input row, scrollable
/// suggestion list with the highlighted entry reversed, status footer.
fn draw_path_prompt(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    title: &str,
    message: &str,
    input: &PathInput,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area().inner(Margin::new(2, 1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(prompt_block_height(message)),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        frame.render_widget(header_paragraph(), chunks[0]);

        let prompt = Paragraph::new(message)
            .block(styled_block(title))
            .wrap(Wrap { trim: false });
        frame.render_widget(prompt, chunks[2]);

        let value_style = Style::default()
            .fg(palette::ACCENT)
            .add_modifier(Modifier::BOLD);
        let value_line: Line = if input.value().is_empty() {
            Line::from(Span::styled(
                "Type to start; Tab completes; ↑/↓ pick a match.",
                Style::default()
                    .fg(palette::MUTED)
                    .add_modifier(Modifier::ITALIC),
            ))
        } else {
            Line::from(Span::styled(input.value().to_string(), value_style))
        };
        let input_block = Paragraph::new(value_line).block(styled_block("Path"));
        frame.render_widget(input_block, chunks[3]);

        let suggestions = input.suggestions();
        let mut list_state = ListState::default();
        if let Some(i) = input.highlighted() {
            list_state.select(Some(i));
        }
        let items: Vec<ListItem> = suggestions
            .iter()
            .map(|p| ListItem::new(Line::from(p.clone())))
            .collect();
        let list_title = if suggestions.is_empty() {
            "Suggestions  (none)"
        } else {
            "Suggestions"
        };
        let list = List::new(items)
            .block(styled_block(list_title))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[4], &mut list_state);

        frame.render_widget(
            footer_hint(
                "Tab complete  •  ↑/↓ navigate  •  ↵ pick / submit  •  Esc back  •  Ctrl+C quit",
            ),
            chunks[5],
        );
    })?;
    Ok(())
}

/// Run the path-picker prompt synchronously and return the resulting
/// [`PromptOutcome`]: `Submitted(path)`, `Back` on Esc, `Quit` on Ctrl+C.
pub fn run_path_prompt(
    title: &str,
    message: &str,
    initial: Option<String>,
) -> Result<PromptOutcome<String>> {
    let mut guard = TerminalGuard::enter()?;
    let mut input = PathInput::new();
    if let Some(value) = initial {
        input.set_value(value);
    } else {
        input.refresh_completions();
    }
    loop {
        draw_path_prompt(&mut guard.terminal, title, message, &input)?;
        let event = next_key_event()?;
        match input.handle_key(event) {
            PathInputAction::Submit => {
                return Ok(PromptOutcome::Submitted(input.value().to_string()));
            }
            PathInputAction::Cancel => return Ok(PromptOutcome::Back),
            PathInputAction::Quit => return Ok(PromptOutcome::Quit),
            PathInputAction::Continue => continue,
        }
    }
}

/// Render a select prompt: title + message, scrollable list, status line.
fn draw_select<T>(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    title: &str,
    message: &str,
    select: &Select<T>,
    list_state: &mut ListState,
) -> Result<()>
where
    T: Clone,
{
    terminal.draw(|frame| {
        let area = frame.area().inner(Margin::new(2, 1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(prompt_block_height(message)),
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(area);
        frame.render_widget(header_paragraph(), chunks[0]);

        let prompt = Paragraph::new(message)
            .block(styled_block(title))
            .wrap(Wrap { trim: false });
        frame.render_widget(prompt, chunks[2]);

        let items: Vec<ListItem> = select
            .options()
            .iter()
            .map(|o| {
                let mut spans = vec![Span::raw(o.label.clone())];
                if let Some(hint) = &o.hint {
                    spans.push(Span::styled(
                        format!("  ({})", hint),
                        Style::default()
                            .fg(palette::MUTED)
                            .add_modifier(Modifier::ITALIC),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items)
            .block(styled_block(select.list_title()))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[3], list_state);

        frame.render_widget(
            footer_hint("↑/↓ move  •  ↵ confirm  •  Esc back  •  Ctrl+C quit"),
            chunks[4],
        );
    })?;
    Ok(())
}

/// Run a select prompt synchronously and return the resulting
/// [`PromptOutcome`]: `Submitted(value)`, `Back` on Esc, `Quit` on Ctrl+C.
pub fn run_select<T>(
    title: &str,
    message: &str,
    mut select: Select<T>,
) -> Result<PromptOutcome<T>>
where
    T: Clone,
{
    let mut guard = TerminalGuard::enter()?;
    let mut list_state = ListState::default();
    list_state.select(Some(select.cursor()));
    loop {
        draw_select(
            &mut guard.terminal,
            title,
            message,
            &select,
            &mut list_state,
        )?;
        let event = next_key_event()?;
        let action = select.handle_key(event);
        list_state.select(Some(select.cursor()));
        match action {
            SelectAction::Submit(value) => return Ok(PromptOutcome::Submitted(value)),
            SelectAction::Cancel => return Ok(PromptOutcome::Back),
            SelectAction::Quit => return Ok(PromptOutcome::Quit),
            SelectAction::Continue => continue,
        }
    }
}

/// Display a static text screen and wait for a keypress: Enter advances
/// (Submitted), Esc goes back, Ctrl+C quits.
pub fn show_note(title: &str, body: &str) -> Result<PromptOutcome<()>> {
    let mut guard = TerminalGuard::enter()?;
    loop {
        guard.terminal.draw(|frame| {
            let area = frame.area().inner(Margin::new(2, 1));
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(1),
                ])
                .split(area);
            frame.render_widget(header_paragraph(), chunks[0]);
            let para = Paragraph::new(body)
                .block(styled_block(title))
                .wrap(Wrap { trim: false });
            frame.render_widget(para, chunks[2]);
            frame.render_widget(
                footer_hint("↵ continue  •  Esc back  •  Ctrl+C quit"),
                chunks[3],
            );
        })?;
        let event = next_key_event()?;
        if is_ctrl_c(&event) {
            return Ok(PromptOutcome::Quit);
        }
        match event.code {
            KeyCode::Enter => return Ok(PromptOutcome::Submitted(())),
            KeyCode::Esc => return Ok(PromptOutcome::Back),
            _ => continue,
        }
    }
}

/// Compute the height of a prompt's title block so that every line in the
/// `message` is visible. The result includes the two border rows and is
/// floor-clamped to 5 so a one-line message still has a comfortable inner
/// height of 3 rows.
pub fn prompt_block_height(message: &str) -> u16 {
    let lines = message.lines().count().max(1);
    let total = lines as u16 + 2;
    total.max(5)
}

/// Animated braille frames used by the loading-screen spinner.
const SPINNER_FRAMES: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
];

/// Drive a long-running async task while keeping a styled "loading" box
/// on screen so the bare terminal never flashes through between prompts.
///
/// Returns `PromptOutcome::Submitted(value)` when the future completes.
/// Esc aborts the task and returns `Back` (so the caller can re-show the
/// previous wizard step); Ctrl+C aborts and returns `Quit`.
pub async fn run_loading_screen<F, T>(
    title: &str,
    message: &str,
    future: F,
) -> Result<PromptOutcome<T>>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let mut guard = TerminalGuard::enter()?;
    let mut task = tokio::spawn(future);
    let started_at = std::time::Instant::now();
    let mut frame: usize = 0;
    let mut user_decision: Option<PromptOutcome<()>> = None;
    loop {
        let elapsed = started_at.elapsed().as_secs();
        let spinner = SPINNER_FRAMES[frame % SPINNER_FRAMES.len()].to_string();
        guard.terminal.draw(|frame_ctx| {
            let area = frame_ctx.area().inner(Margin::new(2, 1));
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(1),
                ])
                .split(area);
            frame_ctx.render_widget(header_paragraph(), chunks[0]);

            let body: Vec<Line> = vec![
                Line::from(""),
                Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        spinner.clone(),
                        Style::default()
                            .fg(palette::PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled(
                        message.to_string(),
                        Style::default()
                            .fg(palette::ACCENT)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    format!("  Elapsed: {}s", elapsed),
                    Style::default().fg(palette::MUTED),
                )),
            ];
            let para = Paragraph::new(body)
                .block(styled_block(title))
                .wrap(Wrap { trim: false });
            frame_ctx.render_widget(para, chunks[2]);

            frame_ctx.render_widget(
                footer_hint("Esc back  •  Ctrl+C quit"),
                chunks[3],
            );
        })?;

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
        {
            if is_ctrl_c(&key) {
                user_decision = Some(PromptOutcome::Quit);
                task.abort();
                break;
            }
            if key.code == KeyCode::Esc {
                user_decision = Some(PromptOutcome::Back);
                task.abort();
                break;
            }
        }
        frame = frame.wrapping_add(1);
        if task.is_finished() {
            break;
        }
    }
    if let Some(decision) = user_decision {
        // Drain the aborted task to release resources, then surface the
        // user's choice.
        let _ = (&mut task).await;
        return Ok(match decision {
            PromptOutcome::Back => PromptOutcome::Back,
            PromptOutcome::Quit => PromptOutcome::Quit,
            PromptOutcome::Submitted(()) => unreachable!(),
        });
    }
    let value = (&mut task)
        .await
        .map_err(|e| anyhow::anyhow!("loading task panicked: {e}"))?;
    Ok(PromptOutcome::Submitted(value))
}

/// Convenience wrapper around [`run_select`] that builds a yes/no select.
pub fn run_confirm(
    title: &str,
    message: &str,
    default_yes: bool,
) -> Result<PromptOutcome<bool>> {
    let options = vec![
        SelectOption {
            label: "Yes".to_string(),
            value: true,
            hint: None,
        },
        SelectOption {
            label: "No".to_string(),
            value: false,
            hint: None,
        },
    ];
    let initial = default_yes;
    run_select(
        title,
        message,
        Select::with_initial(options, &initial).with_list_title("Proceed"),
    )
}

/// Run the whole interactive flow and return the resolved [`InteractivePlan`]
/// or `None` if the user cancelled. Performs an async novel discovery in the
/// middle so the call must be made from a Tokio runtime.
pub async fn run_interactive_flow(
    initial_base_url: Option<String>,
    options: &CliOptions,
) -> Result<Option<InteractivePlan>> {
    let mut state = WizardState::seed(initial_base_url, options);
    let mut step = WizardStep::Welcome;
    loop {
        step = match advance_step(step, &mut state).await? {
            StepResult::Next(next) => next,
            StepResult::Quit => return Ok(None),
            StepResult::Done(plan) => return Ok(Some(plan)),
        };
    }
}

/// One screen in the interactive wizard's step machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardStep {
    Welcome,
    BaseUrl,
    Mode,
    OutputRoot,
    Discover,
    StartChapter,
    EndChapter,
    Workers,
    Delay,
    IfExists,
    ChapterDir,
    FastSkip,
    FontChoice,
    FontPath,
    Confirm,
}

/// Outcome of running one wizard step.
enum StepResult {
    /// Move on to the named step.
    Next(WizardStep),
    /// User pressed Ctrl+C — abort the wizard.
    Quit,
    /// User confirmed the plan; surface the resulting [`InteractivePlan`].
    Done(InteractivePlan),
}

/// Whether the user picked the bundled font or a custom file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FontChoice {
    Default,
    Custom,
}

/// Mutable wizard state threaded across step transitions. Defaults are seeded
/// from the parsed CLI options so that flags pre-populate the prompts.
struct WizardState {
    has_initial_url: bool,
    base_url: String,
    mode: CrawlMode,
    output_root: PathBuf,
    novel_title: Option<String>,
    novel_status: Option<String>,
    novel_description: Option<String>,
    last_discovered: Option<u32>,
    start_chapter: u32,
    end_chapter: u32,
    workers: usize,
    delay: f64,
    if_exists: ExistingFilePolicy,
    fast_skip: bool,
    chapter_dir: Option<PathBuf>,
    font_choice: FontChoice,
    font_path: Option<PathBuf>,
}

impl WizardState {
    /// Build the initial state from CLI options, pre-filling every field
    /// with a sensible default so back-navigation never hits unset values.
    fn seed(initial_base_url: Option<String>, options: &CliOptions) -> Self {
        let mode = if options.epub_only {
            CrawlMode::EpubOnly
        } else if options.epub {
            CrawlMode::CrawlEpub
        } else {
            CrawlMode::Crawl
        };
        Self {
            has_initial_url: initial_base_url.is_some(),
            base_url: initial_base_url.unwrap_or_default(),
            mode,
            output_root: PathBuf::from(&options.output_root),
            novel_title: None,
            novel_status: None,
            novel_description: None,
            last_discovered: None,
            start_chapter: options.start.unwrap_or(1),
            end_chapter: options.end.unwrap_or(0),
            workers: options.workers.max(1),
            delay: options.delay.max(0.0),
            if_exists: options.if_exists,
            fast_skip: options.fast_skip,
            chapter_dir: options.chapter_dir.as_ref().map(PathBuf::from),
            font_choice: if options.font_path.is_some() {
                FontChoice::Custom
            } else {
                FontChoice::Default
            },
            font_path: options.font_path.as_ref().map(PathBuf::from),
        }
    }
}

/// Step-machine driver. Dispatches to the per-step renderer/prompter, then
/// returns where to go next based on the user's decision.
async fn advance_step(step: WizardStep, state: &mut WizardState) -> Result<StepResult> {
    use WizardStep::*;
    match step {
        Welcome => step_welcome(state),
        BaseUrl => step_base_url(state),
        Mode => step_mode(state),
        OutputRoot => step_output_root(state),
        Discover => step_discover(state).await,
        StartChapter => step_start_chapter(state),
        EndChapter => step_end_chapter(state),
        Workers => step_workers(state),
        Delay => step_delay(state),
        IfExists => step_if_exists(state),
        ChapterDir => step_chapter_dir(state),
        FastSkip => step_fast_skip(state),
        FontChoice => step_font_choice(state),
        FontPath => step_font_path(state),
        Confirm => step_confirm(state),
    }
}

/// Tiny convenience: turn a [`PromptOutcome`] into a `StepResult` for the
/// happy path, mapping Quit → Quit and Back → the supplied previous step.
macro_rules! advance_or_back {
    ($outcome:expr, $previous:expr, |$value:ident| $on_submit:block) => {
        match $outcome {
            PromptOutcome::Submitted($value) => $on_submit,
            PromptOutcome::Back => Ok(StepResult::Next($previous)),
            PromptOutcome::Quit => Ok(StepResult::Quit),
        }
    };
}

/// Welcome screen. Esc cancels the wizard since there is no earlier step.
fn step_welcome(_state: &mut WizardState) -> Result<StepResult> {
    match show_note(
        "truyenazz-crawl",
        "Welcome — let's set up the crawl.\n\nPress Enter to continue, Esc/Ctrl+C to quit.",
    )? {
        PromptOutcome::Submitted(()) => Ok(StepResult::Next(WizardStep::BaseUrl)),
        PromptOutcome::Back | PromptOutcome::Quit => Ok(StepResult::Quit),
    }
}

/// Novel base URL prompt. Skipped entirely if the URL was supplied on the CLI.
fn step_base_url(state: &mut WizardState) -> Result<StepResult> {
    if state.has_initial_url {
        return Ok(StepResult::Next(WizardStep::Mode));
    }
    let validator: Validator = Box::new(|value: &str| {
        let trimmed = value.trim();
        let valid = !trimmed.is_empty()
            && (trimmed.starts_with("http://") || trimmed.starts_with("https://"));
        if valid {
            None
        } else {
            Some("Enter a valid http:// or https:// URL.".to_string())
        }
    });
    let outcome = run_text_prompt(
        "Novel base URL",
        "Paste the novel base URL.",
        Some(state.base_url.clone()).filter(|s| !s.is_empty()),
        Some("https://truyenazz.me/your-novel"),
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::Welcome, |value| {
        state.base_url = value.trim().to_string();
        // Invalidate any previously cached discovery whenever the URL changes.
        state.novel_title = None;
        state.last_discovered = None;
        Ok(StepResult::Next(WizardStep::Mode))
    })
}

/// Operating-mode select.
fn step_mode(state: &mut WizardState) -> Result<StepResult> {
    let mode_options = vec![
        SelectOption {
            label: "Crawl chapters".into(),
            value: CrawlMode::Crawl,
            hint: None,
        },
        SelectOption {
            label: "Crawl chapters and build an EPUB".into(),
            value: CrawlMode::CrawlEpub,
            hint: None,
        },
        SelectOption {
            label: "Build an EPUB from existing chapter files".into(),
            value: CrawlMode::EpubOnly,
            hint: None,
        },
    ];
    let outcome = run_select(
        "Mode",
        "What do you want to do?",
        Select::with_initial(mode_options, &state.mode),
    )?;
    advance_or_back!(outcome, WizardStep::BaseUrl, |chosen| {
        state.mode = chosen;
        Ok(StepResult::Next(WizardStep::OutputRoot))
    })
}

/// Output root prompt.
fn step_output_root(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_text_prompt(
        "Output root",
        "Where should chapter files (and the EPUB) be saved?",
        Some(state.output_root.to_string_lossy().into_owned()),
        None,
        Some(Box::new(|v| {
            if v.trim().is_empty() {
                Some("Enter an output directory.".into())
            } else {
                None
            }
        })),
    )?;
    advance_or_back!(outcome, WizardStep::Mode, |value| {
        state.output_root = PathBuf::from(value.trim());
        let next = if state.mode == CrawlMode::EpubOnly {
            WizardStep::ChapterDir
        } else {
            WizardStep::Discover
        };
        Ok(StepResult::Next(next))
    })
}

/// Aggregated novel metadata pulled out of the main page during discovery.
struct DiscoveredNovel {
    title: Option<String>,
    last_chapter: Option<u32>,
    status: Option<String>,
    description: Option<String>,
}

/// Run the title + status + description + last-chapter discovery under a
/// styled loading screen, then show a brief novel-info note.
async fn step_discover(state: &mut WizardState) -> Result<StepResult> {
    let url = state.base_url.clone();
    let outcome = run_loading_screen(
        "Discovering novel",
        "Fetching main page and detecting latest chapter…",
        async move {
            let main_url = format!("{}/", url.trim_end_matches('/'));
            let html = match crate::utils::fetch_html(&main_url).await {
                Ok(h) => h,
                Err(_) => {
                    return DiscoveredNovel {
                        title: None,
                        last_chapter: None,
                        status: None,
                        description: None,
                    };
                }
            };
            DiscoveredNovel {
                title: Some(crate::epub::extract_novel_title_from_main_page(&html)),
                last_chapter: crate::crawler::discover_last_chapter_number_from_html(
                    &html, &main_url,
                )
                .ok(),
                status: crate::epub::extract_novel_status_from_main_page(&html),
                description: crate::epub::extract_novel_description_from_main_page(&html),
            }
        },
    )
    .await?;
    let novel = match outcome {
        PromptOutcome::Submitted(novel) => novel,
        PromptOutcome::Back => return Ok(StepResult::Next(WizardStep::OutputRoot)),
        PromptOutcome::Quit => return Ok(StepResult::Quit),
    };
    state.novel_title = novel.title;
    state.last_discovered = novel.last_chapter;
    state.novel_status = novel.status;
    state.novel_description = novel.description;

    if state.novel_title.is_some() || state.last_discovered.is_some() {
        let mut lines: Vec<String> = Vec::new();
        if let Some(title) = state.novel_title.as_ref() {
            lines.push(format!("Title: {}", title));
        }
        if let Some(status) = state.novel_status.as_ref() {
            lines.push(format!("Status: {}", status));
        }
        if let Some(last) = state.last_discovered {
            lines.push(format!("Latest chapter: {}", last));
        }
        if let Some(desc) = state.novel_description.as_ref() {
            lines.push(String::new());
            lines.push("Description:".to_string());
            lines.push(desc.clone());
        }
        match show_note("Novel", &lines.join("\n"))? {
            PromptOutcome::Submitted(()) => {}
            PromptOutcome::Back => return Ok(StepResult::Next(WizardStep::OutputRoot)),
            PromptOutcome::Quit => return Ok(StepResult::Quit),
        }
    }
    Ok(StepResult::Next(WizardStep::StartChapter))
}

/// Start chapter prompt.
fn step_start_chapter(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<u32>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let initial = if state.start_chapter == 0 {
        1
    } else {
        state.start_chapter
    };
    let outcome = run_text_prompt(
        "Start chapter",
        "First chapter to download.",
        Some(initial.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::OutputRoot, |value| {
        state.start_chapter = value.trim().parse().unwrap_or(1);
        Ok(StepResult::Next(WizardStep::EndChapter))
    })
}

/// End chapter prompt.
fn step_end_chapter(state: &mut WizardState) -> Result<StepResult> {
    let initial = if state.end_chapter > 0 {
        state.end_chapter
    } else {
        state
            .last_discovered
            .unwrap_or_else(|| state.start_chapter.max(1))
    };
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<u32>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let outcome = run_text_prompt(
        "End chapter",
        "Last chapter to download (inclusive).",
        Some(initial.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::StartChapter, |value| {
        let parsed: u32 = value.trim().parse().unwrap_or(state.start_chapter);
        state.end_chapter = parsed;
        if let Some(message) = crate::cli::validate_chapter_range(state.start_chapter, parsed) {
            // Show the validation error and then fall back to the prior step
            // so the user can pick a valid range.
            let _ = show_note("Invalid range", &message)?;
            return Ok(StepResult::Next(WizardStep::StartChapter));
        }
        Ok(StepResult::Next(WizardStep::Workers))
    })
}

/// Workers prompt.
fn step_workers(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<usize>() {
        Ok(n) if n > 0 => None,
        _ => Some("Enter a positive integer.".into()),
    });
    let outcome = run_text_prompt(
        "Workers",
        "How many download workers should run in parallel?",
        Some(state.workers.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::EndChapter, |value| {
        state.workers = value.trim().parse().unwrap_or(1).max(1);
        Ok(StepResult::Next(WizardStep::Delay))
    })
}

/// Delay prompt.
fn step_delay(state: &mut WizardState) -> Result<StepResult> {
    let validator: Validator = Box::new(|v: &str| match v.trim().parse::<f64>() {
        Ok(n) if n >= 0.0 => None,
        _ => Some("Enter a non-negative number.".into()),
    });
    let outcome = run_text_prompt(
        "Delay",
        "Delay between requests (seconds).",
        Some(state.delay.to_string()),
        None,
        Some(validator),
    )?;
    advance_or_back!(outcome, WizardStep::Workers, |value| {
        let parsed: f64 = value.trim().parse().unwrap_or(0.0);
        state.delay = parsed.max(0.0);
        Ok(StepResult::Next(WizardStep::IfExists))
    })
}

/// Existing-file policy select. Hides the `Ask` option when running in parallel.
fn step_if_exists(state: &mut WizardState) -> Result<StepResult> {
    let mut allowed = Vec::new();
    if state.workers <= 1 {
        allowed.push(SelectOption {
            label: "Ask what to do for each existing chapter".into(),
            value: ExistingFilePolicy::Ask,
            hint: None,
        });
    }
    allowed.push(SelectOption {
        label: "Skip existing chapter files".into(),
        value: ExistingFilePolicy::Skip,
        hint: None,
    });
    allowed.push(SelectOption {
        label: "Overwrite existing chapter files".into(),
        value: ExistingFilePolicy::Overwrite,
        hint: None,
    });
    let initial_policy = if state.workers > 1 && state.if_exists == ExistingFilePolicy::Ask {
        ExistingFilePolicy::Skip
    } else {
        state.if_exists
    };
    let outcome = run_select(
        "If chapter exists",
        "Pick a behaviour for existing chapter files.",
        Select::with_initial(allowed, &initial_policy),
    )?;
    advance_or_back!(outcome, WizardStep::Delay, |value| {
        state.if_exists = value;
        Ok(StepResult::Next(WizardStep::FastSkip))
    })
}

/// Chapter directory prompt — only used in `EpubOnly` mode.
fn step_chapter_dir(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_text_prompt(
        "Chapter directory",
        "Path to the existing chapter directory.",
        state
            .chapter_dir
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
        None,
        None,
    )?;
    advance_or_back!(outcome, WizardStep::OutputRoot, |value| {
        state.chapter_dir = Some(PathBuf::from(value.trim()));
        Ok(StepResult::Next(WizardStep::FontChoice))
    })
}

/// Fast-skip yes/no.
fn step_fast_skip(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_confirm(
        "Fast skip",
        "Bypass the remote check when the chapter file already exists locally?",
        state.fast_skip,
    )?;
    advance_or_back!(outcome, WizardStep::IfExists, |value| {
        state.fast_skip = value;
        let next = if state.mode == CrawlMode::Crawl {
            WizardStep::Confirm
        } else {
            WizardStep::FontChoice
        };
        Ok(StepResult::Next(next))
    })
}

/// Choose between bundled vs custom EPUB font.
fn step_font_choice(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_select(
        "EPUB font",
        "Pick the font embedded in the EPUB.",
        Select::with_initial(
            vec![
                SelectOption {
                    label: "Use the bundled Bokerlam.ttf".into(),
                    value: FontChoice::Default,
                    hint: None,
                },
                SelectOption {
                    label: "Pick a custom font file path".into(),
                    value: FontChoice::Custom,
                    hint: None,
                },
            ],
            &state.font_choice,
        ),
    )?;
    let previous = if state.mode == CrawlMode::EpubOnly {
        WizardStep::ChapterDir
    } else {
        WizardStep::FastSkip
    };
    advance_or_back!(outcome, previous, |choice| {
        state.font_choice = choice;
        let next = match choice {
            FontChoice::Custom => WizardStep::FontPath,
            FontChoice::Default => {
                state.font_path = None;
                WizardStep::Confirm
            }
        };
        Ok(StepResult::Next(next))
    })
}

/// Custom font path picker.
fn step_font_path(state: &mut WizardState) -> Result<StepResult> {
    let outcome = run_path_prompt(
        "Font path",
        "Absolute path to the .ttf/.otf file. Tab to autocomplete.",
        state
            .font_path
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned()),
    )?;
    advance_or_back!(outcome, WizardStep::FontChoice, |value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Ok(StepResult::Next(WizardStep::FontChoice));
        }
        state.font_path = Some(PathBuf::from(trimmed));
        Ok(StepResult::Next(WizardStep::Confirm))
    })
}

/// Final confirmation. Yes finalizes the plan; No goes back to the prior step.
fn step_confirm(state: &mut WizardState) -> Result<StepResult> {
    let chapter_numbers = if state.mode == CrawlMode::EpubOnly {
        None
    } else {
        Some(crate::cli::chapter_range(
            state.start_chapter,
            state.end_chapter,
        ))
    };
    let summary = build_summary(SummaryParams {
        base_url: &state.base_url,
        mode: state.mode,
        output_root: &state.output_root,
        chapter_numbers: chapter_numbers.as_deref(),
        delay: state.delay,
        workers: state.workers,
        if_exists: state.if_exists,
        chapter_dir: state.chapter_dir.as_deref(),
        font_path: state.font_path.as_deref(),
        fast_skip: state.fast_skip,
    });
    let previous = match state.mode {
        CrawlMode::Crawl => WizardStep::FastSkip,
        CrawlMode::CrawlEpub | CrawlMode::EpubOnly => match state.font_choice {
            FontChoice::Custom => WizardStep::FontPath,
            FontChoice::Default => WizardStep::FontChoice,
        },
    };
    let outcome = run_confirm("Plan", &summary, true)?;
    match outcome {
        PromptOutcome::Submitted(true) => Ok(StepResult::Done(InteractivePlan {
            base_url: state.base_url.clone(),
            mode: state.mode,
            output_root: state.output_root.clone(),
            chapter_numbers,
            delay: state.delay,
            workers: state.workers,
            epub: state.mode != CrawlMode::Crawl,
            chapter_dir: state.chapter_dir.clone(),
            font_path: state.font_path.clone(),
            if_exists: state.if_exists,
            fast_skip: state.fast_skip,
            novel_title: state.novel_title.clone(),
        })),
        PromptOutcome::Submitted(false) => Ok(StepResult::Next(previous)),
        PromptOutcome::Back => Ok(StepResult::Next(previous)),
        PromptOutcome::Quit => Ok(StepResult::Quit),
    }
}

/// Inputs to [`build_summary`].
pub struct SummaryParams<'a> {
    /// Resolved novel base URL.
    pub base_url: &'a str,
    /// Operating mode chosen by the user.
    pub mode: CrawlMode,
    /// Output root directory.
    pub output_root: &'a std::path::Path,
    /// Chapter range, or `None` for `EpubOnly` mode.
    pub chapter_numbers: Option<&'a [u32]>,
    /// Sleep between successful chapter writes.
    pub delay: f64,
    /// Concurrency level.
    pub workers: usize,
    /// Existing-file policy.
    pub if_exists: ExistingFilePolicy,
    /// Existing chapter directory override.
    pub chapter_dir: Option<&'a std::path::Path>,
    /// Optional embedded font override.
    pub font_path: Option<&'a std::path::Path>,
    /// Fast-skip flag.
    pub fast_skip: bool,
}

/// Render the plan summary text shown before confirmation.
pub fn build_summary(params: SummaryParams<'_>) -> String {
    let SummaryParams {
        base_url,
        mode,
        output_root,
        chapter_numbers,
        delay,
        workers,
        if_exists,
        chapter_dir,
        font_path,
        fast_skip,
    } = params;
    let mode_label = match mode {
        CrawlMode::Crawl => "Crawl chapters",
        CrawlMode::CrawlEpub => "Crawl chapters and build EPUB",
        CrawlMode::EpubOnly => "Build EPUB from existing chapters",
    };
    let if_exists_label = match if_exists {
        ExistingFilePolicy::Ask => "ask",
        ExistingFilePolicy::Skip => "skip",
        ExistingFilePolicy::Overwrite => "overwrite",
        ExistingFilePolicy::SkipAll => "skip-all",
    };
    let mut lines = vec![
        format!("Base URL: {}", base_url),
        format!("Mode: {}", mode_label),
        format!("Output root: {}", output_root.display()),
    ];

    let has_chapter_range = matches!(chapter_numbers, Some(c) if !c.is_empty());
    if let Some(chapters) = chapter_numbers
        && let (Some(first), Some(last)) = (chapters.first(), chapters.last())
    {
        lines.push(format!(
            "Chapters: {} -> {} ({} total)",
            first,
            last,
            chapters.len()
        ));
    }

    // Always show the per-run knobs whenever a download stage is part of the
    // plan, so the user can verify their choices on one screen.
    if mode != CrawlMode::EpubOnly || has_chapter_range {
        lines.push(format!("Workers: {}", workers));
        lines.push(format!("Delay: {}s", delay));
        lines.push(format!("If chapter exists: {}", if_exists_label));
        lines.push(format!(
            "Fast skip: {}",
            if fast_skip { "yes" } else { "no" }
        ));
    }

    if let Some(dir) = chapter_dir {
        lines.push(format!("Chapter directory: {}", dir.display()));
    }

    let build_epub = mode != CrawlMode::Crawl;
    lines.push(format!(
        "Build EPUB: {}",
        if build_epub { "yes" } else { "no" }
    ));
    if build_epub {
        let font_line = match font_path {
            Some(p) => format!("Font path: {}", p.display()),
            None => "Font path: default packaged font".into(),
        };
        lines.push(font_line);
    }
    lines.join("\n")
}

/// One line of the rolling download log shown in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadLogEntry {
    /// Chapter was written to disk.
    Ok(u32),
    /// Chapter file already existed and was skipped.
    Skip(u32),
    /// Chapter download failed.
    Fail(u32),
}

/// Default number of recent log entries kept for display.
const DEFAULT_LOG_WINDOW: usize = 500;

/// Mutable state backing the in-TUI download progress screen.
///
/// The progress callback installed on the runner pushes events into one of
/// these via `from_event`; the render loop reads it on every redraw tick.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    /// Total chapters expected in this run (used for the gauge denominator).
    pub total: u32,
    /// Chapters written successfully so far.
    pub completed: u32,
    /// Chapters that produced an error so far.
    pub failed: u32,
    /// The chapter the most recent `Started` event referenced, if any.
    pub current_chapter: Option<u32>,
    /// Rolling window of the most recent log entries.
    pub log: Vec<DownloadLogEntry>,
    /// Maximum number of log entries kept in `log`.
    pub log_capacity: usize,
    /// Set when the runner has finished — flips the screen into "done" mode.
    pub done: bool,
}

impl DownloadProgress {
    /// Construct an empty progress state for a run of `total` chapters.
    pub fn new(total: u32) -> Self {
        Self::with_log_capacity(total, DEFAULT_LOG_WINDOW)
    }

    /// Same as [`new`] but with a custom log window size.
    pub fn with_log_capacity(total: u32, log_capacity: usize) -> Self {
        Self {
            total,
            completed: 0,
            failed: 0,
            current_chapter: None,
            log: Vec::with_capacity(log_capacity),
            log_capacity,
            done: false,
        }
    }

    /// Record that chapter `number` is about to start downloading.
    pub fn record_started(&mut self, number: u32) {
        self.current_chapter = Some(number);
    }

    /// Record a successful (or skipped) chapter completion.
    pub fn record_completed(&mut self, number: u32, status: CrawlStatus) {
        self.completed += 1;
        let entry = match status {
            CrawlStatus::Written => DownloadLogEntry::Ok(number),
            CrawlStatus::Skipped | CrawlStatus::SkipAll => DownloadLogEntry::Skip(number),
        };
        self.push_log(entry);
    }

    /// Record a failed chapter download.
    pub fn record_failed(&mut self, number: u32) {
        self.failed += 1;
        self.push_log(DownloadLogEntry::Fail(number));
    }

    /// Mark the run as done so the TUI flips into "press Enter to continue" mode.
    pub fn finish(&mut self) {
        self.done = true;
    }

    /// Total chapters with a terminal event observed (completed + failed).
    pub fn advanced(&self) -> u32 {
        self.completed + self.failed
    }

    /// Apply a runner [`ProgressEvent`] to this state.
    pub fn from_event(&mut self, event: ProgressEvent) {
        match event {
            ProgressEvent::Started { number, .. } => self.record_started(number),
            ProgressEvent::Completed { number, status } => self.record_completed(number, status),
            ProgressEvent::Failed { number } => self.record_failed(number),
        }
    }

    /// Percentage complete (0..=100), rounded.
    pub fn percent(&self) -> u16 {
        if self.total == 0 {
            return 100;
        }
        let ratio = self.advanced() as f64 / self.total as f64;
        (ratio.clamp(0.0, 1.0) * 100.0).round() as u16
    }

    /// Push `entry` while preserving the rolling-window invariant.
    fn push_log(&mut self, entry: DownloadLogEntry) {
        self.log.push(entry);
        while self.log.len() > self.log_capacity {
            self.log.remove(0);
        }
    }
}

/// Build a runner progress callback that updates `state` from each event.
pub fn make_tui_progress_callback(
    state: Arc<std::sync::Mutex<DownloadProgress>>,
) -> crate::runner::ProgressCallback {
    Arc::new(move |event| {
        if let Ok(mut guard) = state.lock() {
            guard.from_event(event);
        }
    })
}

/// Render the in-TUI download progress screen: header, status, gauge, log.
fn draw_download(
    terminal: &mut Terminal<ratatui::backend::CrosstermBackend<Stdout>>,
    progress: &DownloadProgress,
) -> Result<()> {
    terminal.draw(|frame| {
        let area = frame.area().inner(Margin::new(2, 1));
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(4),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(area);

        frame.render_widget(header_paragraph(), chunks[0]);

        let title = if progress.done {
            "Done"
        } else {
            "Downloading chapters"
        };
        let cursor = match progress.current_chapter {
            Some(n) if !progress.done => format!("Working on chapter {}", n),
            Some(n) => format!("Last chapter: {}", n),
            None => "Waiting…".to_string(),
        };
        let status_lines: Vec<Line> = vec![
            Line::from(Span::styled(
                cursor,
                Style::default()
                    .fg(palette::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![
                Span::styled("OK ", Style::default().fg(palette::SUCCESS)),
                Span::styled(
                    progress.completed.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled("FAIL ", Style::default().fg(palette::DANGER)),
                Span::styled(
                    progress.failed.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw("   "),
                Span::styled("Total ", Style::default().fg(palette::MUTED)),
                Span::styled(
                    progress.total.to_string(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        let status = Paragraph::new(status_lines).block(styled_block(title));
        frame.render_widget(status, chunks[2]);

        let percent = progress.percent();
        let label = format!(
            "{} / {}  ({}%)",
            progress.advanced(),
            progress.total,
            percent
        );
        let gauge = Gauge::default()
            .block(styled_block("Progress"))
            .gauge_style(
                Style::default()
                    .fg(palette::PRIMARY)
                    .bg(Color::Rgb(20, 30, 40))
                    .add_modifier(Modifier::BOLD),
            )
            .percent(percent)
            .label(label);
        frame.render_widget(gauge, chunks[3]);

        let log_items: Vec<ListItem> = progress
            .log
            .iter()
            .rev()
            .map(|entry| match entry {
                DownloadLogEntry::Ok(n) => ListItem::new(Line::from(vec![
                    Span::styled(
                        " ✓ ",
                        Style::default()
                            .fg(palette::SUCCESS)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("Chapter {} written", n), Style::default()),
                ])),
                DownloadLogEntry::Skip(n) => ListItem::new(Line::from(vec![
                    Span::styled(" · ", Style::default().fg(palette::MUTED)),
                    Span::styled(
                        format!("Chapter {} skipped", n),
                        Style::default().fg(palette::MUTED),
                    ),
                ])),
                DownloadLogEntry::Fail(n) => ListItem::new(Line::from(vec![
                    Span::styled(
                        " ✗ ",
                        Style::default()
                            .fg(palette::DANGER)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("Chapter {} failed", n),
                        Style::default().fg(palette::DANGER),
                    ),
                ])),
            })
            .collect();
        let log = List::new(log_items).block(styled_block("Recent activity"));
        frame.render_widget(log, chunks[4]);

        let hint = if progress.done {
            "↵ continue"
        } else {
            "Esc to abort"
        };
        frame.render_widget(footer_hint(hint), chunks[5]);
    })?;
    Ok(())
}

/// Drive the TUI download screen for a runner already spawned as `runner_task`.
///
/// `state` is shared with the runner's progress callback so the screen always
/// reflects up-to-the-second status. The function:
///
/// - Redraws every ~80ms while the runner is active.
/// - Polls for keyboard input; an `Esc` press aborts the task.
/// - When the runner finishes, marks the state `done`, redraws once, and
///   waits for `Enter` (or a second `Esc`) before tearing the TUI down —
///   unless `wait_for_user` is `false`, in which case the function returns
///   immediately so the caller can chain into the next TUI screen (e.g.
///   the EPUB build screen) without flashing the bare terminal.
pub async fn run_download_screen(
    state: Arc<std::sync::Mutex<DownloadProgress>>,
    runner_task: tokio::task::JoinHandle<crate::runner::RunnerOutcome>,
    wait_for_user: bool,
) -> Result<crate::runner::RunnerOutcome> {
    let mut guard = TerminalGuard::enter()?;
    let mut runner_task = runner_task;
    loop {
        {
            let snapshot = state
                .lock()
                .map_err(|_| anyhow::anyhow!("download progress mutex poisoned"))?
                .clone();
            draw_download(&mut guard.terminal, &snapshot)?;
        }
        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
        {
            let aborted = key.kind != KeyEventKind::Release
                && (key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)));
            if aborted {
                runner_task.abort();
                break;
            }
        }
        if runner_task.is_finished() {
            break;
        }
    }

    let outcome = match (&mut runner_task).await {
        Ok(o) => o,
        Err(error) if error.is_cancelled() => crate::runner::RunnerOutcome {
            output_dir: None,
            failures: Vec::new(),
        },
        Err(error) => return Err(anyhow::anyhow!("download task panicked: {error}")),
    };

    {
        let mut snapshot = state
            .lock()
            .map_err(|_| anyhow::anyhow!("download progress mutex poisoned"))?;
        snapshot.finish();
        draw_download(&mut guard.terminal, &snapshot)?;
    }

    if wait_for_user {
        loop {
            let event = next_key_event()?;
            match event.code {
                KeyCode::Enter | KeyCode::Esc => break,
                _ => continue,
            }
        }
    }
    Ok(outcome)
}
