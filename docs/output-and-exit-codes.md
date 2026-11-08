# Output and exit codes

> **Status:** draft.

Keep it simple to start: commands that return data print the plain answer on success, `--json` prints the full answer, and exit codes only say whether it worked.

`noul` prints its probability by default. `--boolean` selects `true` or `false` output, with an optional `--threshold`.

## Exit codes

| Code | Meaning                                                                        |
| ---- | ------------------------------------------------------------------------------ |
| `0`  | The command succeeded                                                          |
| `1`  | Any error: bad input, failed local checks, API errors, network errors, timeout |

The answer itself never changes the exit code. A `noul` probability of `0.02` is still a success (`0`), as is printing `false` with `--boolean`.

**Decision:** omit answer-dependent exit-code modes. Scripts can compare the printed boolean, while the same success/error status contract applies to all question types.

## Success output

Output goes to stdout as a single line. By default that line is the answer's main value, so scripts can use it without parsing anything.

**Design goal:** common operations must not need `jq`: read the chosen key from `choice`, the numeric score from `score`, the probability from `noul`, or a thresholded `true`/`false` from `noul --boolean`. `--json` provides the full answer when its additional fields are needed.

| Command                     | Default output                           | With `--json`                      |
| --------------------------- | ---------------------------------------- | ---------------------------------- |
| `gutcheck ask noul`         | The probability from 0 to 1              | The answer object for the question |
| `gutcheck ask choice`       | The chosen option key                    | The answer object for the question |
| `gutcheck ask score`        | The numeric probability-weighted score   | The answer object for the question |
| `gutcheck ask raw` (future) | The full API response                    | —                                  |
| `gutcheck auth validate`    | Nothing. The exit code is the answer     | —                                  |
| `gutcheck skill`            | The skill Markdown                       | —                                  |

**Score decision:** rubric positions are numbered from `0`, and the API's weighted average is returned without snapping to a level or rescaling. Fractional results are valid; plain output follows the numeric formatting rule below. A score summarizes the level probabilities; it is distinct from the confidence field. See [`cli-interface.md`](cli-interface.md#score-levels) for the input rubric and an example.

### Plain numeric formatting

Probability and score output use ordinary decimal notation, rounded to at most **three decimal places**, with trailing fractional zeros and an unnecessary decimal point removed. Exponent notation is never used for plain numbers. Zero, including a rounded negative zero, is printed as `0`. Examples: `0.912345678` becomes `0.912`, `0.9126` becomes `0.913`, `1.0` becomes `1`, `1.8000` becomes `1.8`, and `0.0000004` becomes `0`.

Rounding applies only to plain numeric display. Boolean decisions compare the unrounded API value, and `--json` retains the answer without display rounding. For example, probability `0.5004` prints `0.5` normally but prints `true` with `--boolean` at the default cutoff. This is a CLI formatting decision, not a precision guarantee from the API.

### Noul threshold

**Decision:** plain output preserves the primary API value for each text question type. For `noul`, boolean output is an explicit conversion selected with `--boolean`; the probability remains available without parsing JSON.

`--boolean` prints `true` or `false`. Its optional `--threshold` is a number from 0 to 1, default `0.5`, that sets the cutoff:

```text
true   if probability > threshold
false  otherwise
```

| `--threshold`   | Meaning                            | Probability 0.6 prints | Probability 0.02 prints |
| --------------- | ---------------------------------- | ---------------------- | ----------------------- |
| `0.01`          | Only say `false` if nearly certain | `true`                 | `true`                  |
| `0.5` (default) | Go with whichever way it leans     | `true`                 | `false`                 |
| `0.9`           | Only say `true` if very sure       | `false`                | `false`                 |
| `0.99`          | Only say `true` if nearly certain  | `false`                | `false`                 |

Moving it up makes `true` harder to reach, and moving it down makes it easier, so a script can pick the setting based on which mistake costs more. A probability exactly equal to the threshold prints `false`, including `0.5` at the default cutoff. The strict comparison applies to custom thresholds too: `0.7` with `--threshold 0.7` prints `false`. At threshold `0`, only a probability greater than `0` prints `true`; at threshold `1`, the result is always `false`.

Boolean output hides how close the call was: at the default cutoff, `0.51` and `0.99` both print `true`. Omit `--boolean` to get the probability itself. `--threshold` requires `--boolean`; supplying it alone is a usage error.

```bash
gutcheck ask noul "The message is urgent" < ticket.txt
# => 0.91

gutcheck ask noul "The message is urgent" --boolean < ticket.txt
# => true

gutcheck ask noul "The message is urgent" --boolean --threshold 0.95 < ticket.txt
# => false
```

### JSON output

With `--json`, the `ask` subcommands print just the answer, not the full response with `model`, `usage` and the question ID. Its fields and values match the API's, including any fields added later, but key order and number formatting may differ: `0.910` can print as `0.91`. Examples:

```json
{"type":"choice","choice":"platform","probabilities":{"billing":0.04,"platform":0.93,"other":0.03},"confidence":0.91}
```

```json
{"type":"score","score":1.8,"legend":{"0":"Calm","1":"Annoyed","2":"Angry"},"probabilities":{"0":0.02,"1":0.16,"2":0.82},"confidence":0.74}
```

```json
{"type":"noul","noul":0.91}
```

**Decision:** `--boolean` and `--json` are mutually exclusive. Supplying both is a usage error: one requests a derived boolean and the other requests the API answer.

Using it in scripts:

```bash
team=$(gutcheck ask choice "Which team should handle this?" -o billing=... -o platform=... < ticket.txt)

# Capture the command's status separately from its true/false result.
urgent=$(gutcheck ask noul "The message is urgent" --boolean --threshold 0.9 < ticket.txt) || exit 1
if [ "$urgent" = "true" ]; then
  page-oncall
fi

# Full answer fields remain available through --json.
confidence=$(gutcheck ask choice "Which team should handle this?" -o billing=... -o platform=... --json < ticket.txt | jq .confidence)
```

A variable assignment containing command substitution has the command's exit status. The example stops on a failed request before comparing the boolean, so `false` is a successful answer and execution failures remain errors.

## Error output

- Nothing is written to stdout, so a script never mistakes an error for an answer.
- A one-line message goes to stderr, for example `error: questions.team: choice needs at least 1 option`. Command-line usage errors follow the same rule: the parser's message, uncoloured and without its usage line or `--help` pointer, with its line breaks as spaces. A tip such as `tip: a similar argument exists: '--state'` stays at the end of the line. Help shown because a command is missing is not an error message and keeps its full text.
- `--verbose` adds the HTTP status and the raw API response body to stderr.

What causes each error, and which errors are retried first, is covered in [`api-client.md`](api-client.md).

## Future ideas

Not part of v1. Listed so the design leaves room for them:

- **Confidence filter:** fail, or return a fallback option, when `confidence` is below a given value.
- **Distinct exit codes:** separate codes for "the request is wrong" and "try again later".
