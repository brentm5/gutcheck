# Agent skill

> **Status:** draft.

`gutcheck skill` prints an agent skill (Markdown) that teaches an agent how to use `gutcheck`. An agent can run it to load the instructions into its context on demand, or you can save the output as an installed skill.

```bash
# Read into context: an agent runs this when it needs to use gutcheck
gutcheck skill

# Install as a Claude Code skill
mkdir -p ~/.claude/skills/gutcheck
gutcheck skill > ~/.claude/skills/gutcheck/SKILL.md
```

## Why

- **Agents need to know the tool exists and how to use it.** A skill is the standard way to give them that context without pasting docs into every prompt.
- **The skill always matches the installed version.** It is embedded in the binary, so the flags it describes are the flags that binary supports. A skill copied from a repo can fall behind the CLI.
- **No extra install step.** Anyone who has the CLI already has the skill.

## How it differs from TypeSafe's skill

TypeSafe publishes its own [agent skill](https://docs.typesafe.ai/agent-skill.md) (`typesafe-ai/skills`). That skill teaches an agent to **write code** against the TypeSafe API and SDKs. The `gutcheck` skill teaches an agent to **call the `gutcheck` CLI** from a shell to get a quick decision. The two can be used together.

## Command

`gutcheck skill` only prints the skill to stdout. There is no install command: redirecting the output into an agent's skills directory is enough, and it means `gutcheck` doesn't have to track where each agent keeps its skills.

`gutcheck skill` is the one command that isn't under a command group (see [`cli-interface.md`](cli-interface.md)).

## CLI only

The skill covers the CLI only. It isn't served over MCP: agents connected through `gutcheck mcp serve` learn everything they need from the `ask_jev` tool's description and input schema (see [`mcp-server.md`](mcp-server.md)).

## Output format

Standard `SKILL.md` format: YAML frontmatter followed by Markdown, so the output works both as text read into context and as an installed skill file.

```markdown
---
name: gutcheck
description: Ask the Jev model fast, cheap, structured questions (choice, score, yes/no) from the shell using the gutcheck CLI. Use for classification, routing, rating or yes/no judgments where all possible answers are known up front.
---

# gutcheck
...
```

## Contents

The skill should stay short. It covers:

1. **When to use Jev:** quick judgments where every possible answer can be listed up front. When not to use it: open-ended generation, or anything that needs a free-text answer.
2. **Commands:** `gutcheck ask choice`, `gutcheck ask score` and `gutcheck ask noul`, with their flags and one example each.
3. **Passing state:** `--state` or stdin, and the quoting caveat for `--state "$(cat file)"`.
4. **Reading results:** output is the plain answer (`true` or `false` for `noul`, using `--threshold` as the cutoff, the option key for `choice`, the number for `score`). `--json` gives the full answer with its probabilities. Exit code `0` means success and `1` means an error, whatever the answer is.
5. **Writing good questions:** keep each question about one thing, add an "other" option to `choice`, and order `score` levels from low to high.
6. **Setup:** `TYPESAFE_API_KEY` must be set; `gutcheck auth validate` checks it.
