#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template::template::Guest;

use bindings::exports::fermyon::spin_template::template::{Action, Error as TemplateError}; //, Execute, Substitution};
use bindings::fermyon::spin_template::ui;

struct Component;
struct MyEdit;
struct BananaEdit;
struct AddComponentToManifest;

impl Guest for Component {
    type Edit = Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit>;

    fn run(context: &bindings::exports::fermyon::spin_template::template::ExecutionContext, options: bindings::exports::fermyon::spin_template::template::RunOptions) -> Result<Vec<Action>, TemplateError> {
        let is_add = matches!(options.mode, bindings::fermyon::spin_template::types::CreateMode::AddTo(_));
        let skip_copies = if is_add {
            vec!["spin.toml"]
        } else {
            vec![]
        };

        let mut actions = vec![];

        for file in ui::File::list_all().iter().filter(|f| !skip_copies.contains(&f.path().as_str())) {  // TODO: okay this is a bit bloody laborious - I wonder if a `.createonly` extension convention could work
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

        let src: usize = ui::select("What to copy", &things, Some(0)).into();
        if src == 1 {
            let srsly = ui::confirm("A banana? Really?", None);
            if !srsly {
                return Ok(vec![]);
            }
        }

        let http_path = ui::prompt("HTTP route", Some("/..."));
        let mut dest = ui::prompt("Where to write the fruit info", None);
        if dest.is_empty() {
            dest = ui::prompt("NO YOU HAVE TO ENTER A FILE C'MON", None);
            if dest.is_empty() {
                dest = ui::prompt("not gonna ask a third time, enter a fruit file, fruit is good for you", None);
                if dest.is_empty() {
                    return Ok(vec![]);
                }
            }
        }

        let desc = ui::prompt("Description", None);

        let do_it = ui::confirm("Do it?", Some(true));
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

        if let bindings::fermyon::spin_template::types::CreateMode::AddTo(manifest_file) = options.mode {
            let edit: Self::Edit = Box::new(AddComponentToManifest);
            let e = bindings::exports::fermyon::spin_template::template::Edit::new(edit);
            actions.push(Action::EditFile((manifest_file, e)));
        }

        Ok(actions)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for Box<dyn bindings::exports::fermyon::spin_template::template::GuestEdit> {
    fn apply(&self, text: String, context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        self.as_ref().apply(text, context)
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for MyEdit {
    fn apply(&self, text: String, _context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        Ok(format!("{text} AND A SPORK!"))
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for BananaEdit {
    fn apply(&self, text: String, _context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        Ok(format!("{text} But a *banana*? Really?"))
    }
}

impl bindings::exports::fermyon::spin_template::template::GuestEdit for AddComponentToManifest {
    fn apply(&self, text: String, context: &bindings::exports::fermyon::spin_template::template::ExecutionContext) -> Result<String, TemplateError> {
        // QUESTIONS:
        // 1. Where do I put the source for this - in `content` but skipped, or elsewhere?
        // 2. How do I add it - fancy TOML editing or what

        // TODO: this all seems impractically laborious for a simple "add these sections to the manifest."
        // The result doesn't seem to be very delightfully formatted either - e.g. seeing a trigger
        // between a component and its build section!

        // let trigger_type = "http";

        let if_it_were_new_tpl = ui::File::list_all().iter().find(|f| f.path() == "spin.toml")
            .ok_or_else(|| TemplateError::Other("spin.toml not found".to_owned()))?
            .read()
            .map_err(|e| TemplateError::Other(e.to_string()))?;
        let if_it_were_new_text: String = context.evaluate_template(&if_it_were_new_tpl)?;
        let mut if_it_were_new: toml_edit::DocumentMut = if_it_were_new_text.parse().map_err(|e: toml_edit::TomlError| TemplateError::Other(e.to_string()))?;

        if_it_were_new.retain(|k, _| k == "trigger" || k == "component");

        // This works (and would work better with some refactoring) BUT produces very weird
        // ordering of component tables (grouping all the component.* above all the
        // component.*.build).
        // let mut existing: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| TemplateError::Other(e.to_string()))?;
        // smoosh_toml(if_it_were_new, &mut existing);

        // let trigger_toml = if_it_were_new.get("trigger")
        //     .ok_or_else(|| TemplateError::Other("toml to merge has no trigger".to_owned()))?
        //     .as_table()
        //     .ok_or_else(|| TemplateError::Other("toml to merge trigger isn't a table".to_owned()))?
        //     .get(trigger_type)
        //     .ok_or_else(|| TemplateError::Other("toml to merge trigger doesn't have http".to_owned()))?
        //     .as_array_of_tables()
        //     .ok_or_else(|| TemplateError::Other("toml to merge trigger.http isn't a tablearray".to_owned()))?
        //     .clone();
        // let existing_trig = existing.get_mut("trigger")
        //     .ok_or_else(|| TemplateError::Other(format!("existing toml '{text}' has no trigger").to_owned()))?
        //     .get_mut(trigger_type)
        //     .ok_or_else(|| TemplateError::Other("existing toml has no trigger.http".to_owned()))?
        //     .as_array_of_tables_mut()
        //     .ok_or_else(|| TemplateError::Other("existing toml trigger.http isn't a tablearray".to_owned()))?;
        // existing_trig.extend(trigger_toml);

        // let comp = if_it_were_new.get("component")
        //     .ok_or_else(|| TemplateError::Other("toml to merge has no components".to_owned()))?
        //     .as_table()
        //     .ok_or_else(|| TemplateError::Other("toml to merge component entry isn't a table".to_owned()))?
        //     .clone();
        // let existing_comps = existing.get_mut("component")
        //     .ok_or_else(|| TemplateError::Other("existing toml has no components".to_owned()))?
        //     .as_table_mut()
        //     .ok_or_else(|| TemplateError::Other("existing toml component entry isn't a table".to_owned()))?;
        // for (comp_id, comp_table) in comp.into_iter() {
        //     existing_comps.insert(&comp_id, comp_table);
        // }

        // Ok(existing.to_string())

        // Crimes are easier, produce better output, and are legit in this case
        let new_stuff = if_it_were_new.to_string();
        Ok(format!("{text}\n{new_stuff}"))

    }
}

fn _smoosh_toml(source: toml_edit::DocumentMut, dest: &mut toml_edit::DocumentMut) {
    for (k, v) in source.iter() {
        match dest.entry(k) {
            toml_edit::Entry::Occupied(mut e) => {
                let existing = e.get_mut();
                if let Some(arr) = existing.as_array_of_tables_mut() {
                    // expect v to be a tabke array, and add array entries from v
                    let merging = v.as_array_of_tables().unwrap().clone();
                    arr.extend(merging);
                } else if let Some(tbl) = existing.as_table_mut() {
                    // expect v to be a table, and add table entries from v
                    let merging = v.as_table().unwrap().clone();
                    for (tk, tv) in merging {
                        match tbl.entry(&tk) {
                            toml_edit::Entry::Occupied(mut e) => {
                                let existing2 = e.get_mut();
                                if let Some(arr) = existing2.as_array_of_tables_mut() {
                                    let merging = tv.as_array_of_tables().unwrap().clone();
                                    arr.extend(merging);
                                } else if let Some(tbl2) = existing2.as_table_mut() {
                                    let merging = tv.as_table().unwrap().clone();
                                    for (ttk, ttv) in merging {
                                        tbl2.insert(&ttk, ttv);
                                    }
                                } else {
                                    tbl.insert(&tk, tv);
                                }
                            }
                            toml_edit::Entry::Vacant(e) => { e.insert(tv.clone()); }
                        }
                        // tbl.insert(&tk, tv);
                    }
                } else {
                    e.insert(v.clone());
                }
            },
            toml_edit::Entry::Vacant(e) => { e.insert(v.clone()); }
        }
        // if dest.contains_key(k) {
        //     // mergeapalooza
        // } else {
        //     dest.insert(k, v.clone());
        // }
    }
}

bindings::export!(Component with_types_in bindings);
