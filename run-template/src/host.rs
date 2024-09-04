use std::{collections::HashMap, path::{Path, PathBuf}, sync::{Arc, RwLock}};

use crate::bindings::fermyon;

pub struct Host {
    content_root: PathBuf,
    files: wasmtime::component::ResourceTable,
    pub(crate) execution_contexts: wasmtime::component::ResourceTable,
}

#[derive(Debug)]
pub enum DialogueTrap {
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
    pub fn new(root_dir: impl AsRef<Path>) -> Self {
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
    pub fn new(variables: HashMap<String, String>, parser: liquid::Parser) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ExecutionContextInner::new(variables, parser))),
        }
    }

    pub fn evaluate_template(&self, template: &str) -> anyhow::Result<String> {
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
    fn prompt(&mut self, prompt: String, default_value: Option<String>) -> Result<String, wasmtime::Error> {
        let mut input = dialoguer::Input::new().with_prompt(&prompt).allow_empty(true); // if template doesn't want to allow empty it can circle back
        if let Some(default_value) = default_value {
            input = input.default(default_value);
        }
        match input.interact_text() {
            Ok(res) => Ok(res),
            Err(e) => exit_with_error(e),
        }
    }
    
    fn confirm(&mut self, prompt: String, default_value: Option<bool>) -> Result<bool, wasmtime::Error> {
        let mut confirm = dialoguer::Confirm::new().with_prompt(&prompt);
        if let Some(default_value) = default_value {
            confirm = confirm.default(default_value);
        }
        match confirm.interact_opt() {
            Ok(Some(res)) => Ok(res),
            Ok(None) => cancel(),
            Err(e) => exit_with_error(e),
        }
    }

    fn select(&mut self, prompt: String, items: Vec<String>, default_index: Option<u8>) -> Result<u8, wasmtime::Error> {
        let mut select = dialoguer::Select::new().with_prompt(&prompt).items(&items);
        if let Some(default_index) = default_index {
            select = select.default(default_index.into());
        }
        match select.interact_opt() {
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
        let path = self.content_root.join(res);
        //println!("***READING {path:?}");
        std::fs::read_to_string(&path).map_err(|e| fermyon::spin_template::types::Error::Other(format!("Error reading file {res:?}: {e:?}")))
    }

    fn read_binary(&mut self, self_: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> Result<Vec<u8>, fermyon::spin_template::types::Error> {
        let res = self.files.get(&self_).unwrap();
        let path = self.content_root.join(res);
        std::fs::read(&path).map_err(|e| fermyon::spin_template::types::Error::Other(format!("Error reading file {res:?}: {e:?}")))
    }

    fn drop(&mut self, rep: wasmtime::component::Resource<fermyon::spin_template::ui::File>) -> wasmtime::Result<()> {
        self.files.delete(rep)?;
        Ok(())
    }
}

fn cancel<T>() -> Result<T, wasmtime::Error> {
    Err(wasmtime::Error::new(DialogueTrap::Cancel))    
}

fn exit_with_error<T>(e: dialoguer::Error) -> Result<T, wasmtime::Error> {
    Err(wasmtime::Error::new(DialogueTrap::Error(e)))    
}
