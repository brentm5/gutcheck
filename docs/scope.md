# Scope

## Goals

1. **CLI first.** One command per question type. Input comes from args or stdin. Output is the plain answer, with `--json` for the full details, and the exit code says whether the command worked, so the tool composes cleanly in scripts and pipelines. Common operations must not require `jq` or another JSON parser.
2. **MCP server mode.** `gutcheck mcp serve` launches a stdio MCP server with a single `gutcheck_ask` tool that takes a raw Jev request, so agents (Claude Code and others) can call Jev directly.
3. **Agent skill.** `gutcheck skill` prints a skill that teaches agents how to use the CLI, either read into context on demand or saved as an installed skill.
4. **Small surface area.** The first version supports the three text primitives only:

| Primitive  | Asks                              | Returns                                           |
| ---------- | --------------------------------- | ------------------------------------------------- |
| **Choice** | Which of these options fits?      | selected option, probabilities, confidence        |
| **Score**  | Where does this land on a rubric? | score along the levels, probabilities, confidence |
| **Noul**   | Is this statement true?           | probability from 0 to 1                           |

## Non-goals (for now)

- Mixing several questions in one call from the CLI. The API supports it, but it may come later.
- Non-text inputs.
- Being a general-purpose LLM client.

## Decision: text interface

The CLI design covers the three text question types: `noul`, `choice` and `score`. Image inputs and `box` questions are outside this design review. Whether to support them later is undecided; they do not set requirements for the text interface.
