# Testing

> **Status:** draft.

## Principles

- **Automated tests never call the real API.** No test run by `mise run test` or CI talks to `api.typesafe.ai` or needs a real API key. Tests use a fake key and a local mock server. The [manual evaluations](#manual-evaluations) are the only exception, and they are run by hand.
- **Test through real HTTP.** The client is tested against a local mock server rather than a mocked trait, so reqwest, headers, retries, `Retry-After` and the timeout are all exercised the way they run in production.
- **Built-in test framework.** Tests use Rust's standard `cargo test`, with a few helper crates (below).

## Crates (all dev-dependencies)

| Crate                                                                                                   | Used for                                                         |
| ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| [wiremock](https://crates.io/crates/wiremock)                                                           | Local mock HTTP server for the Jev API                           |
| [assert_cmd](https://crates.io/crates/assert_cmd) and [predicates](https://crates.io/crates/predicates) | Run the `gutcheck` binary and check stdout, stderr and exit code |
| [insta](https://crates.io/crates/insta)                                                                 | Snapshot tests (MCP tool description and schema, error messages) |
| [jsonschema](https://crates.io/crates/jsonschema)                                                       | Validate our types against the checked-in OpenAPI spec           |
| [rstest](https://crates.io/crates/rstest)                                                               | Table-driven cases, such as the list of local checks             |
| [tempfile](https://crates.io/crates/tempfile)                                                           | Temporary config files for CLI tests                             |

## Test layers

### 1. Unit tests

Inside the library modules (`#[cfg(test)]`). Fast, no I/O.

- Local checks from [`api-client.md`](api-client.md): one case per rule, valid and invalid.
- Backoff calculation: doubling, the 5 s cap, jitter range, `Retry-After` handling.
- "Will the next wait fit in the remaining time?" logic.
- Config loading and precedence: defaults, config file, flag overrides ([`configuration.md`](configuration.md)).
- Formatting `422` error details as `field: message`.

### 2. Client tests against a mock server

In `tests/`, using the library and wiremock.

- **Request shape:** method, path, `Authorization` header, `Content-Type`, and body.
- **Each status code** maps to the right error and message (`400`, `401`, `413`, `422`).
- **Retries:** `429`, `529`, `5xx` and `408` are retried, up to `retries`; `400`-class errors are not.
- **`Retry-After`** is honored.
- **Timeout:** with many retries allowed and a short timeout, the call stops at the timeout. A slow response is cancelled when the timeout is reached.

Real backoff delays would make these tests slow. The backoff values (0.5 s start, 5 s cap) are fixed for users but can be set to small values inside the library for tests.

### 3. CLI tests

In `tests/`, using assert_cmd to run the real `gutcheck` binary against wiremock. Each test writes a temporary config file with `base_url` set to the mock server's address and passes it with `--config` (see [`configuration.md`](configuration.md)). The library's client tests set the base URL directly.

- Each `ask` subcommand: success prints the plain answer to stdout and exits `0`, and `--json` prints one line of JSON instead.
- Errors: nothing on stdout, a message on stderr, exit `1` ([`output-and-exit-codes.md`](output-and-exit-codes.md)).
- State input: `--state`, stdin, and the missing-state error. `--state` wins when both sources are given and leaves stdin unread without waiting to check it. Cover `/dev/null`, a closed pipe, a pipe with data or one that stays silent or gets late data, a file (unread, empty or at its end), and a real shell `while read` loop.
- Choice options: bare keys and keys with descriptions, including a mixture of the two, produce the intended request and print the selected key. Split descriptions at the first `=` only, preserve later `=` characters, and reject empty or duplicate keys, including duplicates across the two forms.
- Noul output modes: plain output is the probability rounded to at most three decimal places; `--boolean` uses the unrounded value, a default cutoff of `0.5` and a strict `>` comparison, including equality at custom cutoffs. Cover thresholds `0` and `1`, invalid values, `--threshold` without `--boolean`, the `--boolean`/`--json` conflict, and JSON retaining precision.
- Score levels: flag order defines the zero-based rubric; a fractional answer uses the common plain-number display rule without snapping to a level or rescaling, and `--json` retains the answer's legend, probabilities and confidence.
- Parser contracts: repeated single-value flags are rejected, requested help and `--version` print to stdout and exit `0`, a missing command prints help to stderr and exits `1`, errors exit `1`, and the binary's `__usage_spec__` output matches the checked-in generated KDL.
- `gutcheck auth validate` against a mock `GET /v1/models`.
- `gutcheck skill` prints the embedded skill.

### 4. MCP tests

Start the MCP server in-process and connect an rmcp client to it over an in-memory transport.

- `tools/list` returns `gutcheck_ask`. Its description and input schema are snapshot-tested with insta.
- Calling `gutcheck_ask` against wiremock returns only `answers`, including for several questions in one call.
- The `model` argument overrides the configured model; without it, the configured model is sent.
- Unknown question types pass through to the API unchanged.
- Local check failures and API errors come back as tool errors, and failed local checks send no request.

### 5. Spec tests

Check our serde types against the checked-in `spec/openapi.json` with jsonschema (see [`language-and-distribution.md`](language-and-distribution.md)): serialized requests match `SystemOneRequest`, and response fixtures match `SystemOneResponse` and deserialize into our types.

## Fixtures

Sample API responses live in `tests/fixtures/` as JSON files, one per case (for example `choice_success.json`, `validation_error_422.json`). Each is checked against the spec, so the mocks can't drift from the real API.

## Manual evaluations

`mise run eval` runs the release `gutcheck` binary against the **real** Jev API, to check that the CLI works end to end. It covers what the mocked tests can't show:

- that the real API accepts the requests we build;
- that real answers come back in the shape we expect and make sense;
- that real API errors produce clear messages.

Run it by hand with a real key from fnox:

```bash
fnox exec -- mise run eval
```

It uses a real API key and spends real API calls (about 16 per run), so it is kept small. It is never part of `mise run test` or CI. Without `TYPESAFE_API_KEY`, the task exits `1` right away with a one-line message saying how to run it. Otherwise it builds the release binary and runs `evals/run.sh`.

The cases live in one bash script, `evals/run.sh`, which checks JSON with `jq`. Each case prints `PASS` or `FAIL` and its name. A failure also prints the command, exit code, stdout and stderr, with the API key redacted. The run ends with a summary and exits `1` if any case failed.

| Group           | Cases                                                                                                                                                                                                                                                                 |
| --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Contract        | For `noul`, `choice` and `score`: plain output (one line of the right shape, nothing on stderr) and `--json` (one line of JSON with the type's fields), with state from `--state` and from stdin. `noul --boolean` prints `true` or `false`. `auth validate` succeeds |
| Answer sanity   | Clear-cut questions with loose thresholds, since model output isn't deterministic: an obvious yes is above `0.8` and an obvious no below `0.2`; a refund request picks `billing`; a furious message scores at least levels − 1.5 and a calm one at most `0.5`         |
| Real API errors | An invalid key (`401`) for one `ask` call and for `auth validate`, and `--model does-not-exist`. Each prints nothing on stdout, one line on stderr naming the problem, and exits `1`                                                                                  |

Local checks and the other error cases are covered by the mocked tests and are not repeated here. A mock-backed mode would repeat `tests/cli.rs`, so there isn't one.

When a later command lands (`mcp serve`, `skill`), its change adds eval cases for it.

## Running tests

`mise run test` runs `cargo test`, which includes unit tests, integration tests and doc tests.

Run `mise install` first, then use `mise run test` locally and in CI. The parser conformance test in `src/cli.rs` invokes the pinned Usage CLI to compare native parsing with the generated spec. Running `cargo test` directly requires that Usage version on `PATH`; the test fails if the executable is missing.

We use plain `cargo test` rather than [cargo-nextest](https://nexte.st/). The suite is small and fast, and nextest doesn't run doc tests. Switching later only means changing the `test` task (plus `cargo test --doc` for doc tests), for example if CI needs retries for flaky tests or clearer output.
