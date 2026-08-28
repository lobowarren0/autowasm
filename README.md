# AutoWASM

AutoWASM analyzes a supported web application, discovers Hono routes, compiles
supported handlers to WebAssembly, and packages the result for local execution.

## Quick start

```text
cargo test
cargo run -- analyze fixtures/hono-app
cargo run -- deploy fixtures/hono-app
```

Deployment writes deterministic artifacts to `fixtures/hono-app/.autowasm/`:

```text
.autowasm/
	manifest.json
	services/<service-name>/
		service.wasm
		metadata.json
```

To invoke an artifact directly:

```text
cargo run -- invoke fixtures/hono-app/.autowasm/services/get-hello/service.wasm GET /hello
```

## Supported compiler subset

The current compiler supports Hono handlers returning `c.json(...)` with
static JSON-compatible literals: strings, numbers, booleans, null, arrays, and
nested objects. It also supports one route parameter used as a JSON string
value, for example `c.req.param("id")` on `/users/:id`.

Handlers using network, filesystem, environment, database, or other dynamic
behavior are reported as unsupported. They are retained in `manifest.json`
with their capabilities and compilation reason; they are never silently
dropped.

## Existing commands

```text
autowasm analyze <repository-path>
autowasm deploy <repository-path>
autowasm build <wat-path> <wasm-path>
autowasm run <wasm-path>
autowasm invoke <wasm-path> <method> <path> [body]
```

This is a local packaging MVP. It does not deploy to a cloud provider.