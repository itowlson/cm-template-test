use std::path::{Path, PathBuf};

use serde::Deserialize;

mod filters;

wasmtime::component::bindgen!({
    path: "../wit",
    trappable_imports: ["prompt", "confirm", "select"],
    with: {
        "fermyon:spin-template/ui/file": std::path::PathBuf,
    }
});

struct Host {
    content_root: PathBuf,
    files: wasmtime::component::ResourceTable,
}

#[derive(Debug)]
enum DialogueTrap {
    Cancel,
    Error(dialoguer::Error),
}

impl std::error::Error for DialogueTrap {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cancel => None,
            Self::Error(e) => Some(e),
        }
    }

    fn description(&self) -> &str {
        "trap"
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        self.source()
    }
}

impl std::fmt::Display for DialogueTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancel => f.write_str("cancelled"),
            Self::Error(e) => f.write_str(&e.to_string()),
        }
    }
}

impl Host {
    fn new(root_dir: impl AsRef<Path>) -> Self {
        Self {
            content_root: root_dir.as_ref().to_owned(),
            files: wasmtime::component::ResourceTable::new(),
        }
    }
}

impl fermyon::spin_template::types::Host for Host {}  // y tho

impl fermyon::spin_template::ui::Host for Host {
    fn prompt(&mut self, prompt: String) -> Result<String, wasmtime::Error> {
        match dialoguer::Input::new().with_prompt(&prompt).interact_text() {
            Ok(res) => Ok(res),
            Err(e) => exit_with_error(e),
        }
    }
    
    fn confirm(&mut self, prompt: String) -> Result<bool, wasmtime::Error> {
        match dialoguer::Confirm::new().with_prompt(&prompt).interact_opt() {
            Ok(Some(res)) => Ok(res),
            Ok(None) => cancel(),
            Err(e) => exit_with_error(e),
        }
    }

    fn select(&mut self, prompt: String, items: Vec<String>) -> Result<u8, wasmtime::Error> {
        match dialoguer::Select::new().with_prompt(&prompt).items(&items).interact_opt() {
            Ok(Some(res)) => res.try_into().or_else(|_| cancel()),
            Ok(None) => cancel(),
            Err(e) => exit_with_error(e),
        }
    }
    
    fn substitute_text(&mut self, text: String, substitutions: Vec<fermyon::spin_template::types::Substitution>) -> Result<String, fermyon::spin_template::types::Error> {
        Ok(substitute_text(&text, &substitutions).unwrap())
    }
}

fn substitute_text(text: &str, substitutions: &[fermyon::spin_template::types::Substitution]) -> Result<String, std::convert::Infallible> {
    let parser = liquid::ParserBuilder::with_stdlib()
        .filter(crate::filters::KebabCaseFilterParser)
        .filter(crate::filters::PascalCaseFilterParser)
        .filter(crate::filters::DottedPascalCaseFilterParser)
        .filter(crate::filters::SnakeCaseFilterParser)
        .filter(crate::filters::HttpWildcardFilterParser)
        .build()
        .unwrap();

    let template = parser.parse(text).unwrap();

    let mut object = liquid::Object::new();

    for sub in substitutions {
        object.insert(
            sub.key.to_owned().into(),
            liquid_core::Value::Scalar(sub.value.to_owned().into()),
        );
    }

    let res = template.render(&object).unwrap();
    Ok(res)

}

impl fermyon::spin_template::ui::HostFile for Host {
    fn list_all(&mut self) -> Vec<wasmtime::component::Resource<fermyon::spin_template::ui::File>> {
        let w = walkdir::WalkDir::new(&self.content_root);
        w.into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().strip_prefix(&self.content_root).unwrap().to_owned())
            .map(|p| {
                // TODO: I am aware of all the crimes
                self.files.push(p).unwrap()
            })
            .collect()
    }

    fn path(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> String {
        let res = self.files.get(&self_).unwrap();
        res.display().to_string()
    }

    fn read(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> Result<String, fermyon::spin_template::types::Error> {
        let res = self.files.get(&self_).unwrap();
        Ok(std::fs::read_to_string(&res).unwrap())
    }

    fn read_binary(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> Result<Vec<u8>, fermyon::spin_template::types::Error> {
        let res = self.files.get(&self_).unwrap();
        Ok(std::fs::read(&res).unwrap())
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> wasmtime::Result<()> {
        self.files.delete(rep)?;
        Ok(())
    }
}

trait ActionExecutor {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action, exec: &exports::fermyon::spin_template::template::Execute) -> anyhow::Result<()>;
}

struct DryRun;

impl ActionExecutor for DryRun {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action, _exec: &exports::fermyon::spin_template::template::Execute) -> anyhow::Result<()> {
        // TODO: this should probably at least eval substitutions
        let dryrun = match action {
            exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => format!("Copy {path}"),
            exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => format!("Copy file {from} to {to}"),
            exports::fermyon::spin_template::template::Action::CopyFileToRaw((from, to)) => format!("Copy raw file {from} to {to}"),
            exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => format!("Write '{content}' bytes to {path}"),
            exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => format!("Write {} bytes to {path}", content.len()),
        };
        println!("{dryrun}");
        Ok(())
    }
}

struct Apply {
    content_dir: PathBuf,
    output_dir: PathBuf,
}

impl ActionExecutor for Apply {
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action, exec: &exports::fermyon::spin_template::template::Execute) -> anyhow::Result<()> {
        match action {
            exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => {
                let out_file = self.output_dir.join(&path);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&path))?;
                let new_text = substitute_text(&tpl, &exec.substitutions)?;
                std::fs::write(&out_file, &new_text)?;
            }
            exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => {
                let out_file = self.output_dir.join(&to);
                if let Some(d) = out_file.parent() {
                    std::fs::create_dir_all(d)?;
                }
                let tpl = std::fs::read_to_string(self.content_dir.join(&from))?;
                let new_text = substitute_text(&tpl, &exec.substitutions)?;
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
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let manifest_path = PathBuf::from(std::env::args().nth(1).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
    let tpl_dir = manifest_path.parent().expect("shouldna passed the root dir");
    let content_dir = tpl_dir.join("content");

    let action_executor: Box<dyn ActionExecutor> = if std::env::args().any(|v| v == "--dry-run") {
        Box::new(DryRun)
    } else {
        let output_dir = PathBuf::from(std::env::args().nth(2).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
        Box::new(Apply { content_dir: content_dir.clone(), output_dir })
    };

    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let file = tpl_dir.join(manifest.template);

    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).expect("shoulda engined");

    let component = wasmtime::component::Component::from_file(&engine, &file).expect("shoulda loaded a component");

    let mut linker = wasmtime::component::Linker::new(&engine);
    RunTemplate::add_to_linker(&mut linker, |state: &mut Host| state).expect("shoulda added to linker");

    let mut store = wasmtime::Store::new(&engine, Host::new(&content_dir));

    let (bindings, _) = RunTemplate::instantiate(&mut store, &component, &linker).expect("should instantiated");

    let mut exec = match bindings.fermyon_spin_template_template().call_run(&mut store) {
        Ok(Ok(exec)) => exec,
        Ok(Err(exports::fermyon::spin_template::template::Error::Cancel)) => return Ok(()),
        Ok(Err(e)) => return Err(e.into()),
        Err(e) => return match e.downcast::<DialogueTrap>() {
                Ok(DialogueTrap::Cancel) => Ok(()),
                Ok(DialogueTrap::Error(e)) => Err(e.into()),
                Err(e) => return Err(e),
            }
    };

    augment_substitutions(&mut exec.substitutions);

    for action in &exec.actions {
        action_executor.execute(action, &exec)?;
    };

    // if dry_run {
    //     for a in exec.actions {
    //         let dryrun = match a {
    //             exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => format!("Copy {path}"),
    //             exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => format!("Copy file {from} to {to}"),
    //             exports::fermyon::spin_template::template::Action::CopyFileToRaw((from, to)) => format!("Copy raw file {from} to {to}"),
    //             exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => format!("Write '{content}' bytes to {path}"),
    //             exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => format!("Write {} bytes to {path}", content.len()),
    //         };
    //         println!("{dryrun}");
    //     }
    // } else {
    //     for a in exec.actions {
    //         match a {
    //             exports::fermyon::spin_template::template::Action::CopyFileSubstituted(path) => {
    //                 let out_file = output_dir.join(&path);
    //                 if let Some(d) = out_file.parent() {
    //                     std::fs::create_dir_all(d).unwrap();
    //                 }
    //                 let tpl = std::fs::read_to_string(content_dir.join(&path)).unwrap();
    //                 let new_text = substitute_text(&tpl, &exec.substitutions).unwrap();
    //                 std::fs::write(&out_file, &new_text).unwrap();
    //             }
    //             exports::fermyon::spin_template::template::Action::CopyFileToSubstituted((from, to)) => {
    //                 let out_file = output_dir.join(&to);
    //                 if let Some(d) = out_file.parent() {
    //                     std::fs::create_dir_all(d).unwrap();
    //                 }
    //                 let tpl = std::fs::read_to_string(content_dir.join(&from)).unwrap();
    //                 let new_text = substitute_text(&tpl, &exec.substitutions).unwrap();
    //                 std::fs::write(&out_file, &new_text).unwrap();
    //             }
    //             exports::fermyon::spin_template::template::Action::CopyFileToRaw((from, to)) => {
    //                 let out_file = output_dir.join(&to);
    //                 if let Some(d) = out_file.parent() {
    //                     std::fs::create_dir_all(d).unwrap();
    //                 }
    //                 std::fs::copy(content_dir.join(&from), &out_file).unwrap();
    //             }
    //             exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => {
    //                 let out_file = output_dir.join(&path);
    //                 if let Some(d) = out_file.parent() {
    //                     std::fs::create_dir_all(d).unwrap();
    //                 }
    //                 std::fs::write(&out_file, &content).unwrap();
    //             }
    //             exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => {
    //                 let out_file = output_dir.join(&path);
    //                 if let Some(d) = out_file.parent() {
    //                     std::fs::create_dir_all(d).unwrap();
    //                 }
    //                 std::fs::write(&out_file, &content).unwrap();
    //             }
    //         }
    //     }
    // }

    println!("Done!");
    Ok(())
}

// TODO: maybe substitutions should be a resource already initialised with this stuff
fn augment_substitutions(substitutions: &mut Vec<fermyon::spin_template::types::Substitution>) {
    try_augment_substitutions(substitutions, "project-name", "merlin-the-happy-project");
    try_augment_substitutions(substitutions, "authors", "merlin-the-happy-pig");
}

fn try_augment_substitutions(substitutions: &mut Vec<fermyon::spin_template::types::Substitution>, key: &str, value: &str) {
    if substitutions.iter().any(|s| s.key == key) {
        return;
    }

    substitutions.push(fermyon::spin_template::types::Substitution { key: key.to_string(), value: value.to_string() });
}

fn cancel<T>() -> Result<T, wasmtime::Error> {
    Err(wasmtime::Error::new(DialogueTrap::Cancel))    
}

fn exit_with_error<T>(e: dialoguer::Error) -> Result<T, wasmtime::Error> {
    Err(wasmtime::Error::new(DialogueTrap::Error(e)))    
}

#[derive(Deserialize)]
struct Manifest {
    template: String,
}
