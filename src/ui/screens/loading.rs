use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use std::time::Duration;

use crate::ui::{
    PromptOutcome, TerminalGuard, footer_hint, header_paragraph, is_ctrl_c, palette, styled_block,
};

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

            frame_ctx.render_widget(footer_hint("Esc back  •  Ctrl+C quit"), chunks[3]);
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
