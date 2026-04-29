use anyhow::Result;

use crate::cli::CliOptions;
use crate::ui::plan::InteractivePlan;

mod state;
mod steps;

use state::{StepResult, WizardState, WizardStep};

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

/// Step-machine driver. Dispatches to the per-step renderer/prompter, then
/// returns where to go next based on the user's decision.
async fn advance_step(step: WizardStep, state: &mut WizardState) -> Result<StepResult> {
    use WizardStep::*;
    match step {
        Welcome => steps::step_welcome(state),
        BaseUrl => steps::step_base_url(state),
        Mode => steps::step_mode(state),
        OutputRoot => steps::step_output_root(state),
        Discover => steps::step_discover(state).await,
        StartChapter => steps::step_start_chapter(state),
        EndChapter => steps::step_end_chapter(state),
        Workers => steps::step_workers(state),
        Delay => steps::step_delay(state),
        IfExists => steps::step_if_exists(state),
        ChapterDir => steps::step_chapter_dir(state),
        FastSkip => steps::step_fast_skip(state),
        FontChoice => steps::step_font_choice(state),
        FontPath => steps::step_font_path(state),
        Confirm => steps::step_confirm(state),
    }
}
