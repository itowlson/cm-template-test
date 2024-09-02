use std::{path::{Path, PathBuf}, sync::{Arc, RwLock}};

use crate::bindings::{fermyon, exports, RunTemplate};
use crate::host::{ExecutionContext, Host};

pub trait ActionExecutor {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action) -> anyhow::Result<()>;
}

pub fn dry_run() -> Box<dyn ActionExecutor> {
    Box::new(DryRun)
}

pub fn apply(
    store: &Arc<RwLock<wasmtime::Store<Host>>>,
    guest: RunTemplate,
    execution_context: &ExecutionContext,
    content_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Box<dyn ActionExecutor> {
    Box::new(Apply {
        store: store.clone(),
        guest,
        execution_context: execution_context.clone(),
        content_dir: content_dir.as_ref().to_owned(),
        output_dir: output_dir.as_ref().to_owned(),
    })
}

struct DryRun;

impl ActionExecutor for DryRun {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action) -> anyhow::Result<()> {
        // TODO: this should probably at least eval substitutions
        let dryrun = match action {
            exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => format!("Copy {path}"),
            exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => format!("Copy file {from} to {to}"),
            exports::fermyon::spin_template::template::Action::CopyFileToRaw((from, to)) => format!("Copy raw file {from} to {to}"),
            exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => format!("Write '{content}' bytes to {path}"),
            exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => format!("Write {} bytes to {path}", content.len()),
            exports::fermyon::spin_template::template::Action::EditFile((path, _edit)) => format!("Edit {path}"),
        };
        println!("{dryrun}");
        Ok(())
    }
}

struct Apply {
    store: Arc<RwLock<wasmtime::Store<Host>>>,  // we're going to need a mutable ref via an immutable self
    guest: RunTemplate,
    content_dir: PathBuf,
    output_dir: PathBuf,
    execution_context: ExecutionContext,
}

impl ActionExecutor for Apply {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action) -> anyhow::Result<()> {
        match action {
            exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => {
                let out_file = self.output_dir.join(&path);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&path))?;
                // let new_text = substitute_text(&tpl, &exec.substitutions)?;
                let new_text = self.execution_context.evaluate_template(&tpl)?;
                std::fs::write(&out_file, &new_text)?;
            }
            exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => {
                let out_file = self.output_dir.join(&to);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&from))?;
                // let new_text = substitute_text(&tpl, &exec.substitutions)?;
                let new_text = self.execution_context.evaluate_template(&tpl)?;
                std::fs::write(&out_file, &new_text)?;
            }
            exports::fermyon::spin_template::template::Action::CopyFileToRaw((from, to)) => {
                let out_file = self.output_dir.join(&to);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::copy(self.content_dir.join(&from), &out_file)?;
            }
            exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => {
                let out_file = self.output_dir.join(&path);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(&out_file, &content)?;
            }
            exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => {
                let out_file = self.output_dir.join(&path);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(&out_file, &content)?;
            }
            exports::fermyon::spin_template::template::Action::EditFile((path, edit)) => {
                use std::ops::DerefMut;
                let mut store = self.store.write().unwrap();
                let store = store.deref_mut();
                let guest = self.guest.fermyon_spin_template_template();

                let edit_file = self.output_dir.join(&path);
                let edit_result = apply_edit(edit_file, edit, guest, store);
                _ = edit.resource_drop(store);
                edit_result?;
            }
        }
        Ok(())
    }
}

// Extracts implementation of edit callback so that we can make sure to dispose the ResourceAny without
// having a surfeit of failure paths.
fn apply_edit(edit_file: impl AsRef<Path>, edit: &wasmtime::component::ResourceAny, guest: &exports::fermyon::spin_template::template::Guest, store: &mut wasmtime::Store<Host>) -> anyhow::Result<()> {
    let edit_file = edit_file.as_ref();
    let edit_content = std::fs::read_to_string(&edit_file).unwrap_or_default();
    let edit_result = match guest.edit().call_apply(store, *edit, &edit_content) {
        Ok(Ok(r)) => r,
        Ok(Err(fermyon::spin_template::types::Error::Cancel)) => return Ok(()),
        Ok(Err(e)) => anyhow::bail!("Inner err! {e:#}"),
        Err(e) => anyhow::bail!("Outer err! {e:#}"),
    };
    if edit_result != edit_content {
        if let Some(d) = edit_file.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&edit_file, edit_result)?;
    }
    Ok(())
}
