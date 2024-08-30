#[allow(warnings)]
mod bindings;

use bindings::exports::fermyon::spin_template_filter::types::Guest;

struct Component;

impl Guest for Component {
    fn exec(text: String) -> Result<String, String> {
        let bits = text.split('-').collect::<Vec<_>>();
        Ok(bits.join("-SPORK-"))
    }
}

bindings::export!(Component with_types_in bindings);
