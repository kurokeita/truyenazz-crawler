use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use truyenazz_crawler::crawler::CrawlStatus;
use truyenazz_crawler::crawler::ExistingFilePolicy;
use truyenazz_crawler::ui::{
    CrawlMode, DownloadLogEntry, DownloadProgress, PathInput, PathInputAction, Select,
    SelectOption, SummaryParams, TextInput, TextInputAction, build_summary,
    longest_common_prefix, path_completions, prompt_block_height,
};

/// Build a `KeyEvent` with no modifiers — a tiny ergonomic helper for the
/// tests below.
fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn text_input_appends_chars_typed_by_user() {
    let mut input = TextInput::new();
    input.handle_key(key(KeyCode::Char('h')));
    input.handle_key(key(KeyCode::Char('i')));
    assert_eq!(input.value(), "hi");
}

#[test]
fn text_input_backspace_removes_last_char() {
    let mut input = TextInput::new();
    input.set_value("ab");
    input.handle_key(key(KeyCode::Backspace));
    assert_eq!(input.value(), "a");
    input.handle_key(key(KeyCode::Backspace));
    assert_eq!(input.value(), "");
    input.handle_key(key(KeyCode::Backspace));
    assert_eq!(input.value(), "");
}

#[test]
fn text_input_enter_emits_submit() {
    let mut input = TextInput::new();
    input.set_value("done");
    let action = input.handle_key(key(KeyCode::Enter));
    assert_eq!(action, TextInputAction::Submit);
}

#[test]
fn text_input_esc_emits_cancel() {
    let mut input = TextInput::new();
    let action = input.handle_key(key(KeyCode::Esc));
    assert_eq!(action, TextInputAction::Cancel);
}

#[test]
fn text_input_ctrl_c_emits_quit_not_text_insert() {
    let mut input = TextInput::new();
    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let action = input.handle_key(event);
    assert_eq!(action, TextInputAction::Quit);
    assert_eq!(input.value(), "", "Ctrl+C must not insert 'c'");
}

#[test]
fn text_input_plain_c_still_inserts_character() {
    let mut input = TextInput::new();
    input.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert_eq!(input.value(), "c");
}

#[test]
fn text_input_runs_validator_on_submit() {
    let mut input = TextInput::with_validator(|value| {
        if value.is_empty() {
            Some("required".to_string())
        } else {
            None
        }
    });
    let action = input.handle_key(key(KeyCode::Enter));
    assert_eq!(action, TextInputAction::Invalid("required".to_string()));
    assert_eq!(input.error(), Some("required"));
    input.set_value("ok");
    assert_eq!(
        input.handle_key(key(KeyCode::Enter)),
        TextInputAction::Submit
    );
    assert!(input.error().is_none());
}

#[test]
fn select_arrow_keys_move_selection() {
    let mut select: Select<&'static str> = Select::new(vec![
        SelectOption {
            label: "A".into(),
            value: "a",
            hint: None,
        },
        SelectOption {
            label: "B".into(),
            value: "b",
            hint: None,
        },
        SelectOption {
            label: "C".into(),
            value: "c",
            hint: None,
        },
    ]);
    assert_eq!(select.selected_value(), Some(&"a"));
    select.handle_key(key(KeyCode::Down));
    assert_eq!(select.selected_value(), Some(&"b"));
    select.handle_key(key(KeyCode::Down));
    assert_eq!(select.selected_value(), Some(&"c"));
    // Wraps around to the first item on further Down.
    select.handle_key(key(KeyCode::Down));
    assert_eq!(select.selected_value(), Some(&"a"));
    select.handle_key(key(KeyCode::Up));
    assert_eq!(select.selected_value(), Some(&"c"));
}

#[test]
fn select_enter_submits_current_value() {
    let mut select: Select<u8> = Select::new(vec![
        SelectOption {
            label: "1".into(),
            value: 1,
            hint: None,
        },
        SelectOption {
            label: "2".into(),
            value: 2,
            hint: None,
        },
    ]);
    select.handle_key(key(KeyCode::Down));
    let action = select.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        action,
        truyenazz_crawler::ui::SelectAction::Submit(2)
    ));
}

#[test]
fn select_esc_cancels() {
    let mut select: Select<&'static str> = Select::new(vec![SelectOption {
        label: "A".into(),
        value: "a",
        hint: None,
    }]);
    let action = select.handle_key(key(KeyCode::Esc));
    assert!(matches!(
        action,
        truyenazz_crawler::ui::SelectAction::Cancel
    ));
}

#[test]
fn select_ctrl_c_emits_quit() {
    let mut select: Select<&'static str> = Select::new(vec![SelectOption {
        label: "A".into(),
        value: "a",
        hint: None,
    }]);
    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let action = select.handle_key(event);
    assert!(matches!(action, truyenazz_crawler::ui::SelectAction::Quit));
}

#[test]
fn select_with_initial_value_starts_on_that_option() {
    let select: Select<&'static str> = Select::with_initial(
        vec![
            SelectOption {
                label: "A".into(),
                value: "a",
                hint: None,
            },
            SelectOption {
                label: "B".into(),
                value: "b",
                hint: None,
            },
            SelectOption {
                label: "C".into(),
                value: "c",
                hint: None,
            },
        ],
        &"b",
    );
    assert_eq!(select.selected_value(), Some(&"b"));
}

#[test]
fn download_progress_records_started_and_completed() {
    let mut progress = DownloadProgress::new(3);
    assert_eq!(progress.total, 3);
    assert_eq!(progress.completed, 0);
    progress.record_started(1);
    assert_eq!(progress.current_chapter, Some(1));
    progress.record_completed(1, CrawlStatus::Written);
    assert_eq!(progress.completed, 1);
    assert_eq!(progress.log.last(), Some(&DownloadLogEntry::Ok(1)));
    progress.record_completed(2, CrawlStatus::Skipped);
    assert_eq!(progress.completed, 2);
    assert_eq!(progress.log.last(), Some(&DownloadLogEntry::Skip(2)));
}

#[test]
fn download_progress_records_failures() {
    let mut progress = DownloadProgress::new(2);
    progress.record_started(1);
    progress.record_failed(1);
    assert_eq!(progress.failed, 1);
    assert_eq!(progress.log.last(), Some(&DownloadLogEntry::Fail(1)));
    // failed entries also count as "advanced" for percentage.
    assert_eq!(progress.advanced(), 1);
}

#[test]
fn download_progress_finish_marks_done() {
    let mut progress = DownloadProgress::new(1);
    progress.record_started(1);
    progress.record_completed(1, CrawlStatus::Written);
    assert!(!progress.done);
    progress.finish();
    assert!(progress.done);
}

#[test]
fn download_progress_log_caps_to_window() {
    let cap = 5;
    let mut progress = DownloadProgress::with_log_capacity(20, cap);
    for n in 1..=15u32 {
        progress.record_completed(n, CrawlStatus::Written);
    }
    assert_eq!(progress.log.len(), cap);
    // Most recent entries should be retained.
    assert_eq!(progress.log.last(), Some(&DownloadLogEntry::Ok(15)));
    assert_eq!(progress.log.first(), Some(&DownloadLogEntry::Ok(11)));
}

#[test]
fn download_progress_default_log_capacity_is_generous() {
    // Big enough that the activity log can fill a tall terminal without
    // dropping recent entries.
    let progress = DownloadProgress::new(0);
    assert!(
        progress.log_capacity >= 200,
        "default capacity too small: {}",
        progress.log_capacity
    );
}

#[test]
fn longest_common_prefix_handles_empty_and_single() {
    let empty: Vec<String> = vec![];
    assert_eq!(longest_common_prefix(&empty), "");
    assert_eq!(longest_common_prefix(&["only".to_string()]), "only");
}

#[test]
fn longest_common_prefix_returns_shared_start() {
    let inputs = vec!["foobar".to_string(), "foobaz".to_string(), "fooqux".to_string()];
    assert_eq!(longest_common_prefix(&inputs), "foo");
}

#[test]
fn longest_common_prefix_returns_empty_when_no_overlap() {
    let inputs = vec!["abc".to_string(), "xyz".to_string()];
    assert_eq!(longest_common_prefix(&inputs), "");
}

#[test]
fn path_completions_lists_children_matching_prefix() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("alpine.ttf"), b"a").unwrap();
    std::fs::write(dir.path().join("beta.txt"), b"b").unwrap();

    let prefix = format!("{}/al", dir.path().display());
    let mut completions = path_completions(&prefix);
    completions.sort();
    assert_eq!(completions.len(), 2);
    assert!(completions[0].ends_with("alpha.txt"));
    assert!(completions[1].ends_with("alpine.ttf"));
}

#[test]
fn path_completions_returns_directory_listing_for_trailing_slash() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("one.txt"), b"x").unwrap();
    std::fs::write(dir.path().join("two.txt"), b"x").unwrap();
    let prefix = format!("{}/", dir.path().display());
    let completions = path_completions(&prefix);
    assert_eq!(completions.len(), 2);
}

#[test]
fn path_input_tab_completes_to_common_prefix() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("alpine.ttf"), b"a").unwrap();
    let mut input = PathInput::new();
    let typed = format!("{}/al", dir.path().display());
    input.set_value(&typed);
    input.refresh_completions();
    let action = input.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(action, PathInputAction::Continue);
    // Should have advanced to the longest common prefix `<dir>/alp`.
    assert!(
        input.value().ends_with("alp"),
        "expected value ending in 'alp', got: {}",
        input.value()
    );
}

#[test]
fn path_input_down_key_navigates_suggestions() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("alpine.ttf"), b"a").unwrap();
    let mut input = PathInput::new();
    input.set_value(format!("{}/al", dir.path().display()));
    input.refresh_completions();
    assert!(input.suggestions().len() >= 2);
    assert_eq!(input.highlighted(), None);
    input.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(input.highlighted(), Some(0));
    input.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(input.highlighted(), Some(1));
}

#[test]
fn path_input_enter_on_highlighted_replaces_value() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.path().join("alpine.ttf"), b"a").unwrap();
    let mut input = PathInput::new();
    input.set_value(format!("{}/al", dir.path().display()));
    input.refresh_completions();
    input.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let action = input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(action, PathInputAction::Continue);
    assert!(input.highlighted().is_none(), "highlight clears after pick");
    let suggested_first_child_name = "alpha.txt";
    assert!(
        input.value().ends_with(suggested_first_child_name)
            || input.value().ends_with("alpine.ttf"),
        "value should be a full child path, got: {}",
        input.value()
    );
}

#[test]
fn path_input_enter_without_highlight_submits() {
    let mut input = PathInput::new();
    input.set_value("/tmp/foo");
    let action = input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(action, PathInputAction::Submit);
}

#[test]
fn path_input_esc_cancels() {
    let mut input = PathInput::new();
    let action = input.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(action, PathInputAction::Cancel);
}

#[test]
fn path_input_ctrl_c_emits_quit() {
    let mut input = PathInput::new();
    let event = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    let action = input.handle_key(event);
    assert_eq!(action, PathInputAction::Quit);
    assert_eq!(input.value(), "", "Ctrl+C must not insert 'c'");
}

#[test]
fn build_summary_includes_every_chosen_option_for_crawl_epub() {
    let chapters: Vec<u32> = (1..=50).collect();
    let output_root = std::path::PathBuf::from("/tmp/out");
    let font_path = std::path::PathBuf::from("/tmp/MyFont.ttf");
    let summary = build_summary(SummaryParams {
        base_url: "https://truyenazz.me/foo",
        mode: CrawlMode::CrawlEpub,
        output_root: output_root.as_path(),
        chapter_numbers: Some(chapters.as_slice()),
        delay: 0.5,
        workers: 4,
        if_exists: ExistingFilePolicy::Skip,
        chapter_dir: None,
        font_path: Some(font_path.as_path()),
        fast_skip: true,
    });
    assert!(summary.contains("Base URL: https://truyenazz.me/foo"));
    assert!(summary.contains("Mode: Crawl chapters and build EPUB"));
    assert!(summary.contains("Output root: /tmp/out"));
    assert!(summary.contains("Chapters: 1 -> 50 (50 total)"));
    assert!(summary.contains("Workers: 4"));
    assert!(summary.contains("Delay: 0.5s"));
    assert!(summary.contains("If chapter exists: skip"));
    assert!(
        summary.contains("Fast skip: yes"),
        "expected explicit fast-skip line, got:\n{}",
        summary
    );
    assert!(summary.contains("Build EPUB: yes"));
    assert!(summary.contains("/tmp/MyFont.ttf"));
}

#[test]
fn prompt_block_height_grows_with_message_lines() {
    // Borders eat 2 rows; we need at least 3 visible content rows so a
    // single short line is not visually cramped.
    assert!(prompt_block_height("") >= 5);
    assert!(prompt_block_height("one liner") >= 5);
    // 10-line plan summary needs 10 content rows + 2 borders.
    let ten = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
    assert!(
        prompt_block_height(ten) >= 12,
        "expected >= 12 rows, got {}",
        prompt_block_height(ten)
    );
}

#[test]
fn build_summary_marks_fast_skip_no_when_disabled() {
    let chapters: Vec<u32> = vec![1, 2];
    let summary = build_summary(SummaryParams {
        base_url: "https://x/",
        mode: CrawlMode::Crawl,
        output_root: std::path::Path::new("output"),
        chapter_numbers: Some(chapters.as_slice()),
        delay: 0.0,
        workers: 1,
        if_exists: ExistingFilePolicy::Ask,
        chapter_dir: None,
        font_path: None,
        fast_skip: false,
    });
    assert!(summary.contains("Fast skip: no"));
    assert!(summary.contains("Build EPUB: no"));
}
