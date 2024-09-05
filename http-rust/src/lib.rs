#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template::template::Guest;

use bindings::exports::fermyon::spin_template::template::{Action, Error as TemplateError}; //, Execute, Substitution};
use bindings::fermyon::spin_template::ui;

struct Component;
struct AddComponentToManifest;
struct AddProjectToCargo;

impl Guest for Component {
    type Edit = Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit>;

    fn run(context: &bindings::exports::fermyon::spin_template::template::ExecutionContext, options: bindings::exports::fermyon::spin_template::template::RunOptions) -> Result<Vec<Action>, TemplateError> {
        let is_add = matches!(options.mode, bindings::fermyon::spin_template::types::CreateMode::AddTo(_));
        let skip_copies = if is_add {
            vec!["spin.toml.tmpl", "Cargo.toml.tmpl"]
        } else {
            vec![]
        };

        let mut actions = vec![];

        for file in ui::File::list_all().iter().filter(|f| !skip_copies.contains(&f.path().as_str())) {  // TODO: okay this is a bit bloody laborious - I wonder if a `.createonly` extension convention could work
            let path = file.path();

            let action = if let Some(prefix) = path.strip_suffix(".raw") {
                let final_path = context.evaluate_template(prefix)?;
                Action::CopyFileToRaw((path, final_path))
            } else if let Some(prefix) = path.strip_suffix(".tmpl") {
                let final_path = context.evaluate_template(prefix)?;
                Action::CopyFileToSubstituted((path, final_path))
            } else {
                let final_path = context.evaluate_template(&file.path())?;
                Action::CopyFileToSubstituted((path, final_path))
            };

            actions.push(action);
        };

        let http_path = ui::prompt("HTTP route", Some("/..."));
        let desc = ui::prompt("Description", Some(""));

        context.set_variable("project-description", &desc);
        context.set_variable("http-path", &http_path);

        if let bindings::fermyon::spin_template::types::CreateMode::AddTo(manifest_file) = options.mode {
            let add_to_manifest: Self::Edit = Box::new(AddComponentToManifest);
            let add_to_manifest = bindings::exports::fermyon::spin_template::template::Edit::new(add_to_manifest);
            actions.push(Action::EditFile((manifest_file, add_to_manifest)));

            let add_to_cargo: Self::Edit = Box::new(AddProjectToCargo);
            let add_to_cargo = bindings::exports::fermyon::spin_template::template::Edit::new(add_to_cargo);
            actions.push(Action::EditFile(("Cargo.toml".to_owned(), add_to_cargo)));
        }

        Ok(actions)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit> {
    fn apply(&self, text: String, context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        self.as_ref().apply(text, context)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for AddComponentToManifest {
    fn apply(&self, text: String, context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        let if_it_were_new_tpl = ui::File::list_all().iter().find(|f| f.path() == "spin.toml.tmpl")
            .ok_or_else(|| TemplateError::Other("spin.toml.tmpl not found".to_owned()))?
            .read()
            .map_err(|e| TemplateError::Other(e.to_string()))?;
        let if_it_were_new_text: String = context.evaluate_template(&if_it_were_new_tpl)?;
        let mut if_it_were_new: toml_edit::DocumentMut = if_it_were_new_text.parse().map_err(|e: toml_edit::TomlError| TemplateError::Other(e.to_string()))?;

        if_it_were_new.retain(|k, _| k == "trigger" || k == "component");

        let new_stuff = if_it_were_new.to_string();
        Ok(format!("{text}\n{new_stuff}"))

    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for AddProjectToCargo {
    fn apply(&self, text: String, context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        let mut cargo: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| TemplateError::Other(e.to_string()))?;
        let members = cargo
            .get_mut("workspace")
            .and_then(|item| item.get_mut("members"))
            .and_then(|item| item.as_array_mut())
            .ok_or(TemplateError::Other("existing Cargo.toml doesn't have a workspace.members".to_owned()))?;
        members.push(context.evaluate_template("{{ project-name | kebab_case}}").unwrap());
        Ok(cargo.to_string())
    }
}

bindings::export!(Component with_types_in bindings);
