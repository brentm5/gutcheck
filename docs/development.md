# Development

## Tooling

[mise](https://mise.jdx.dev/) manages the project's tool versions and task definitions. Both live in `mise.toml` at the repo root.

```bash
mise install    # install the pinned tools
mise tasks      # list available tasks
```

| Tool                                    | Purpose                        | Config        |
| --------------------------------------- | ------------------------------ | ------------- |
| [rumdl](https://github.com/rvben/rumdl) | Lint and format Markdown       | `.rumdl.toml` |
| Rust (with clippy and rustfmt)          | Build, lint and format the CLI | `Cargo.toml`  |

Rust is pinned in `mise.toml`; see [`language-and-distribution.md`](language-and-distribution.md) for libraries and patterns.

```bash
cargo build                   # build a debug binary
cargo run -- --version        # run it
```

## Tasks

| Task                 | What it does                                                                       |
| -------------------- | ---------------------------------------------------------------------------------- |
| `mise run lint`      | Run all linters                                                                    |
| `mise run fmt`       | Run all formatters                                                                 |
| `mise run lint:md`   | Lint Markdown with `rumdl check`                                                   |
| `mise run fmt:md`    | Format Markdown with `rumdl fmt`                                                   |
| `mise run lint:rust` | Lint Rust with `cargo clippy` (warnings are errors)                                |
| `mise run fmt:rust`  | Format Rust with `cargo fmt`                                                       |
| `mise run build`     | Build the release binary at `target/release/gutcheck` with `cargo build --release` |
| `mise run test`      | Run all tests with `cargo test`                                                    |

`lint` and `fmt` are umbrella tasks. As new tools are added, each gets its own `lint:<name>` and `fmt:<name>` task, which the umbrella tasks depend on.

## Markdown style

- Rumdl's default rules apply, except MD013 (line length), which is disabled. Each paragraph stays on one line, and editors handle soft wrapping.
- MD060 (table alignment) is turned on with the `aligned` style, so `mise run fmt` pads table columns to line up.
- Run `mise run fmt` before committing.

## Deferred

- **CI (GitHub Actions):** will run `mise run lint` and `mise run test` on pull requests. The basic Rust project now exists, so this can be set up next.
- **Release workflow:** how releases are cut (versioning, tagging, changelog, and how dist's generated workflow is triggered) is still undecided. The build targets and install channels are in [`language-and-distribution.md`](language-and-distribution.md#distribution).
