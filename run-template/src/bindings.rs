wasmtime::component::bindgen!({
    path: "../wit/template",
    trappable_imports: ["prompt", "confirm", "select"],

    with: {
        "fermyon:spin-template/ui/file": std::path::PathBuf,
        "fermyon:spin-template/types/execution-context": crate::host::ExecutionContext,
    }
});
