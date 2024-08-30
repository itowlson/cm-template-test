use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, RwLock}};

use serde::Deserialize;

mod filters;

wasmtime::component::bindgen!({
    path: "../wit",
    trappable_imports: ["prompt", "confirm", "select"],
    with: {
        "fermyon:spin-template/ui/file": std::path::PathBuf,
        "fermyon:spin-template/types/execution-context": ExecutionContext,
    }
});

struct Host {
    content_root: PathBuf,
    files: wasmtime::component::ResourceTable,
    execution_contexts: wasmtime::component::ResourceTable,
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
            execution_contexts: wasmtime::component::ResourceTable::new(),
        }
    }
}

#[derive(Clone)]
pub struct ExecutionContext {
    inner: Arc<RwLock<ExecutionContextInner>>,
}

struct ExecutionContextInner {
    variables: HashMap<String, String>,
    parser: liquid::Parser,
}

impl ExecutionContext {
    fn new(variables: HashMap<String, String>, parser: liquid::Parser) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ExecutionContextInner::new(variables, parser))),
        }
    }

    fn evaluate_template(&self, template: &str) -> anyhow::Result<String> {
        self.inner.read().unwrap().evaluate_template(template)
    }
}

impl ExecutionContextInner {
    fn new(variables: HashMap<String, String>, parser: liquid::Parser) -> Self {
        Self {
            variables,
            parser,
        }
    }

    fn evaluate_template(&self, template: &str) -> anyhow::Result<String> {
        let template = self.parser.parse(template)?;
        
        let mut object = liquid::Object::new();

        for (name, value) in &self.variables {
            object.insert(
                name.to_owned().into(),
                liquid_core::Value::Scalar(value.to_owned().into()),
            );
        }
    
        let res = template.render(&object).unwrap();
        Ok(res)
    
    }
}

impl fermyon::spin_template::types::Host for Host {}  // y tho

impl fermyon::spin_template::types::HostExecutionContext for Host {
    fn set_variable(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::types::ExecutionContext>, name: String, value: String) {
        let context = self.execution_contexts.get_mut(&self_).unwrap();
        let mut inner = context.inner.write().unwrap();
        inner.variables.insert(name, value);
    }

    fn evaluate_template(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::types::ExecutionContext>, template: String) -> Result<String, fermyon::spin_template::types::Error> {
        let context = self.execution_contexts.get_mut(&self_).unwrap();
        let inner = context.inner.read().unwrap();
        inner.evaluate_template(&template).map_err(|e| fermyon::spin_template::types::Error::Other(e.to_string()))
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<fermyon::spin_template::types::ExecutionContext>) -> wasmtime::Result<()> {
        self.execution_contexts.delete(rep)?;
        Ok(())
    }
}

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
    fn execute(&self, action: &exports::fermyon::spin_template::template::Action) -> anyhow::Result<()>;
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
        };
        println!("{dryrun}");
        Ok(())
    }
}

struct Apply {
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
        }
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let manifest_path = PathBuf::from(std::env::args().nth(1).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
    let tpl_dir = manifest_path.parent().expect("shouldna passed the root dir");
    let content_dir = tpl_dir.join("content");

    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let file = tpl_dir.join(manifest.template);

    let parser_builder = liquid::ParserBuilder::with_stdlib()
        .filter(crate::filters::KebabCaseFilterParser)
        .filter(crate::filters::PascalCaseFilterParser)
        .filter(crate::filters::DottedPascalCaseFilterParser)
        .filter(crate::filters::SnakeCaseFilterParser)
        .filter(crate::filters::HttpWildcardFilterParser);

    for (_name, _filter_path) in &manifest.filter {
        // parser_builder.filter(something);
    }

    let parser = parser_builder.build()?;

    let initial_variables = [
        ("project-name", "merlin-the-happy-project"),
        ("authors", "merlin-the-happy-pig"),
    ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

    let execution_context = ExecutionContext::new(initial_variables, parser);

    let action_executor: Box<dyn ActionExecutor> = if std::env::args().any(|v| v == "--dry-run") {
        Box::new(DryRun)
    } else {
        let output_dir = PathBuf::from(std::env::args().nth(2).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
        Box::new(Apply { execution_context: execution_context.clone(), content_dir: content_dir.clone(), output_dir })
    };

    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).expect("shoulda engined");

    let component = wasmtime::component::Component::from_file(&engine, &file).expect("shoulda loaded a component");

    let mut linker = wasmtime::component::Linker::new(&engine);
    RunTemplate::add_to_linker(&mut linker, |state: &mut Host| state).expect("shoulda added to linker");

    let mut host = Host::new(&content_dir);
    let execution_context = host.execution_contexts.push(execution_context)?;

    let mut store = wasmtime::Store::new(&engine, host);

    let (bindings, _) = RunTemplate::instantiate(&mut store, &component, &linker).expect("should instantiated");

    let actions = match bindings.fermyon_spin_template_template().call_run(store, execution_context) {
        Ok(Ok(actions)) => actions,
        Ok(Err(exports::fermyon::spin_template::template::Error::Cancel)) => return Ok(()),
        Ok(Err(e)) => return Err(e.into()),
        Err(e) => return match e.downcast::<DialogueTrap>() {
                Ok(DialogueTrap::Cancel) => Ok(()),
                Ok(DialogueTrap::Error(e)) => Err(e.into()),
                Err(e) => return Err(e),
            }
    };

    for action in &actions {
        action_executor.execute(action)?;
    };

    println!("Done!");
    Ok(())
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
    #[serde(default)]
    filter: HashMap<String, PathBuf>,
}
