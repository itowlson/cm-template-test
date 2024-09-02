use std::{collections::HashMap, path::PathBuf, sync::{Arc, RwLock}};

use serde::Deserialize;

mod actions;
mod bindings;
mod custom_filter;
mod filters;
mod host;

use bindings::{fermyon, exports, RunTemplate};
use host::{DialogueTrap, ExecutionContext, Host};

fn main() -> anyhow::Result<()> {
    let manifest_path = PathBuf::from(std::env::args().nth(1).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
    let tpl_dir = manifest_path.parent().expect("shouldna passed the root dir");
    let content_dir = tpl_dir.join("content");
    let is_dry_run = std::env::args().any(|v| v == "--dry-run");

    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let file = tpl_dir.join(manifest.template);

    let mut parser_builder = liquid::ParserBuilder::with_stdlib()
        .filter(crate::filters::KebabCaseFilterParser)
        .filter(crate::filters::PascalCaseFilterParser)
        .filter(crate::filters::DottedPascalCaseFilterParser)
        .filter(crate::filters::SnakeCaseFilterParser)
        .filter(crate::filters::HttpWildcardFilterParser);

    for (name, filter_path) in &manifest.filter {
        let wasm_path = tpl_dir.join("filters").join(filter_path);
        parser_builder = parser_builder.filter(custom_filter::CustomFilterParser::load(name, &wasm_path)?);
    }

    let parser = parser_builder.build()?;

    let initial_variables = [
        ("project-name", "merlin-the-happy-project"),
        ("authors", "merlin-the-happy-pig"),
    ].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();

    let execution_context = ExecutionContext::new(initial_variables, parser);

    let mut config = wasmtime::Config::new();
    config.wasm_component_model(true);
    let engine = wasmtime::Engine::new(&config).expect("shoulda engined");

    let component = wasmtime::component::Component::from_file(&engine, &file).expect("shoulda loaded a component");

    let mut linker = wasmtime::component::Linker::new(&engine);
    RunTemplate::add_to_linker(&mut linker, |state: &mut Host| state).expect("shoulda added to linker");

    let mut host = Host::new(&content_dir);
    let execution_context_rsrc = host.execution_contexts.push(execution_context.clone())?;

    let store = wasmtime::Store::new(&engine, host);
    let store = Arc::new(RwLock::new(store));

    let options = fermyon::spin_template::types::RunOptions {
        mode: fermyon::spin_template::types::CreateMode::CreateNew,
        use_default_values: false,
    };

    let (bindings, _instance, actions) = {
        // scope the unlock of the store
        use std::ops::DerefMut;
        let mut store = store.write().unwrap();
        let (bindings, _instance) = RunTemplate::instantiate(store.deref_mut(), &component, &linker).expect("should instantiated");
        let actions = bindings.fermyon_spin_template_template().call_run(store.deref_mut(), execution_context_rsrc, &options);
        (bindings, _instance, actions)
    };

    let action_executor = if is_dry_run {
        actions::dry_run()
    } else {
        let output_dir = PathBuf::from(std::env::args().nth(2).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
        actions::apply(&store, bindings, &execution_context, &content_dir, &output_dir)
    };

    let actions = match actions {
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

#[derive(Deserialize)]
struct Manifest {
    template: String,
    #[serde(default)]
    filter: HashMap<String, PathBuf>,
}
