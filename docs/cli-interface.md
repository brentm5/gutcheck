# CLI interface

> **Status:** draft.

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

### `gutcheck ask`

Every `ask` subcommand takes the same core inputs:

- **The question:** a positional argument, sent to the API as `instructions`.
- **The state:** the input text the question is asked about. See [Passing state](#passing-state).
- **The answers:** options for `choice`, levels for `score`, optional yes/no clarification for `noul`.

Each subcommand sends a single question, so the request always contains exactly one entry in `questions`. Its ID is the question type (`noul`, `choice` or `score`). The ID never appears in successful output, but API errors can name it, as in `questions.noul.criteria: Field required`.

| Command               | Answer flags                                                                 | Output                | Output with `--json`                             |
| --------------------- | ---------------------------------------------------------------------------- | --------------------- | ------------------------------------------------ |
| `gutcheck ask choice` | `-o, --option key="description"` (repeatable, 1–255)                         | The chosen key        | `choice`, `probabilities`, `confidence`          |
| `gutcheck ask score`  | `-l, --level "description"` (repeatable and ordered, 1–10)                   | The score             | `score`, `legend`, `probabilities`, `confidence` |
| `gutcheck ask noul`   | `--true "what counts as yes"`, `--false "what counts as no"` (both optional) | `true` or `false`     | `noul`                                           |
| `gutcheck ask raw`    | (future) a full request body from a file or stdin                            | The full API response | —                                                |

`gutcheck ask noul` also takes `--threshold <0–1>` (default `0.5`), the cutoff at or above which the answer is `true`. See [`output-and-exit-codes.md`](output-and-exit-codes.md#noul-threshold). It can't be combined with `--json`, which reports the probability itself.

Output format and exit codes are covered in [`output-and-exit-codes.md`](output-and-exit-codes.md). Checks before sending, API error handling and retries are covered in [`api-client.md`](api-client.md).

### `gutcheck auth validate`

Checks that `TYPESAFE_API_KEY` is set and accepted by the API. It prints nothing and exits 0 if the key works, and exits 1 with an error message if it doesn't. It calls `GET /v1/models`, which needs a valid key but doesn't run the model.

### `gutcheck mcp serve`

Starts a stdio MCP server that exposes a single `ask_jev` tool, which takes a raw Jev request payload. Details are in [`mcp-server.md`](mcp-server.md).

### `gutcheck skill`

Prints a `SKILL.md` (frontmatter plus Markdown) that teaches an agent how to use the `gutcheck` CLI. An agent can run it to load the instructions into its context, or you can redirect the output into a skills directory to install it. The skill is embedded in the binary, so it always matches the installed version. Details are in [`agent-skill.md`](agent-skill.md).

## Global flags

| Flag        | Purpose                                                              |
| ----------- | -------------------------------------------------------------------- |
| `--config`  | Path to a config file to use instead of the default                  |
| `--model`   | Model to use (default `jev-latest`)                                  |
| `--timeout` | Maximum time for the whole command, including retries (e.g. `10s`)   |
| `--verbose` | On errors, print the HTTP status and raw API response body           |

`--model` and `--timeout` override the config file. Config and the API key are covered in [`configuration.md`](configuration.md).

## Passing state

The **state** is the input data that the question is asked against, such as a support ticket, a log line or a diff. It is sent as the API's `state` field.

Rules:

1. `--state "text"` passes the state inline.
2. If `--state` is not given and stdin is not a terminal, the state is read from stdin.
3. If neither is given, the command fails with an error.
4. If both are given, the command fails with an error rather than silently picking one.

A stdin that isn't a terminal only counts as given if it delivers data. Scripts, cron jobs and agents often run with a stdin that is empty or stays open and silent, and `--state` has to work there:

- With `--state`, the command waits up to 100 ms for data on stdin. Any data triggers rule 4. If stdin closes or nothing arrives in time, the flag is used. Data that arrives later than that is ignored.
- Without `--state`, the command reads stdin until it closes. If stdin closes without sending anything, that is rule 3. If it never closes, `--timeout` ends the command, because the timeout includes the wait for stdin.

Stdin must be valid UTF-8, and it is sent as is, including any trailing newline.

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

# Noul: true or false, here stricter about true than the default threshold of 0.5
if [ "$(gutcheck ask noul "The message is urgent" --threshold 0.9 < ticket.txt)" = "true" ]; then
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
