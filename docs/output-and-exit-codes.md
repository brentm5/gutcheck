# Output and exit codes

> **Status:** draft.

Keep it simple to start: commands that return data print the plain answer on success, `--json` prints the full answer, and exit codes only say whether it worked.

## Exit codes

| Code | Meaning                                                                        |
| ---- | ------------------------------------------------------------------------------ |
| `0`  | The command succeeded                                                          |
| `1`  | Any error: bad input, failed local checks, API errors, network errors, timeout |

The answer itself never changes the exit code. A `noul` probability of `0.02` is still a success (`0`).

## Success output

Output goes to stdout as a single line. By default that line is the answer's main value, so scripts can use it without parsing anything.

| Command                     | Default output                                       | With `--json`                                                      |
| --------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------ |
| `gutcheck ask noul`         | `true` or `false`, using `--threshold` (default 0.5) | The answer object for the question, exactly as the API returned it |
| `gutcheck ask choice`       | The chosen option key                                | The answer object for the question, exactly as the API returned it |
| `gutcheck ask score`        | The score, which can fall between levels             | The answer object for the question, exactly as the API returned it |
| `gutcheck ask raw` (future) | The full API response                                | —                                                                  |
| `gutcheck auth validate`    | Nothing. The exit code is the answer                 | —                                                                  |
| `gutcheck skill`            | The skill Markdown                                   | —                                                                  |

### Noul threshold

`--threshold` is a slider from 0 to 1 that sets the cutoff between `true` and `false`:

```text
true   if probability >= threshold
false  otherwise
```

| `--threshold`   | Meaning                            | Probability 0.6 prints | Probability 0.02 prints |
| --------------- | ---------------------------------- | ---------------------- | ----------------------- |
| `0.01`          | Only say `false` if nearly certain | `true`                 | `true`                  |
| `0.5` (default) | Go with whichever way it leans     | `true`                 | `false`                 |
| `0.9`           | Only say `true` if very sure       | `false`                | `false`                 |
| `0.99`          | Only say `true` if nearly certain  | `false`                | `false`                 |

Moving it up makes `true` harder to reach, and moving it down makes it easier, so a script can pick the setting based on which mistake costs more. A probability of exactly 0.5 prints `true` at the default.

The plain output hides how close the call was: at the default, `0.51` and `0.99` both print `true`. Use `--json` to get the probability itself. `--threshold` can't be combined with `--json`.

### JSON output

With `--json`, the `ask` subcommands print just the answer, not the full response with `model`, `usage` and the question ID. Examples:

```json
{"type":"choice","choice":"platform","probabilities":{"billing":0.04,"platform":0.93,"other":0.03},"confidence":0.91}
```

```json
{"type":"score","score":1.8,"legend":{"0":"Calm","1":"Annoyed","2":"Angry"},"probabilities":{"0":0.02,"1":0.16,"2":0.82},"confidence":0.74}
```

```json
{"type":"noul","noul":0.91}
```

Using it in scripts:

```bash
team=$(gutcheck ask choice "Which team should handle this?" -o billing=... -o platform=... < ticket.txt)

if [ "$(gutcheck ask noul "The message is urgent" --threshold 0.9 < ticket.txt)" = "true" ]; then
  page-oncall
fi

confidence=$(gutcheck ask choice "Which team should handle this?" -o billing=... -o platform=... --json < ticket.txt | jq .confidence)
```

The `if` above treats `false` and a failed command the same way, since a failed command leaves stdout empty. Check the exit code when those have to be told apart.

## Error output

- Nothing is written to stdout, so a script never mistakes an error for an answer.
- A one-line message goes to stderr, for example `error: questions.team: choice needs at least 1 option`.
- `--verbose` adds the HTTP status and the raw API response body to stderr.

What causes each error, and which errors are retried first, is covered in [`api-client.md`](api-client.md).

## Future ideas

Not part of v1. Listed so the design leaves room for them:

- **Confidence filter:** fail, or return a fallback option, when `confidence` is below a given value.
- **Noul exit code:** turn the answer into an exit code, for use directly in `if` statements. This needs distinct exit codes first, so that "no" can't be mistaken for a failed command.
- **Distinct exit codes:** separate codes for "the request is wrong" and "try again later".
