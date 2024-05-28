use std::path::{Path, PathBuf};

use serde::Deserialize;

wasmtime::component::bindgen!({
    path: "../wit",
    with: {
        "fermyon:spin-template/ui/file": std::path::PathBuf,
    }
});

struct Host {
    content_root: PathBuf,
    files: wasmtime::component::ResourceTable,
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
    fn prompt(&mut self, prompt: String) -> String {
        match dialoguer::Input::new().with_prompt(&prompt).interact_text() {
            Ok(res) => res,
            Err(e) => exit_with_error(e),
        }
    }
    
    fn confirm(&mut self, prompt: String) -> bool {
        match dialoguer::Confirm::new().with_prompt(&prompt).interact_opt() {
            Ok(Some(res)) => res,
            Ok(None) => cancel(),
            Err(e) => exit_with_error(e),
        }
    }

    fn select(&mut self, prompt: String, items: Vec<String>) -> u8 {
        match dialoguer::Select::new().with_prompt(&prompt).items(&items).interact_opt() {
            Ok(Some(res)) => res.try_into().unwrap_or_else(|_| cancel()),
            Ok(None) => cancel(),
            Err(e) => exit_with_error(e),
        }
    }
    
    fn substitute_text(&mut self, text: String, substitutions: Vec<fermyon::spin_template::types::Substitution>) -> Result<String, fermyon::spin_template::types::Error> {
        Ok(substitute_text(&text, &substitutions).unwrap())
        // let parser = liquid::Parser::new();
        // let template = parser.parse(&text).unwrap();

        // let mut object = liquid::Object::new();

        // for sub in substitutions {
        //     object.insert(
        //         sub.key.into(),
        //         liquid_core::Value::Scalar(sub.value.into()),
        //     );
        // }

        // let res = template.render(&object).unwrap();
        // Ok(res)
    }
}

fn substitute_text(text: &str, substitutions: &[fermyon::spin_template::types::Substitution]) -> Result<String, std::convert::Infallible> {
    let parser = liquid::Parser::new();
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

fn main() {
    let manifest_path = PathBuf::from(std::env::args().nth(1).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
    let dry_run = std::env::args().any(|v| v == "--dry-run");
    let output_dir = PathBuf::from(std::env::args().nth(2).expect("Usage: run-template <FILE> {--dry-run | <OUT_DIR> }"));
    let tpl_dir = manifest_path.parent().expect("shouldna passed the root dir");
    let content_dir = tpl_dir.join("content");

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

    let exec = bindings.fermyon_spin_template_template().call_run(&mut store).expect("should called run").expect("and it shouldn't have failed");

    if dry_run {
        for a in exec.actions {
            let dryrun = match a {
                exports::fermyon::spin_template::template::Action::CopyFileSubstituted((from, to)) => format!("Copy file {from} to {to}"),
                exports::fermyon::spin_template::template::Action::CopyFileRaw((from, to)) => format!("Copy raw file {from} to {to}"),
                exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => format!("Write '{content}' bytes to {path}"),
                exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => format!("Write {} bytes to {path}", content.len()),
            };
            println!("{dryrun}");
        }
    } else {
        std::fs::create_dir_all(&output_dir).unwrap();
        for a in exec.actions {
            match a {
                exports::fermyon::spin_template::template::Action::CopyFileSubstituted((from, to)) => {
                    let out_file = output_dir.join(&to);
                    if let Some(d) = out_file.parent() {
                        std::fs::create_dir_all(d).unwrap();
                    }
                    println!("resolving tpl {}", content_dir.join(&from).display());
                    let tpl = std::fs::read_to_string(content_dir.join(&from)).unwrap();
                    let new_text = substitute_text(&tpl, &exec.substitutions).unwrap();
                    println!("writin {}", out_file.display());
                    std::fs::write(&out_file, &new_text).unwrap();
                }
                exports::fermyon::spin_template::template::Action::CopyFileRaw((from, to)) => {
                    let out_file = output_dir.join(&to);
                    if let Some(d) = out_file.parent() {
                        std::fs::create_dir_all(d).unwrap();
                    }
                    std::fs::copy(content_dir.join(&from), &out_file).unwrap();
                }
                exports::fermyon::spin_template::template::Action::WriteFile((path, content)) => {
                    let out_file = output_dir.join(&path);
                    if let Some(d) = out_file.parent() {
                        std::fs::create_dir_all(d).unwrap();
                    }
                    std::fs::write(&out_file, &content).unwrap();
                }
                exports::fermyon::spin_template::template::Action::WriteFileBinary((path, content)) => {
                    let out_file = output_dir.join(&path);
                    if let Some(d) = out_file.parent() {
                        std::fs::create_dir_all(d).unwrap();
                    }
                    std::fs::write(&out_file, &content).unwrap();
                }
            }
        }
    }

    println!("Done!");
}

fn exit_with_error(e: dialoguer::Error) -> ! {
    eprintln!("Error: {e}");
    std::process::exit(1);
}

fn cancel() -> ! {
    std::process::exit(0);
}

#[derive(Deserialize)]
struct Manifest {
    template: String,
}
