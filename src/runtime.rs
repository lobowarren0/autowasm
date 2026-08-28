use std::io;
use std::path::Path;

use wasmtime::{Config, Engine, Instance, Module, Store};

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_FUEL: u64 = 1_000_000;

fn create_engine() -> io::Result<Engine> {
    let mut config = Config::new();
    config.consume_fuel(true);
    Engine::new(&config).map_err(|error| io::Error::other(error.to_string()))
}

fn create_store(engine: &Engine) -> io::Result<Store<()>> {
    let mut store = Store::new(engine, ());
    store
        .set_fuel(MAX_FUEL)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(store)
}

pub fn execute_module(module_path: &Path) -> io::Result<i32> {
    let engine = create_engine()?;

    let module = Module::from_file(&engine, module_path)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let mut store = create_store(&engine)?;

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
    let engine = create_engine()?;

    let module = Module::from_file(&engine, module_path)
        .map_err(|error| io::Error::other(error.to_string()))?;

    let mut store = create_store(&engine)?;

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

    if request_bytes.len() > MAX_REQUEST_BYTES {
        return Err(io::Error::other(
            "request exceeds the maximum supported size",
        ));
    }

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

    let (response_ptr, response_len) = unpack_pointer_length(packed)?;

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

fn unpack_pointer_length(packed: i64) -> io::Result<(u32, u32)> {
    if packed < 0 {
        return Err(io::Error::other(
            "invalid packed response pointer and length",
        ));
    }

    let packed = packed as u64;
    let pointer = (packed >> 32) as u32;
    let length = packed as u32;

    if length as usize > MAX_RESPONSE_BYTES {
        return Err(io::Error::other(
            "response exceeds the maximum supported size",
        ));
    }

    Ok((pointer, length))
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

    #[test]
    fn rejects_invalid_packed_response_values() {
        assert!(unpack_pointer_length(-1).is_err());
        assert!(
            unpack_pointer_length(((2048_i64) << 32) | ((MAX_RESPONSE_BYTES as i64) + 1)).is_err()
        );
        assert_eq!(
            unpack_pointer_length((2048_i64 << 32) | 26).unwrap(),
            (2048, 26)
        );
    }

    #[test]
    fn rejects_modules_that_exhaust_fuel() {
        let module_path = write_temporary_module(
            r#"
            (module
                (func (export "run") (result i32)
                    (loop (br 0))
                )
            )
            "#,
        );

        let error = execute_module(&module_path).unwrap_err();

        assert!(!error.to_string().is_empty());
    }

    fn write_temporary_module(wat_source: &str) -> std::path::PathBuf {
        let wasm = compile_wat(wat_source).expect("WAT should compile");
        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("module.wasm");
        std::fs::write(&module_path, wasm).expect("temporary module should be written");
        Box::leak(Box::new(temp_dir));
        module_path
    }

    #[test]
    fn compiles_service_and_executes_request() {
        let service = crate::service::Service::new(
            "get-hello".to_string(),
            "GET".to_string(),
            "/hello".to_string(),
            r#"(c) => {
  return c.json({ message: "hello", active: true });
}"#
            .to_string(),
            vec![],
        );

        let wasm = crate::compiler::compile_service(&service).expect("service should compile");
        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("service.wasm");
        fs::write(&module_path, wasm).expect("temporary WASM module should be written");

        let request = crate::abi::Request::new("GET", "/hello", "");
        let response = execute_request(&module_path, &request).expect("request should execute");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"active":true,"message":"hello"}"#);
    }

    #[test]
    fn compiles_and_executes_route_parameter_response() {
        let service = crate::service::Service::new(
            "delete-users-id".to_string(),
            "DELETE".to_string(),
            "/users/:id".to_string(),
            r#"(c) => {
  return c.json({ id: c.req.param("id") });
}"#
            .to_string(),
            vec![],
        );

        let wasm = crate::compiler::compile_service(&service).expect("service should compile");
        let engine = Engine::default();
        Module::new(&engine, &wasm).expect("route parameter module should validate");
        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("service.wasm");
        fs::write(&module_path, wasm).expect("temporary WASM module should be written");

        let request = crate::abi::Request::new("DELETE", "/users/123", "");
        let response = execute_request(&module_path, &request).expect("request should execute");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, r#"{"id":"123"}"#);
    }

    #[test]
    fn compiles_and_executes_static_text_response() {
        let service = crate::service::Service::new(
            "get-health".to_string(),
            "GET".to_string(),
            "/health".to_string(),
            r#"(c) => {
  return c.text("ok");
}"#
            .to_string(),
            vec![],
        );

        let wasm = crate::compiler::compile_service(&service).expect("service should compile");
        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("service.wasm");
        fs::write(&module_path, wasm).expect("temporary WASM module should be written");

        let response = execute_request(
            &module_path,
            &crate::abi::Request::new("GET", "/health", ""),
        )
        .expect("request should execute");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, "ok");
    }

    #[test]
    fn executes_static_response_status() {
        let service = crate::service::Service::new(
            "post-users".to_string(),
            "POST".to_string(),
            "/users".to_string(),
            r#"(c) => c.json({ created: true }, 201)"#.to_string(),
            vec![],
        );
        let wasm = crate::compiler::compile_service(&service).expect("service should compile");
        let temp_dir = tempfile::tempdir().expect("temporary directory should be created");
        let module_path = temp_dir.path().join("service.wasm");
        fs::write(&module_path, wasm).expect("temporary WASM module should be written");

        let response = execute_request(
            &module_path,
            &crate::abi::Request::new("POST", "/users", ""),
        )
        .expect("request should execute");

        assert_eq!(response.status, 201);
        assert_eq!(response.body, r#"{"created":true}"#);
    }
}
