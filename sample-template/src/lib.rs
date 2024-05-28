#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template::template::Guest;

struct Component;

impl Guest for Component {
    fn run() -> Result<bindings::exports::fermyon::spin_template::template::Execute, bindings::exports::fermyon::spin_template::template::Error> {

        let files = bindings::fermyon::spin_template::ui::File::list_all();
        let mut lens = String::new();
        for f in &files {
            let len = f.path();
            lens = format!("{len}, {lens}");
        }

        let things = vec!["Apple".to_owned(), "Banana".to_owned()];

        let src: usize = bindings::fermyon::spin_template::ui::select("What to copy", &things).into();
        let dest = bindings::fermyon::spin_template::ui::prompt("Where to copy it");
        let do_it = bindings::fermyon::spin_template::ui::confirm("Do it?");

        let substitutions = vec![
            bindings::exports::fermyon::spin_template::template::Substitution { key: "fruit".to_owned(), value: things[src].clone() },
        ];

        let fruit_info = bindings::fermyon::spin_template::ui::substitute_text("om nom nom {{ fruit }}", &substitutions).unwrap();
        let actions = if do_it {
            vec![
                // TODO: probably the "from" should be File resources
                bindings::exports::fermyon::spin_template::template::Action::CopyFileSubstituted(("fruit.txt".to_owned(), dest.clone())),
                bindings::exports::fermyon::spin_template::template::Action::CopyFileRaw((files[0].path(), "raw_fruit.txt".to_owned())),
                bindings::exports::fermyon::spin_template::template::Action::WriteFile(("writed.txt".to_owned(), fruit_info)),
                bindings::exports::fermyon::spin_template::template::Action::WriteFileBinary(("binned.bin".to_owned(), vec![1,2,3,4])),
            ]
        } else {
            vec![]
        };
        let ex = bindings::exports::fermyon::spin_template::template::Execute {
            substitutions,
            actions
        };
        Ok(ex)
    }
}

bindings::export!(Component with_types_in bindings);
