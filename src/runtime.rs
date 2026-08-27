use std::io;
use std::path::Path;

use wasmtime::{Engine, Instance, Module, Store};

pub fn execute_module(module_path: &Path) -> io::Result<i32> {
    let engine = Engine::default();

    let module = Module::from_file(&engine, module_path)
        .map_err(|error| io::Error::other(error.to_string()))?;

    execute_module_with_engine(&engine, module)
}

fn execute_module_with_engine(engine: &Engine, module: Module) -> io::Result<i32> {
    let mut store = Store::new(engine, ());

    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|error| io::Error::other(error.to_string()))?;

    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|error| io::Error::other(error.to_string()))?;

    run.call(&mut store, ())
        .map_err(|error| io::Error::other(error.to_string()))
}

pub fn compile_wat(wat_source: &str) -> io::Result<Vec<u8>> {
    wat::parse_str(wat_source).map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_wat() {
        let wasm = compile_wat(
            r#"
            (module
                (func (export "run") (result i32)
                    i32.const 42
                )
            )
            "#,
        )
        .unwrap();

        assert!(!wasm.is_empty());
        assert_eq!(&wasm[0..4], b"\0asm");
    }

    #[test]
    fn executes_wasm_module() {
        let wasm = compile_wat(
            r#"
            (module
                (func (export "run") (result i32)
                    i32.const 42
                )
            )
            "#,
        )
        .unwrap();

        let engine = Engine::default();

        let module =
            Module::new(&engine, wasm).expect("compiled WASM should create a valid module");

        let result = execute_module_with_engine(&engine, module).unwrap();

        assert_eq!(result, 42);
    }
}
