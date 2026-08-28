# AutoWASM

AutoWASM analyzes a supported web application, discovers Hono routes from
TypeScript or JavaScript, compiles supported handlers to WebAssembly, and
packages the result for local execution.

## Quick start

```text
cargo test
cargo run -- analyze fixtures/hono-app
cargo run -- deploy fixtures/hono-app
cargo run -- deploy fixtures/hono-app --allow-capability network
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
nested objects. It also supports static `c.text(...)` responses, explicit
numeric status codes, and one route parameter used as a JSON string value, for
example `c.req.param("id")` on `/users/:id`.

Handlers using network, filesystem, environment, database, or other dynamic
behavior are reported as unsupported. They are retained in `manifest.json`
with their capabilities and compilation reason; they are never silently
dropped.

## Existing commands

```text
autowasm analyze <repository-path>
autowasm deploy <repository-path>
autowasm deploy <repository-path> [--allow-capability <name>]...
autowasm build <wat-path> <wasm-path>
autowasm run <wasm-path>
autowasm invoke <wasm-path> <method> <path> [body]
```

This is a local packaging MVP. It does not deploy to a cloud provider.
Capability flags configure the policy boundary; capability-specific host
implementations remain unsupported until their runtime integration is added.

## Cloudflare deployment

Cloudflare Workers is the first cloud provider supported. AutoWASM packages
all compiled services into one module Worker and adds a route table in a thin
JavaScript adapter. Each request is converted to the existing AutoWASM JSON
request ABI, executed by the selected Wasm service, and converted back to an
HTTP response.

Create a Cloudflare API token with Workers Scripts Write permission, then set:

```text
CLOUDFLARE_API_TOKEN=<token>
CLOUDFLARE_ACCOUNT_ID=<account-id>
AUTOWASM_CLOUDFLARE_WORKER_NAME=<worker-name>
```

Deploy explicitly with:

```text
cargo run -- deploy fixtures/hono-js --provider cloudflare
```

The provider uses Cloudflare's Workers Script Upload API and updates the
deterministic Worker name on repeated deployments. The API may return a
deployment ID without a URL; in that case AutoWASM tells you to configure a
Worker route or workers.dev subdomain rather than inventing an endpoint.

Cloud deployment is opt-in, requires network access and credentials, and does
not weaken capability policy. Unsupported services remain excluded from the
uploaded Worker and remain listed in the local manifest with their reasons.