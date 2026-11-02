# Design docs

Specs and decisions for `gutcheck`. Each doc should end up with a clear **Decision** section that can be implemented against.

## Docs

| Doc                                                            | Covers                                                                                            | Status    |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- | --------- |
| [`scope.md`](scope.md)                                         | Goals, non-goals, supported primitives                                                            | draft     |
| [`jev-api.md`](jev-api.md)                                     | Summary of the upstream Jev API we build on                                                       | reference |
| [`cli-interface.md`](cli-interface.md)                         | Command tree (`ask`, `auth`, `mcp`, `skill`), flags, how state, options and levels are passed     | draft     |
| [`language-and-distribution.md`](language-and-distribution.md) | Rust: libraries, code patterns, tooling, distribution                                             | draft     |
| [`output-and-exit-codes.md`](output-and-exit-codes.md)         | JSON output, exit codes 0 and 1, error output, future filtering ideas                             | draft     |
| [`configuration.md`](configuration.md)                         | Config file (TOML), `model`, `retries`, `timeout`, `base_url`, flag overrides, API key (env only) | draft     |
| [`mcp-server.md`](mcp-server.md)                               | The `ask_jev` tool: input, output, description and schema docs                                    | draft     |
| [`agent-skill.md`](agent-skill.md)                             | `gutcheck skill`: the embedded agent skill, its format and contents                               | draft     |
| [`api-client.md`](api-client.md)                               | Local checks before sending, API error handling, retries                                          | draft     |
| [`testing.md`](testing.md)                                     | Mock server (wiremock), CLI, MCP and spec tests; no live API tests                                | draft     |
| [`development.md`](development.md)                             | mise tool and task setup, Markdown linting with rumdl                                             | draft     |

## Open questions

Each doc lists its own open questions at the end.
