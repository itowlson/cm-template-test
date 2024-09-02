#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template::template::Guest;

use bindings::exports::fermyon::spin_template::template::{Action, Error as TemplateError}; //, Execute, Substitution};
use bindings::fermyon::spin_template::ui;

struct Component;
struct MyEdit;
struct BananaEdit;

impl Guest for Component {
    type Edit = Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit>;

    fn run(context: bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<Vec<Action>, TemplateError> {
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
            let srsly = ui::confirm("A banana? Really?");
            if !srsly {
                return Ok(vec![]);
            }
        }

        let http_path = ui::prompt("HTTP route"); // TODO: needs a default
        let dest = ui::prompt("Where to write the fruit info");

        let desc = ui::prompt("Description");

        let do_it = ui::confirm("Do it?");
        if !do_it {
            return Ok(vec![]);
        }

        context.set_variable("fruit", &things[src]);
        context.set_variable("project-description", &desc);
        context.set_variable("http-path", &http_path);

        let fruit_info = context.evaluate_template("om nom nom {{ fruit }}")?;
        actions.push(Action::WriteFile((dest, fruit_info)));

        actions.push(Action::WriteFileBinary(("binned.bin".to_owned(), vec![1,2,3,4])));

        let edit: Self::Edit = if src == 1 {
            Box::new(BananaEdit)
        } else {
            Box::new(MyEdit)
        };
        let e = bindings::exports::fermyon::spin_template::template::Edit::new(edit);
        actions.push(Action::EditFile(("spork.txt".to_owned(), e)));

        Ok(actions)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit> {
    fn apply(&self, text: String) -> Result<String, TemplateError> {
        self.as_ref().apply(text)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for MyEdit {
    fn apply(&self, text: String) -> Result<String, TemplateError> {
        Ok(format!("{text} AND A SPORK!"))
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for BananaEdit {
    fn apply(&self, text: String) -> Result<String, TemplateError> {
        Ok(format!("{text} But a *banana*? Really?"))
    }
}

bindings::export!(Component with_types_in bindings);
