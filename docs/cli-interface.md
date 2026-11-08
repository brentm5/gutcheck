# CLI interface

> **Status:** draft.

## Usage specification

The Rust declarations in [`src/cli.rs`](../src/cli.rs) define the executable CLI with [usage-rs](https://usage.jdx.dev/rust/quickstart) 7.3.0. The same declarations generate [`gutcheck.usage.kdl`](../gutcheck.usage.kdl), including flag constraints, output modes and package metadata from Cargo. Regenerate it with `mise run spec:usage`; do not edit it by hand. A test compares the checked-in spec with the executable's `__usage_spec__` output.

The generated spec can produce documentation and shell completions for the implemented commands: `ask choice`, `ask noul`, `ask score` and `auth validate`. Planned commands in the tree below are documented here until their Rust declarations and handlers exist. Input selection, duplicate option-key checks, API validation and output formatting remain application responsibilities.

Run `mise run lint:usage` to validate it. See [`development.md`](development.md#usage-specification) for artifact generation commands.

## Command structure

Every command sits under a command group. There are no top-level verbs.

```text
gutcheck
├── ask                 Ask Jev a question about some input text
│   ├── choice          Pick one option from a list
│   ├── score           Place the input on an ordered rubric
│   ├── noul            Probability (0–1) that a statement is true
│   └── raw             (future) Send a full multi-question request
├── auth
│   └── validate        Check that the API key works
├── mcp
│   └── serve           Run as an MCP server over stdio
└── skill               Print an agent skill describing how to use gutcheck
```

`gutcheck skill` is the one exception to the command-group rule. See [`agent-skill.md`](agent-skill.md).

**Decision:** keep a separate `ask` subcommand for each text question type, with shared conventions for the question, state, output format and operational flags. Each subcommand documents its own answer-specific arguments in `--help`.

### `gutcheck ask`

Every `ask` subcommand takes the same core inputs:

- **The question:** a positional argument, sent to the API as `instructions`.
- **The state:** the input text the question is asked about. See [Passing state](#passing-state).
- **The answers:** options for `choice`, levels for `score`, optional yes/no clarification for `noul`.

Each subcommand sends a single question, so the request always contains exactly one entry in `questions`. Its ID is the question type (`noul`, `choice` or `score`). The ID never appears in successful output, but API errors can name it, as in `questions.noul.criteria: Field required`.

| Command               | Answer flags                                                                 | Output                | Output with `--json`                             |
| --------------------- | ---------------------------------------------------------------------------- | --------------------- | ------------------------------------------------ |
| `gutcheck ask choice` | `-o, --option key` or `key="description"` (repeatable, 1–255)                | The chosen key        | `choice`, `probabilities`, `confidence`          |
| `gutcheck ask score`  | `-l, --level "description"` (repeatable and ordered, 1–10)                   | The score             | `score`, `legend`, `probabilities`, `confidence` |
| `gutcheck ask noul`   | `--true "what counts as yes"`, `--false "what counts as no"` (both optional) | The probability       | `noul`                                           |
| `gutcheck ask raw`    | (future) a full request body from a file or stdin                            | The full API response | —                                                |

`gutcheck ask noul` also takes `--boolean` to print `true` or `false`. In this mode, `--threshold <0–1>` optionally sets the cutoff (default `0.5`); the answer is `true` only when the unrounded probability is strictly greater than the cutoff. `--threshold` requires `--boolean`. `--boolean` and `--json` are mutually exclusive. See [`output-and-exit-codes.md`](output-and-exit-codes.md) for formatting and threshold rules. Exit codes report command success or failure for all question types; scripts compare stdout to act on an answer.

Output format and exit codes are covered in [`output-and-exit-codes.md`](output-and-exit-codes.md). Checks before sending, API error handling and retries are covered in [`api-client.md`](api-client.md).

#### Choice options

**Decision:** descriptions are optional. `-o billing` asks Jev to interpret the option by its name alone; `-o billing="Payments and refunds"` adds a description. Both forms return `billing` if that option is selected, and they may be mixed in one command. In the request, a bare key has a `null` description, which the API interprets by the key alone.

**Parsing decision:** split an option at its first `=`. Everything after that separator belongs to the description, including additional `=` characters. With no separator, the entire value is the option key. Empty keys and duplicate keys are CLI usage errors, including a duplicate supplied once as a bare key and once with a description; an earlier definition is never silently replaced.

```bash
gutcheck ask choice "Which team owns this?" \
  -o billing -o platform -o other < ticket.txt
# => platform
```

For example, `-o 'billing=Payments=refunds'` defines the key `billing` with description `Payments=refunds`. `-o billing -o billing="Payments"` fails with a duplicate-key error.

#### Score levels

**Decision:** `score` returns the API's probability-weighted numeric score, without snapping to a level or rescaling. Plain output uses the shared three-decimal formatting rule; `--json` retains the answer without display rounding. Repeated `-l, --level` descriptions define the rubric in order: the first level is `0`, the second is `1`, and so on. There are 1–10 levels; numeric text inside a description does not change its assigned position.

For `-l Calm -l Annoyed -l Angry`, the scale is `0` to `2`. If the model assigns probabilities of `0`, `0.2` and `0.8` to those levels, the weighted score is `1.8`. Plain output is that number; `--json` also provides the legend, probabilities and confidence. To select one named category instead, use `choice`.

### `gutcheck auth validate`

Checks that `TYPESAFE_API_KEY` is set and accepted by the API. It prints nothing and exits 0 if the key works, and exits 1 with an error message if it doesn't. It calls `GET /v1/models`, which needs a valid key but doesn't run the model.

### `gutcheck mcp serve`

Starts a stdio MCP server that exposes a single `ask_jev` tool, which takes a raw Jev request payload. Details are in [`mcp-server.md`](mcp-server.md).

### `gutcheck skill`

Prints a `SKILL.md` (frontmatter plus Markdown) that teaches an agent how to use the `gutcheck` CLI. An agent can run it to load the instructions into its context, or you can redirect the output into a skills directory to install it. The skill is embedded in the binary, so it always matches the installed version. Details are in [`agent-skill.md`](agent-skill.md).

## Global flags

| Flag        | Purpose                                                                                    |
| ----------- | ------------------------------------------------------------------------------------------ |
| `--config`  | Path to a config file to use instead of the default                                        |
| `--model`   | Model to use (default `jev-latest`)                                                        |
| `--timeout` | Maximum time for the whole operation, including waiting for stdin and retries (e.g. `10s`) |
| `--verbose` | On errors, print the HTTP status and raw API response body                                 |

`--model` and `--timeout` override the config file. Config and the API key are covered in [`configuration.md`](configuration.md).

## Passing state

The **state** is the input data that the question is asked against, such as a support ticket, a log line or a diff. It is sent as the API's `state` field.

**Decision:** an explicit `--state` takes precedence over stdin.

Rules:

1. `--state "text"` passes the state inline. When supplied, stdin is left unread and there is no wait to check it for data.
2. If `--state` is not given and stdin is not a terminal, the state is read from stdin.
3. If neither is given, the command fails with an error.
4. If both are given, `--state` is used and stdin is left unread.

Without `--state`, the command reads nonterminal stdin until it closes. If stdin closes without sending anything, that is rule 3. If it never closes, `--timeout` ends the command, because the timeout includes the wait for stdin.

An explicit `--state ""` supplies an empty text value. Empty stdin does not supply a value and triggers the missing-state error.

Scripts, cron jobs and agents may inherit stdin that contains unrelated data or stays open and silent. With `--state`, neither affects the command. A `while read` loop can call `gutcheck ask noul "..." --state "$line"` without redirecting stdin.

Stdin must be valid UTF-8, and it is sent as is, including any trailing newline. Reading stops as soon as it goes over the API's 32 MiB body limit, and the command fails with the size error.

`--state "$(cat file.txt)"` works, but stdin is usually the better choice for files:

- It needs no quoting. Without quotes around `$(...)`, the shell splits the text into separate arguments.
- It avoids the operating system's limit on argument length for large inputs.
- It composes with pipelines: `git diff | gutcheck ask noul "This change touches auth code"`.

## Examples

```bash
export TYPESAFE_API_KEY=...

# Choice: route a ticket
echo "Payment page throws 500 after checkout" \
  | gutcheck ask choice "Which team should handle this?" \
      -o billing="Payments, invoices, refunds" \
      -o platform="Infra, outages, server errors" \
      -o other="Anything else"
# => platform

# Score: rate against an ordered rubric
gutcheck ask score "How frustrated is the customer?" \
  --state "This is the third time I've asked. Fix it." \
  -l "Calm" -l "Annoyed" -l "Angry"
# => 1.8

# Noul: the probability that the statement is true
gutcheck ask noul "The message is urgent" < ticket.txt
# => 0.91

# Boolean mode: stricter about true than the default threshold of 0.5
urgent=$(gutcheck ask noul "The message is urgent" --boolean --threshold 0.9 < ticket.txt) || exit 1
if [ "$urgent" = "true" ]; then
  page-oncall
fi

# --json: the full answer, including the probability
gutcheck ask noul "The message is urgent" --json < ticket.txt
# => {"type":"noul","noul":0.91}

# Check the API key, then start the MCP server
gutcheck auth validate
gutcheck mcp serve
```

## Reusable question definitions (not in v1)

With flags alone, every call has to spell out all of its options or levels. That's fine for one-off scripts. It gets awkward when:

- **The same question is asked in many places.** For example, a ticket-routing question with 8 teams is used by three scripts. Each script copies the same 8 `-o` flags, and the copies drift apart when a team is added.
- **Descriptions are long.** Good option descriptions often run to a sentence or two, which is hard to read and quote inside a shell command.
- **Agents generate the call.** Writing a small file is often easier for an agent than building a long command line with nested quotes.

A question definition file would store the question and its answers once:

```yaml
# questions/route-ticket.yaml
type: choice
instructions: Which team should handle this?
criteria:
  billing: Payments, invoices, refunds
  platform: Infra, outages, server errors
  other: Anything else
```

```bash
gutcheck ask choice --file questions/route-ticket.yaml < ticket.txt
```

Flags could still override or extend what the file sets. This is the same shape as one entry in the API's `questions` object, so it would lead naturally into `gutcheck ask raw`, which would take many questions at once.

**Decision:** v1 ships with flags only. `--file` will be added when a script actually needs to reuse a question. If it's added, the file format is still to be decided (YAML reads well for long descriptions).
