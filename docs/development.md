# Development

## Tooling

[mise](https://mise.jdx.dev/) manages the project's tool versions and task definitions. Both live in `mise.toml` at the repo root.

```bash
mise install    # install the pinned tools
mise tasks      # list available tasks
```

| Tool                                                 | Purpose                                                 | Config               |
| ---------------------------------------------------- | ------------------------------------------------------- | -------------------- |
| [rumdl](https://github.com/rvben/rumdl)              | Lint and format Markdown                                | `.rumdl.toml`        |
| Rust (with clippy and rustfmt)                       | Build, lint and format the CLI                          | `Cargo.toml`         |
| [Usage](https://usage.jdx.dev/guide/getting-started) | Validate the CLI spec and generate docs and completions | `gutcheck.usage.kdl` |

Rust is pinned in `mise.toml`; see [`language-and-distribution.md`](language-and-distribution.md) for libraries and patterns.

```bash
cargo build                   # build a debug binary
cargo run -- --version        # run it
```

## Tasks

| Task                  | What it does                                                                       |
| --------------------- | ---------------------------------------------------------------------------------- |
| `mise run lint`       | Run all linters                                                                    |
| `mise run fmt`        | Run all formatters                                                                 |
| `mise run lint:md`    | Lint Markdown with `rumdl check`                                                   |
| `mise run fmt:md`     | Format Markdown with `rumdl fmt`                                                   |
| `mise run lint:rust`  | Lint Rust with `cargo clippy` (warnings are errors)                                |
| `mise run lint:usage` | Validate `gutcheck.usage.kdl`, treating warnings as errors                         |
| `mise run fmt:rust`   | Format Rust with `cargo fmt`                                                       |
| `mise run build`      | Build the release binary at `target/release/gutcheck` with `cargo build --release` |
| `mise run test`       | Run all tests with `cargo test`                                                    |
| `mise run spec:usage` | Regenerate the Usage spec from the executable's Rust declarations                  |

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

## Markdown style

- Rumdl's default rules apply, except MD013 (line length), which is disabled. Each paragraph stays on one line, and editors handle soft wrapping.
- MD060 (table alignment) is turned on with the `aligned` style, so `mise run fmt` pads table columns to line up.
- Run `mise run fmt` before committing.

## Deferred

- **CI (GitHub Actions):** will install the pinned tools with `mise install`, then run `mise run lint` and `mise run test` on pull requests. Tests need the Usage CLI on `PATH` for parser conformance checks. The basic Rust project now exists, so this can be set up next.
- **Release workflow:** how releases are cut (versioning, tagging, changelog, and how dist's generated workflow is triggered) is still undecided. The build targets and install channels are in [`language-and-distribution.md`](language-and-distribution.md#distribution).
