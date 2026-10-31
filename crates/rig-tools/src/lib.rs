//! Built-in tools. Names are snake_case verbs; every path parameter is called `path`.

mod ask;
mod context_tools;
mod exec;
mod fetch;
mod glob;
mod goal;
mod grep;
pub mod index;
mod ls;
mod patch;
mod read;
mod todo;
mod util;

use rig_core::sync::MutexExt;
use rig_core::tool::{Registry, Shared, Tool};

pub fn builtin() -> Registry {
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(read::ReadFile),
        Box::new(ls::ListDir),
        Box::new(glob::FindFiles),
        Box::new(grep::SearchText),
        Box::new(exec::RunCommand),
        Box::new(patch::EditFiles),
        Box::new(todo::UpdateTasks::default()),
        Box::new(fetch::FetchPage),
        Box::new(context_tools::UseSkill),
        Box::new(context_tools::Recall),
        Box::new(ask::AskUser),
        Box::new(goal::UpdateGoal),
    ];
    Registry::new(tools)
}

/// Restore the files changed by the most recent edit. `None` when there is nothing to undo.
pub fn undo_last(shared: &Shared) -> Option<String> {
    let batch = shared.undo.locked().pop()?;
    let mut lines = Vec::new();
    for (path, before) in &batch.files {
        let result = match before {
            Some(bytes) => std::fs::write(path, bytes).map(|_| "restored"),
            None => std::fs::remove_file(path).map(|_| "removed"),
        };
        match result {
            Ok(what) => lines.push(format!("{what} {}", path.display())),
            Err(e) => lines.push(format!("could not undo {}: {e}", path.display())),
        }
    }
    Some(format!(
        "Undid the last edit ({} file(s)):\n{}",
        batch.files.len(),
        lines.join("\n")
    ))
}
