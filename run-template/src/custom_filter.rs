use std::{
    fmt::{Debug, Display},
    path::Path,
    sync::{Arc, RwLock},
};

// use anyhow::Context;
use liquid_core::{Filter, ParseFilter, Runtime, ValueView};

wasmtime::component::bindgen!({
    path: "../wit/filter",
});

#[derive(Clone)]
pub struct CustomFilterParser {
    name: String,
    store: Arc<RwLock<wasmtime::Store<()>>>,
    bindings: Arc<CustomFilter>,
    _instance: wasmtime::component::Instance,
}

impl CustomFilterParser {
    pub fn load(name: &str, wasm_path: &Path) -> anyhow::Result<Self> {
        let mut config = wasmtime::Config::new();
        config.wasm_component_model(true);
        let engine = wasmtime::Engine::new(&config).expect("shoulda engined");
    
        let component = wasmtime::component::Component::from_file(&engine, wasm_path).expect("shoulda loaded a component");
    
        let linker = wasmtime::component::Linker::new(&engine);
    
        let mut store = wasmtime::Store::new(&engine, ());

        let (bindings, instance) = CustomFilter::instantiate(&mut store, &component, &linker).expect("should instantiated");
    
        Ok(Self {
            name: name.to_owned(),
            store: Arc::new(RwLock::new(store)),
            bindings: Arc::new(bindings),
            _instance: instance,
        })
    }
}

impl Debug for CustomFilterParser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomFilterParser")
            .field("name", &self.name)
            .finish()
    }
}

impl ParseFilter for CustomFilterParser {
    fn parse(
        &self,
        _arguments: liquid_core::parser::FilterArguments,
    ) -> liquid_core::Result<Box<dyn Filter>> {
        Ok(Box::new(CustomFilterRunner {
            name: self.name.to_owned(),
            store: self.store.clone(),
            bindings: self.bindings.clone(),
        }))
    }

    fn reflection(&self) -> &dyn liquid_core::FilterReflection {
        self
    }
}

const EMPTY: [liquid_core::parser::ParameterReflection; 0] = [];

impl liquid_core::FilterReflection for CustomFilterParser {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        ""
    }

    fn positional_parameters(&self) -> &'static [liquid_core::parser::ParameterReflection] {
        &EMPTY
    }

    fn keyword_parameters(&self) -> &'static [liquid_core::parser::ParameterReflection] {
        &EMPTY
    }
}

struct CustomFilterRunner {
    name: String,
    store: Arc<RwLock<wasmtime::Store<()>>>,
    bindings: Arc<CustomFilter>,
}

impl Debug for CustomFilterRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomFilter")
            .field("name", &self.name)
            .finish()
    }
}

impl Display for CustomFilterRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl Filter for CustomFilterRunner {
    fn evaluate(
        &self,
        input: &dyn ValueView,
        _runtime: &dyn Runtime,
    ) -> Result<liquid::model::Value, liquid_core::error::Error> {
        let input_str = self.liquid_value_as_string(input)?;
        let mut store = self.store.write().unwrap();
        match self.bindings.fermyon_spin_template_filter_types().call_exec(&mut *store, &input_str) {
            Ok(Ok(text)) => Ok(to_liquid_value(text)),
            Ok(Err(s)) => Err(liquid_err(s)),
            Err(trap) => Err(liquid_err(format!("{:?}", trap))),
        }
    }
}

impl CustomFilterRunner {
    fn liquid_value_as_string(&self, input: &dyn ValueView) -> Result<String, liquid::Error> {
        let str = input.as_scalar().map(|s| s.into_cow_str()).ok_or_else(|| {
            liquid_err(format!(
                "Filter '{}': no input or input is not a string",
                self.name
            ))
        })?;
        Ok(str.to_string())
    }
}

fn to_liquid_value(value: String) -> liquid::model::Value {
    liquid::model::Value::Scalar(liquid::model::Scalar::from(value))
}

fn liquid_err(text: String) -> liquid_core::error::Error {
    liquid_core::error::Error::with_msg(text)
}

