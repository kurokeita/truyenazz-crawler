use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::Alignment;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io::{self, Stdout};
use std::time::Duration;

mod plan;
mod screens;
mod widgets;
mod wizard;

pub use plan::{CrawlMode, InteractivePlan, SummaryParams, build_summary};
pub use screens::{
    prompt_block_height, run_confirm, run_download_screen, run_loading_screen, run_path_prompt,
    run_select, run_text_prompt, show_note,
};
pub use widgets::{
    DownloadLogEntry, DownloadProgress, PathInput, PathInputAction, Select, SelectAction,
    SelectOption, TextInput, TextInputAction, Validator, longest_common_prefix,
    make_tui_progress_callback, path_completions,
};
pub use wizard::run_interactive_flow;

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
pub(crate) mod palette {
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
pub(crate) fn styled_block(title: &str) -> Block<'_> {
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
pub(crate) fn header_paragraph() -> Paragraph<'static> {
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
pub(crate) fn footer_hint(text: &str) -> Paragraph<'_> {
    Paragraph::new(Line::from(Span::styled(
        text,
        Style::default().fg(palette::MUTED),
    )))
    .alignment(Alignment::Center)
}

/// Read the next key press, ignoring everything else (resize, mouse, paste).
/// Ctrl+C is passed through unchanged so the run_* layer can distinguish it
/// from Esc (which means "back to previous step").
pub(crate) fn next_key_event() -> Result<KeyEvent> {
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
pub(crate) fn is_ctrl_c(event: &KeyEvent) -> bool {
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
