# Development

## Tooling

[mise](https://mise.jdx.dev/) manages the project's tool versions and task definitions. Both live in `mise.toml` at the repo root, and [`mise.lock`](#mise-lock) records the resolved tool versions, download URLs and checksums.

```bash
mise install    # install the pinned tools
mise tasks      # list available tasks
```

| Tool                                                 | Purpose                                                 | Config               |
| ---------------------------------------------------- | ------------------------------------------------------- | -------------------- |
| [rumdl](https://github.com/rvben/rumdl)              | Lint and format Markdown                                | `.rumdl.toml`        |
| Rust (with clippy and rustfmt)                       | Build, lint and format the CLI                          | `Cargo.toml`         |
| [Usage](https://usage.jdx.dev/guide/getting-started) | Validate the CLI spec and generate docs and completions | `gutcheck.usage.kdl` |
| [jq](https://jqlang.org/)                            | Check and pretty-print the downloaded OpenAPI spec      | —                    |

Rust is pinned in `mise.toml`; see [`language-and-distribution.md`](language-and-distribution.md) for libraries and patterns.

```bash
cargo build                   # build a debug binary
cargo run -- --version        # run it
```

## Tasks

| Task                   | What it does                                                                        |
| ---------------------- | ----------------------------------------------------------------------------------- |
| `mise run lint`        | Run all linters                                                                     |
| `mise run fmt`         | Run all formatters                                                                  |
| `mise run lint:md`     | Lint Markdown with `rumdl check`                                                    |
| `mise run fmt:md`      | Format Markdown with `rumdl fmt`                                                    |
| `mise run lint:rust`   | Lint Rust with `cargo clippy` (warnings are errors)                                 |
| `mise run lint:usage`  | Validate `gutcheck.usage.kdl`, treating warnings as errors                          |
| `mise run fmt:rust`    | Format Rust with `cargo fmt`                                                        |
| `mise run build`       | Build the release binary at `target/release/gutcheck` with `cargo build --release`  |
| `mise run test`        | Run all tests with `cargo test`                                                     |
| `mise run lock`        | Update `mise.lock` for Linux and macOS after changing a tool version in `mise.toml` |
| `mise run spec:usage`  | Regenerate the Usage spec from the executable's Rust declarations                   |
| `mise run spec:update` | Download the latest OpenAPI spec to `spec/openapi.json`, pretty-printed with `jq`   |

`lint` and `fmt` are umbrella tasks. As new tools are added, each gets its own `lint:<name>` and `fmt:<name>` task, which the umbrella tasks depend on.

## Usage specification

Usage 7.3.0 is pinned in `mise.toml`. `mise run lint` includes `usage lint --warnings-as-errors gutcheck.usage.kdl` through `lint:usage`.

The executable uses `usage-rs` 7.3.0. `src/cli.rs` defines its parser and metadata, and the binary exports them through `__usage_spec__`. Package metadata comes from Cargo. Regenerate the checked-in spec after changing the CLI or package version:

```bash
mise run spec:usage
```

A test compares the checked-in file with the binary's export, so stale declarations or package metadata fail the test suite. The generated file lists only implemented commands; planned commands remain in the design docs. Generate a reference or shell completion script without calling the API:

```bash
mise exec -- usage generate markdown --file gutcheck.usage.kdl --out-file /tmp/gutcheck-cli.md
mise exec -- usage generate completion bash gutcheck --file gutcheck.usage.kdl > /tmp/gutcheck-completion.bash
```

Inspect argument binding and flag dependencies with `explain`; it reports errors in the result even when the explain command itself succeeds:

```bash
mise exec -- usage explain --file gutcheck.usage.kdl -- gutcheck ask noul "The message is urgent" --boolean
```

## mise lock

`mise.lock` pins each tool in `mise.toml` to an exact version and, where the tool's backend supports it, the download URL and checksum for each platform. It covers `linux-x64`, `linux-arm64`, `macos-x64` and `macos-arm64`.

| Tool  | What the lockfile records                                                          |
| ----- | ---------------------------------------------------------------------------------- |
| jq    | Version, plus URL, checksum and GitHub attestation for each platform               |
| rumdl | Version, plus URL, checksum and GitHub attestation for each platform               |
| Usage | Version, plus URL, checksum and Sigstore signer for each platform                  |
| Rust  | Version and components only; rustup installs it, so there are no URLs or checksums |

`mise install` reads the lockfile as usual. After changing a version in `mise.toml`, update the lockfile and commit both files together:

```bash
mise run lock
```

The task keeps the platform list in one place. CI installs with `mise install --locked`, so it fails when `mise.lock` is stale, for example when `mise.toml` asks for a version the lockfile doesn't have.

## Markdown style

- Rumdl's default rules apply, except MD013 (line length), which is disabled. Each paragraph stays on one line, and editors handle soft wrapping.
- MD060 (table alignment) is turned on with the `aligned` style, so `mise run fmt` pads table columns to line up.
- Run `mise run fmt` before committing.

## CI

GitHub Actions runs [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) on every pull request and on pushes to `main`. CI builds on Linux only: one job on `ubuntu-latest`, with no macOS or Windows jobs and no OS matrix. The job times out after 30 minutes.

1. [`jdx/mise-action`](https://github.com/jdx/mise-action) runs `mise install --locked`, so CI installs exactly what `mise.lock` records, with no separately pinned toolchain. It switches to `--locked` on its own when it finds `mise.lock`, and includes the lockfile in its cache key. This puts the Usage CLI on `PATH` for the parser conformance test.
2. [`Swatinem/rust-cache`](https://github.com/Swatinem/rust-cache) caches the cargo registry and `target/`. Only runs on `main` save the cache (`save-if`); pull requests restore it but never write their own. `mise-action` caches the mise installs.
3. `mise run lint`, then `mise run test`. The job fails if either fails.

A new push to a pull request cancels that pull request's older run. Runs on `main` always finish. The workflow sets `CARGO_PROFILE_DEV_DEBUG=line-tables-only` to keep `target/` and the cache small, and `CARGO_TERM_COLOR=always` for readable logs.

CI never calls the live API and needs no secrets ([`testing.md`](testing.md)).

Workflows pin every action to an exact commit SHA, never a tag, with the version in a trailing comment (for example `actions/checkout@<sha> # v7.0.1`).

## Deferred

- **Release workflow:** how releases are cut (versioning, tagging, changelog, and how dist's generated workflow is triggered) is still undecided. The build targets and install channels are in [`language-and-distribution.md`](language-and-distribution.md#distribution).
