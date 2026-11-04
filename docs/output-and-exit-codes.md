# Output and exit codes

> **Status:** draft.

Keep it simple to start: commands that return data print JSON on success, and exit codes only say whether it worked. Scripts do any further parsing themselves (for example with `jq`).

## Exit codes

| Code | Meaning                                                                        |
| ---- | ------------------------------------------------------------------------------ |
| `0`  | The command succeeded                                                          |
| `1`  | Any error: bad input, failed local checks, API errors, network errors, timeout |

The answer itself never changes the exit code. A `noul` probability of `0.02` is still a success (`0`).

## Success output

Output goes to stdout as a single line of JSON.

| Command                              | Output                                                             |
| ------------------------------------ | ------------------------------------------------------------------ |
| `gutcheck ask choice / score / noul` | The answer object for the question, exactly as the API returned it |
| `gutcheck ask raw` (future)          | The full API response                                              |
| `gutcheck auth validate`             | Nothing. The exit code is the answer                               |
| `gutcheck skill`                     | The skill Markdown (not JSON)                                      |

The `ask` subcommands send one question, so they print just that answer, not the full response with `model`, `usage` and the question ID. Examples:

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
team=$(gutcheck ask choice "Which team should handle this?" -o billing=... -o platform=... < ticket.txt | jq -r .choice)

if [ "$(gutcheck ask noul "The message is urgent" < ticket.txt | jq '.noul > 0.7')" = "true" ]; then
  page-oncall
fi
```

## Error output

- Nothing is written to stdout, so a script never mistakes an error for an answer.
- A one-line message goes to stderr, for example `error: questions.team: choice needs at least 1 option`.
- `--verbose` adds the HTTP status and the raw API response body to stderr.

What causes each error, and which errors are retried first, is covered in [`api-client.md`](api-client.md).

## Future ideas

Not part of v1. Listed so the design leaves room for them:

- **Field output:** print a single field as plain text, for example the chosen key, so simple scripts don't need `jq`.
- **Confidence filter:** fail, or return a fallback option, when `confidence` is below a given value.
- **Noul threshold:** turn the probability into a yes/no answer, possibly as an exit code, for use directly in `if` statements.
- **Distinct exit codes:** separate codes for "the request is wrong" and "try again later".
