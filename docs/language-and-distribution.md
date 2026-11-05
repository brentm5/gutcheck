# Language and distribution

> **Status:** draft.

`gutcheck` is written in Rust and ships as a single static binary.

## Libraries

| Need                            | Crate                                                                                                                        |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| CLI framework                   | [clap](https://crates.io/crates/clap) (derive API): nested subcommands and global flags                                      |
| Async runtime                   | [tokio](https://crates.io/crates/tokio)                                                                                      |
| HTTP client                     | [reqwest](https://crates.io/crates/reqwest) with rustls (no OpenSSL dependency)                                              |
| JSON                            | [serde](https://crates.io/crates/serde) and [serde_json](https://crates.io/crates/serde_json)                                |
| MCP server                      | [rmcp](https://crates.io/crates/rmcp) (official MCP SDK) with the stdio transport                                            |
| Config file                     | [toml](https://crates.io/crates/toml)                                                                                        |
| Errors                          | [thiserror](https://crates.io/crates/thiserror)                                                                              |
| HTTP dates                      | [httpdate](https://crates.io/crates/httpdate), for the date form of `Retry-After`                                            |
| Request bodies                  | [bytes](https://crates.io/crates/bytes), so a retry doesn't copy the body                                                    |
| Checking stdin                  | [rustix](https://crates.io/crates/rustix), to see how much data is waiting without reading it (Unix only)                    |
| Spec validation (tests only)    | [jsonschema](https://crates.io/crates/jsonschema), as a dev-dependency                                                       |
| Snapshot tests (tests only)     | [insta](https://crates.io/crates/insta), as a dev-dependency (MCP tool description and schema)                               |
| Mock API server (tests only)    | [wiremock](https://crates.io/crates/wiremock), as a dev-dependency                                                           |
| CLI tests (tests only)          | [assert_cmd](https://crates.io/crates/assert_cmd) and [predicates](https://crates.io/crates/predicates), as dev-dependencies |
| Table-driven tests (tests only) | [rstest](https://crates.io/crates/rstest), as a dev-dependency                                                               |
| Temporary files (tests only)    | [tempfile](https://crates.io/crates/tempfile), as a dev-dependency (config files for CLI tests)                              |

Crate versions are pinned through `Cargo.lock`, not in this doc. How the test crates are used is in [`testing.md`](testing.md).

## Crate layout

One Cargo package with a library and a thin binary. Cargo picks up both files automatically, with no workspace and no extra `Cargo.toml` config.

```text
src/
  lib.rs    the core: API types, local checks, client, retries, config, errors
  main.rs   thin: parse clap args, call the library, map errors to exit codes
tests/      integration tests against the library (including spec validation)
spec/       checked-in copy of TypeSafe's openapi.json
```

- `main.rs` (and the MCP server code it starts) can only use what `lib.rs` makes public, which enforces the "shared core, thin front ends" pattern below.
- Tests in `tests/` import the library directly.
- Code examples in doc comments run as doc tests.
- Keep the public API of `lib.rs` small: only what the CLI and MCP front ends need.

## Patterns

### Async entry point

`rmcp` and `reqwest` both need tokio, so `main` runs every command on a tokio runtime, including one-shot `ask` commands. Use the current-thread runtime unless there's a measured reason not to.

`main` builds the runtime by hand rather than using `#[tokio::main]`, and ends it with `shutdown_background()`. `#[tokio::main]` waits for blocking tasks at exit, which includes DNS lookups that can't be cancelled, so a stuck resolver could keep the process alive long after `timeout`.

### One timeout for the whole operation

The configured `timeout` becomes a single deadline, set when the operation starts and passed down, so time spent before the request (such as waiting on stdin) counts too. The full request-plus-retries future runs under `tokio::time::timeout_at` with that deadline. When it fires, the in-flight request is dropped (cancelled) and the command returns a timeout error. The retry loop itself only enforces `retries` and checks that the next backoff fits in the time left before the deadline. See [`configuration.md`](configuration.md).

### Retries and backoff

Written in-house, not with a retry crate. The rules (exponential backoff from 0.5 s to 5 s, jitter, `Retry-After`) are short, and owning them keeps the timeout behavior exact.

### Typed API models

Hand-written serde types for the request and response, **validated against TypeSafe's OpenAPI spec in tests**.

The types:

- Questions are an enum tagged on `type` (`choice`, `score`, `noul`) with an extra variant that keeps unknown types as raw JSON, so `ask_jev` can pass them through ([`api-client.md`](api-client.md)).
- Answers follow the same pattern.
- Only `noul` exists so far. The other types and the unknown-type variant arrive with the commands that need them.
- Error bodies get their own types for `400` (`detail`) and `422` (`detail[]` with `loc`, `msg`).

Validating against the spec:

- A copy of the spec is checked in at `spec/openapi.json`, so tests run offline and changes to it show up in review.
- Tests serialize our request types and check them against the spec's `SystemOneRequest` schema with `jsonschema`. Sample responses (fixtures) are checked against `SystemOneResponse` and then deserialized into our types, so both directions are covered.
- A `spec:update` mise task downloads the latest spec from `https://api.typesafe.ai/openapi.json`. When TypeSafe changes the API, updating the spec makes the tests show what broke.
- Updates are manual for now: run `spec:update` before a release, or when TypeSafe announces API changes. There's no automatic check, because the server sends no `ETag` or `Last-Modified` header and `info.version` (currently `0.2.0`) isn't guaranteed to change, so comparing the whole file is the only reliable signal. A scheduled CI job that opens a PR when the file changes is a possible later addition.

Why not generate the types: the spec is OpenAPI 3.1, which the main Rust generator (progenitor) doesn't fully support, and generated types wouldn't include the unknown-type pass-through variant we need. The schema is small, so hand-written types plus spec tests give the same safety with code we control.

### Errors and exit codes

One error enum (with `thiserror`) whose variants match the categories in [`api-client.md`](api-client.md): usage/validation, API rejection, temporary failure, timeout, plus one for an answer that couldn't be printed. For now every variant exits with code 1 ([`output-and-exit-codes.md`](output-and-exit-codes.md)), but keeping the categories makes distinct exit codes easy to add later. `main` handles the exit code and the stderr message in one place. Library code never calls `process::exit`.

### Shared core, thin front ends

The CLI commands and the MCP server are thin layers over the same core:

```text
cli (clap) ─┐
            ├─► core: build request → local checks → send with retries → typed response
mcp (rmcp) ─┘
```

Command handlers only parse input and format output. Validation, HTTP, retries and error mapping live in the core.

### Configuration loading

Load the TOML file into a settings struct with defaults (`#[serde(default)]`), then apply `--model` and `--timeout` from clap on top. The result is a single resolved settings value passed to the core. See [`configuration.md`](configuration.md).

### Embedded skill

The skill Markdown lives in the repo as a normal file and is compiled in with `include_str!`. `gutcheck skill` writes it to stdout unchanged. See [`agent-skill.md`](agent-skill.md).

## Tooling

| Need      | Choice                                                                   |
| --------- | ------------------------------------------------------------------------ |
| Toolchain | Rust pinned to an exact version in `mise.toml` (with clippy and rustfmt) |
| Format    | `cargo fmt`, as a `fmt:rust` mise task                                   |
| Lint      | `cargo clippy` (warnings as errors), as a `lint:rust` mise task          |
| Test      | `cargo test`, as a `test` mise task                                      |
| API spec  | `spec:update` mise task to refresh `spec/openapi.json`                   |

The pinned version is the latest stable release when it's set. It's bumped on purpose when a new stable ships (about every six weeks), in its own change, together with `rust-version` in `Cargo.toml`, so any new clippy warnings are fixed in that one change.

## Distribution

[dist](https://opensource.axo.dev/cargo-dist/) builds release binaries and publishes them as GitHub releases.

| Platform | Targets                                                            |
| -------- | ------------------------------------------------------------------ |
| macOS    | `aarch64-apple-darwin`, `x86_64-apple-darwin`                      |
| Linux    | `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` (static) |

License: Apache 2.0 (`LICENSE` at the repo root, `license = "Apache-2.0"` in `Cargo.toml`). dist includes it in each release archive.

Install channels:

- **GitHub releases:** download the archive for your platform.
- **mise:** `mise use github:brentm5/gutcheck`, which installs from the GitHub releases.

Not at launch: Windows builds (later), Homebrew, crates.io, other package managers.

The release process itself (versioning, tagging, changelog, CI workflow) isn't decided yet. See [`development.md`](development.md#deferred).
