use std::sync::Arc;

use crate::crawler::CrawlStatus;
use crate::runner::ProgressEvent;

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
