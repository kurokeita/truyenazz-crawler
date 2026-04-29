use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout, Margin};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Gauge, List, ListItem, Paragraph};
use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use crate::ui::widgets::{DownloadLogEntry, DownloadProgress};
use crate::ui::{
    TerminalGuard, footer_hint, header_paragraph, next_key_event, palette, styled_block,
};

/// Render one frame of the download screen — header, status, gauge, recent
/// activity log, and footer hint — from the current `DownloadProgress`.
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
            cancelled: true,
            ..Default::default()
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
