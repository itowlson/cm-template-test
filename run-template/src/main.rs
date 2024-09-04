use std::{collections::HashMap, path::PathBuf, sync::{Arc, RwLock}};

use clap::Parser;
use serde::Deserialize;

mod actions;
mod bindings;
mod custom_filter;
mod filters;
mod host;

use bindings::{fermyon, exports, RunTemplate};
use host::{DialogueTrap, ExecutionContext, Host};

#[derive(clap::Parser)]
struct Args {
    /// The template file e.g. ../sample-template/template/spin-template.toml
    template_manifest: PathBuf,

    /// The name for the item being generated. This will also be used as the
    /// directory to generate into (for new apps relative to current directory,
    /// for additions relative to the directory containing `spin.toml`).
    name: String,

    /// The spin.toml file to add the component to.
    #[clap(long = "add-to")]
    add_to: Option<PathBuf>,

    /// Print what would be done but don't do it.
    #[clap(long = "dry-run")]
    dry_run: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let tpl_dir = args.template_manifest.parent().expect("shouldna passed the root dir");
    let content_dir = tpl_dir.join("content");

    let manifest: Manifest = toml::from_str(&std::fs::read_to_string(&args.template_manifest).unwrap()).unwrap();
    let file = tpl_dir.join(manifest.template);

    let name = safeify(&args.name);

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
        ("project-name", name.as_str()),
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
    let execution_context_rsrc_rep = execution_context_rsrc.rep();

    let store = wasmtime::Store::new(&engine, host);
    let store = Arc::new(RwLock::new(store));

    let mode = match args.add_to.as_ref() {
        Some(manifest) => fermyon::spin_template::types::CreateMode::AddTo(manifest.file_name().expect("shoulda had a file name").to_string_lossy().to_string()),
        None => fermyon::spin_template::types::CreateMode::CreateNew,
    };
    let options = fermyon::spin_template::types::RunOptions {
        mode,
        use_default_values: false,
    };

    let (bindings, _instance, actions) = {
        // scope the unlock of the store
        use std::ops::DerefMut;
        let mut store = store.write().unwrap();
        let (bindings, _instance) = RunTemplate::instantiate(store.deref_mut(), &component, &linker).expect("shoulda instantiated");
        let actions = bindings.fermyon_spin_template_template().call_run(store.deref_mut(), execution_context_rsrc, &options);
        (bindings, _instance, actions)
    };

    let action_executor = if args.dry_run {
        actions::dry_run()
    } else {
        let output_dir = PathBuf::from(&name);
        let existing_app_dir = args.add_to.map(|f| f.parent().expect("stop passing the root directory, I have warned you before").to_owned());
        let (output_dir, edit_dir_base) = match &existing_app_dir {
            None => (output_dir.clone(), output_dir),
            Some(ead) => (ead.join(output_dir), ead.to_owned()),
        };
        // println!("***EXISTING APP DIR {existing_app_dir:?}, OUTPUT DIR {output_dir:?}, EDIT DIR {edit_dir_base:?}");
        actions::apply(&store, bindings, &execution_context, execution_context_rsrc_rep, &content_dir, &output_dir, &edit_dir_base)
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

fn safeify(text: &str) -> String {
    let unsafe_chars = regex::Regex::new("[^-_.a-zA-Z0-9]").expect("invalid safety regex");
    let s = unsafe_chars.replace_all(text, "-");
    s.to_string()
}
