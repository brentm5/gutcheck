# MCP server

> **Status:** draft.

`gutcheck mcp serve` runs `gutcheck` as an MCP server over stdio, so agents (Claude Code and others) can call Jev as a tool. It exposes one tool, `gutcheck_ask`.

## Decision: one tool, `gutcheck_ask`

The server exposes a single tool, `gutcheck_ask`. It takes a raw Jev request payload and returns the answers.

We chose this over one tool per question type (`ask_choice`, `ask_score`, `ask_noul`) because:

- **Agents handle JSON well.** Unlike shell scripts, they don't need flags to build a request.
- **Multiple questions in one call.** The API evaluates questions in parallel against the same state, so an agent can ask several related questions for about the cost of one. Per-type tools would lose that.
- **Less to maintain.** The tool mirrors the API, so new question types or fields work without changes to the tool schema.
- **Shared code path.** It is the MCP equivalent of the future `gutcheck ask raw` command, and both can share the same request handling.

## Tool: `gutcheck_ask`

### Input

The same shape as the API request body (see [`jev-api.md`](jev-api.md)), without any credentials.

The agent never sees or sends the API key. The `gutcheck mcp serve` process reads `TYPESAFE_API_KEY` from its own environment, set once when the server is registered, and adds the `Authorization` header to each request itself. This keeps the key out of the model's context and transcripts. The key is read once at startup: without it, `gutcheck mcp serve` exits with the usual missing-key error instead of starting. For example, in Claude Code:

```json
{
  "mcpServers": {
    "gutcheck": {
      "command": "gutcheck",
      "args": ["mcp", "serve"],
      "env": { "TYPESAFE_API_KEY": "..." }
    }
  }
}
```

| Field       | Type                    | Required | Notes                                                                |
| ----------- | ----------------------- | -------- | -------------------------------------------------------------------- |
| `state`     | string, object or array | yes      | The input data the questions are asked about                         |
| `questions` | object                  | yes      | Keys are question IDs; values are `{ type, instructions, criteria }` |
| `model`     | string                  | no       | Overrides the configured model for this call                         |

Example call:

```json
{
  "state": "Payment page throws 500 after checkout. Third time this week!",
  "questions": {
    "team": {
      "type": "choice",
      "instructions": "Which team should handle this?",
      "criteria": {
        "billing": "Payments, invoices, refunds",
        "platform": "Infra, outages, server errors",
        "other": "Anything else"
      }
    },
    "urgent": {
      "type": "noul",
      "instructions": "The message is urgent"
    }
  }
}
```

`model` is optional. When the agent leaves it out, or sends a blank string, the server uses the configured model: `--model` on `gutcheck mcp serve`, then the config file, then `jev-latest`.

### Output

Only the `answers` object from the API response, keyed by question ID. `model` and `usage` are dropped to save agent tokens.

```json
{
  "team": { "type": "choice", "choice": "platform", "probabilities": { "...": 0.0 }, "confidence": 0.91 },
  "urgent": { "type": "noul", "noul": 0.88 }
}
```

### Validation and errors

The server runs the same basic local checks as the CLI before sending a request, and the API makes the final call. Each call is one operation: `timeout` and `retries` apply per call, as for one CLI command. Local check failures and API errors are both returned as MCP tool errors with a message naming the field, so the agent can fix its request and try again. With `--verbose`, the server also prints API errors in full (status and raw body) to stderr, never to the agent. See [`api-client.md`](api-client.md).

### Tool documentation

The agent learns how to use `gutcheck_ask` only from what the server sends in `tools/list`: the tool description and the input schema. There is no other channel (the agent skill is CLI-only and not served over MCP), so these two must be enough on their own.

Both are loaded into every connected agent's context, so they should be complete but short: aim for a few hundred tokens in total.

#### Description

Draft text for the tool description:

```text
Ask Jev, a fast and cheap model that answers structured questions about some input
("state"). Every possible answer must be defined up front, so use it for
classification, routing, rating and yes/no judgments, not for open-ended text.

Question types (set "type" on each question):
- choice: pick one option. criteria = {"option_key": "when this option applies", ...}
  (1-255 options). Returns choice, probabilities, confidence.
- score: place the state on an ordered rubric. criteria = ["lowest level", ..., "highest level"]
  (1-10 levels). Returns score (can fall between levels), legend, probabilities, confidence.
- noul: probability (0-1) that a statement is true. criteria is optional:
  {"true": "what counts as yes", "false": "what counts as no"}. Returns noul.

Tips:
- Ask several questions in one call; they run in parallel against the same state.
- Keep each question about one thing. Combine results yourself.
- Add an "other" option to choice questions when the list may not cover every input.

Returns the answers keyed by your question IDs. Errors name the invalid field so
you can fix the request and retry.
```

#### Input schema

The input schema documents each field, so agents see the rules again at the point where they fill in the field. With rmcp, the schema is derived from the Rust input types, and doc comments on fields become the schema `description`s.

| Field                       | Schema description                                                                 |
| --------------------------- | ---------------------------------------------------------------------------------- |
| `state`                     | The input the questions are about: text, or a JSON object or array                 |
| `questions`                 | Questions keyed by an ID you choose; answers come back under the same IDs          |
| `questions.*.type`          | `choice`, `score` or `noul`                                                        |
| `questions.*.instructions`  | The question or statement to evaluate                                              |
| `questions.*.criteria`      | Options (choice), ordered levels (score), or optional true/false meanings (noul)   |
| `model`                     | Optional. Overrides the configured model for this call                             |

The schema lists the three supported types, but the server still passes unknown types through to the API (see [`api-client.md`](api-client.md)).

#### Keeping it accurate

- A snapshot test covers the tool description and the generated input schema, so any change to what agents see shows up in review.
- The question-writing tips match the ones in the agent skill ([`agent-skill.md`](agent-skill.md)). When one changes, update the other.
