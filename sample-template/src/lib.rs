#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template::template::Guest;

use bindings::exports::fermyon::spin_template::template::{Action, Error as TemplateError, Execute, Substitution};
use bindings::fermyon::spin_template::ui;

struct Component;

impl Guest for Component {
    fn run() -> Result<Execute, TemplateError> {
        let mut actions = vec![];

        for file in ui::File::list_all() {
            let path = file.path();

            let action = if let Some(prefix) = file.path().strip_suffix(".raw") {
                Action::CopyFileToRaw((file.path(), prefix.to_owned()))
            } else if let Some(prefix) = file.path().strip_suffix(".tmpl") {
                Action::CopyFileToSubstituted((file.path(), prefix.to_owned()))
            } else {
                Action::CopyFileSubstituted(path)
            };

            actions.push(action);
        };

        let things = vec!["Apple".to_owned(), "Banana".to_owned()];

        let src: usize = ui::select("What to copy", &things).into();
        if src == 1 {
            let srsly = ui::confirm("A banana, really?");
            if !srsly {
                return Err(TemplateError::Cancel);
            }
        }

        let http_path = ui::prompt("HTTP route"); // TODO: needs a default
        let dest = ui::prompt("Where to write the fruit info");

        let desc = ui::prompt("Description");

        let do_it = ui::confirm("Do it?");
        if !do_it {
            return Err(TemplateError::Cancel);
        }

        let substitutions = vec![
            Substitution { key: "fruit".to_owned(), value: things[src].clone() },
            Substitution { key: "project-description".to_owned(), value: desc },
            Substitution { key: "http-path".to_owned(), value: http_path },
        ];

        let fruit_info = ui::substitute_text("om nom nom {{ fruit }}", &substitutions)?;
        actions.push(Action::WriteFile((dest, fruit_info)));

        actions.push(Action::WriteFileBinary(("binned.bin".to_owned(), vec![1,2,3,4])));

        let ex = Execute {
            substitutions,
            actions
        };
        Ok(ex)
    }
}

bindings::export!(Component with_types_in bindings);
