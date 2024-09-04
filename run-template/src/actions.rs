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
    execution_context_rsrc: u32, // like the animal that I am
    content_dir: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
    edit_dir_base: impl AsRef<Path>,
) -> Box<dyn ActionExecutor> {
    Box::new(Apply {
        store: store.clone(),
        guest,
        execution_context: execution_context.clone(),
        execution_context_rsrc,
        content_dir: content_dir.as_ref().to_owned(),
        output_dir: output_dir.as_ref().to_owned(),
        edit_dir_base: edit_dir_base.as_ref().to_owned(),
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
            exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => format!("Write '{content}' to {path}"),
            exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => format!("Write {} bytes to {path}", content.len()),
            exports::fermyon::spin_template::template::Action::CreateDir(path) => format!("Create empty directory {path}"),
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
    edit_dir_base: PathBuf,
    execution_context: ExecutionContext,
    execution_context_rsrc: u32,
}

impl ActionExecutor for Apply {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action) -> anyhow::Result<()> {
        // TODO: Ideally we would eval all this stuff before writing any files - like
        // verify overwrites, Liquid expansion, etc.  Like eval it to the point of
        // "write these buffers to these locations, which either do not exist or we are
        // allowed to write to", so the only thing that can fail at this point is the
        // file writes.  This creates a mild faff for the copy operation, though, and
        // would need delicate handling for multiple edits to the same file.

        // TODO: a buttload of validation. Paths must be relative and must resolve to within
        // the output tree.  Ironically having the guest use WASI filesystem operations
        // would do this for us!
        match action {
            exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => {
                let out_file = self.output_dir.join(&path);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&path))?;
                let new_text = self.execution_context.evaluate_template(&tpl)?;
                std::fs::write(&out_file, &new_text)?;
            }
            exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => {
                let out_file = self.output_dir.join(&to);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&from))?;
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
            exports::fermyon::spin_template::template::Action::CreateDir(path) => {
                let out_dir = self.output_dir.join(&path);
                std::fs::create_dir_all(out_dir)?;
            }
            exports::fermyon::spin_template::template::Action::EditFile((path, edit)) => {
                use std::ops::DerefMut;
                //println!("***EDIT PATH FROM TPL {path:?}");
                let mut store = self.store.write().unwrap();
                let store = store.deref_mut();
                let guest = self.guest.fermyon_spin_template_template();
                let ec_rsrc = wasmtime::component::Resource::new_borrow(self.execution_context_rsrc);

                let edit_file = self.edit_dir_base.join(&path);
                //println!("***EDB {:?}, ABS EDIT FILE {edit_file:?}", self.edit_dir_base);
                let edit_result = apply_edit(edit_file, ec_rsrc, edit, guest, store);
                _ = edit.resource_drop(store);
                edit_result?;
            }
        }
        Ok(())
    }
}

// Extracts implementation of edit callback so that we can make sure to dispose the ResourceAny without
// having a surfeit of failure paths.
fn apply_edit(edit_file: impl AsRef<Path>, context: wasmtime::component::Resource<ExecutionContext>, edit: &wasmtime::component::ResourceAny, guest: &exports::fermyon::spin_template::template::Guest, store: &mut wasmtime::Store<Host>) -> anyhow::Result<()> {
    let edit_file = edit_file.as_ref();
    //println!("***APPLYING EDIT TO {edit_file:?}");
    let edit_content = std::fs::read_to_string(&edit_file).unwrap_or_default();
    let edit_result = match guest.edit().call_apply(store, *edit, &edit_content, context) {
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
