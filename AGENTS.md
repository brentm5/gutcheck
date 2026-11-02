# AGENTS.md

Guidance for AI agents working in this repo.

## Project

`gutcheck` is a command-line tool and stdio MCP server for asking TypeSafe's Jev model structured questions (choice, score, noul). See [`README.md`](README.md) for the overview.

## Specs are the source of truth

All design decisions live in [`docs/`](docs/). Start with the index in [`docs/README.md`](docs/README.md), then read the docs for the area you're working on before writing code.

| Working on                          | Read                                                                     |
| ----------------------------------- | ------------------------------------------------------------------------ |
| Commands, flags, input handling     | [`docs/cli-interface.md`](docs/cli-interface.md)                         |
| Request checks, API errors, retries | [`docs/api-client.md`](docs/api-client.md)                               |
| Config file and settings            | [`docs/configuration.md`](docs/configuration.md)                         |
| MCP server                          | [`docs/mcp-server.md`](docs/mcp-server.md)                               |
| `gutcheck skill`                    | [`docs/agent-skill.md`](docs/agent-skill.md)                             |
| Upstream Jev API                    | [`docs/jev-api.md`](docs/jev-api.md)                                     |
| Rust libraries, patterns, packaging | [`docs/language-and-distribution.md`](docs/language-and-distribution.md) |
| Tests and mocking the API           | [`docs/testing.md`](docs/testing.md)                                     |
| Tooling and tasks                   | [`docs/development.md`](docs/development.md)                             |

Rules:

- **Follow the specs.** If a spec is unclear, or doesn't cover what you need, ask rather than guess.
- **Don't build open questions.** Items listed under "Open questions" in a doc are undecided. Ask before implementing them.
- **Keep docs in sync.** If a decision changes while you work, update the relevant doc in the same change.
- **Scope.** Don't add features that aren't in the specs.

## Tooling

[mise](https://mise.jdx.dev/) manages tool versions and tasks (`mise.toml`).

```bash
mise install     # install pinned tools
mise run lint    # run all linters
mise run fmt     # run all formatters
mise run test    # run all tests
```

Run `mise run fmt`, `mise run lint` and `mise run test` before finishing a change. All must pass.
