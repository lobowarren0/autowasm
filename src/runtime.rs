use std::io;
use std::path::Path;

use wasmtime::{Engine, Instance, Module, Store};

pub fn execute_module(module_path: &Path) -> io::Result<i32> {
    let engine = Engine::default();

    let module = Module::from_file(&engine, module_path)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let mut store = Store::new(&engine, ());

    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|error| io::Error::other(error.to_string()))?;

    let run = instance
        .get_typed_func::<(), i32>(&mut store, "run")
        .map_err(|error| io::Error::other(error.to_string()))?;

    run.call(&mut store, ())
        .map_err(|error| io::Error::other(error.to_string()))
}

pub fn execute_request(
    module_path: &Path,
    request: &crate::abi::Request,
) -> io::Result<crate::abi::Response> {
    let engine = Engine::default();

    let module = Module::from_file(&engine, module_path)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let mut store = Store::new(&engine, ());

    let instance = Instance::new(&mut store, &module, &[])
        .map_err(|error| io::Error::other(error.to_string()))?;

    let memory = instance
        .get_memory(&mut store, "memory")
        .ok_or_else(|| io::Error::other("WASM module does not export memory"))?;

    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "alloc")
        .map_err(|error| io::Error::other(error.to_string()))?;

    let handle = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "handle")
        .map_err(|error| io::Error::other(error.to_string()))?;

    let request_bytes =
        serde_json::to_vec(request).map_err(|error| io::Error::other(error.to_string()))?;

    let request_len =
        i32::try_from(request_bytes.len()).map_err(|_| io::Error::other("request is too large"))?;

    let request_ptr = alloc
        .call(&mut store, request_len)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let request_ptr_usize =
        usize::try_from(request_ptr).map_err(|_| io::Error::other("invalid request pointer"))?;

    memory
        .write(&mut store, request_ptr_usize, &request_bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let packed = handle
        .call(&mut store, (request_ptr, request_len))
        .map_err(|error| io::Error::other(error.to_string()))?;

    let response_ptr = (packed >> 32) as i32;
    let response_len = packed as i32;

    let response_ptr_usize =
        usize::try_from(response_ptr).map_err(|_| io::Error::other("invalid response pointer"))?;

    let response_len_usize =
        usize::try_from(response_len).map_err(|_| io::Error::other("invalid response length"))?;

    let mut response_bytes = vec![0u8; response_len_usize];

    memory
        .read(&store, response_ptr_usize, &mut response_bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;

    serde_json::from_slice(&response_bytes).map_err(|error| io::Error::other(error.to_string()))
}

pub fn compile_wat(wat_source: &str) -> io::Result<Vec<u8>> {
    wat::parse_str(wat_source).map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::fs;

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

        let mut store = Store::new(&engine, ());

        let instance = Instance::new(&mut store, &module, &[]).expect("instance should be created");

        let run = instance
            .get_typed_func::<(), i32>(&mut store, "run")
            .expect("run function should exist");

        let result = run.call(&mut store, ()).unwrap();

        assert_eq!(result, 42);
    }

    #[test]
    fn executes_request_against_wasm_module() {
        let wat_source =
            fs::read_to_string("fixtures/wasm/echo.wat").expect("echo.wat should exist");

        let wasm = compile_wat(&wat_source).expect("WAT should compile");

        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("echo.wasm");

        fs::write(&module_path, wasm).expect("temporary WASM module should be written");

        let request = crate::abi::Request::new("GET", "/hello", "");

        let response =
            execute_request(&module_path, &request).expect("request should execute successfully");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "ok");
    }
}
