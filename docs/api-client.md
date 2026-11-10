# API client: validation and error handling

> **Status:** draft.

Every `gutcheck ask` subcommand, `gutcheck ask raw` and the `ask_jev` MCP tool send requests through the same client code. This doc covers what that code checks before it sends a request, and how it handles errors from the API.

## Principle

**Check the basics locally, and treat the API as the final authority.**

- Local checks catch common mistakes early, with clear messages, and without spending a network round trip or API usage.
- Local checks are deliberately shallow. We do not try to copy every rule the API enforces. If our checks pass and the API still rejects the request, we report the API's error clearly.
- Local checks never reject something the API would accept. When in doubt, send the request.

## Local checks

These run before any request is sent. Limits come from TypeSafe's [OpenAPI spec](https://api.typesafe.ai/openapi.json) and [errors page](https://docs.typesafe.ai/api/errors.md).

### Environment

- `TYPESAFE_API_KEY` is set and not empty.

### Request

| Check        | Rule                                                                                                                               |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| `model`      | Present (filled in from config if the caller didn't set it or left it blank; see [`configuration.md`](configuration.md#overrides)) |
| `state`      | Present. A string, object or array, which may be empty                                                                             |
| `questions`  | An object with at least one entry                                                                                                  |
| Question IDs | Non-empty strings                                                                                                                  |
| Body size    | At most 32 MiB once encoded                                                                                                        |

### Questions

| Type         | Rule                                                                      |
| ------------ | ------------------------------------------------------------------------- |
| `choice`     | `criteria` is an object with 1–255 options                                |
| `score`      | `criteria` is an array with 1–10 levels                                   |
| `noul`       | `criteria`, if present, is an object with only `true` and/or `false` keys |
| Unknown type | Not checked locally. Passed to the API as is                              |

Passing unknown types through keeps `ask_jev` and `gutcheck ask raw` working when TypeSafe adds new question types (the API already has `box`, for images).

Things we leave to the API: nesting depth, the model's context length, whether the model name exists, and anything else not listed above.

### Reporting local check failures

- **CLI:** print a one-line message to stderr naming the field, for example `error: questions.team: choice needs at least 1 option`. Exit with code 1.
- **MCP:** return a tool error with the same message, so the agent can fix the request and try again.

## API errors

TypeSafe returns these errors. The response bodies are defined in the OpenAPI spec.

| Status                                           | Meaning                                                                                                                         | Body                                                       | What we do                                                                                                                 |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `400`                                            | Breaks a request rule (invalid UTF-8, nesting, non-finite number) or exceeds the model's context length (`max_tokens_exceeded`) | `{ "detail": string or object }`                           | Report `detail`. Do not retry                                                                                              |
| `401`                                            | Missing or invalid API key                                                                                                      | —                                                          | Report "invalid or missing API key" and point to `gutcheck auth validate` (except on `auth validate` itself). Do not retry |
| `413`                                            | Body over 32 MiB                                                                                                                | —                                                          | Report the size limit. Do not retry. Local checks should normally catch this first                                         |
| `415`                                            | Unsupported compression                                                                                                         | —                                                          | Should never happen, since we control encoding. Report as an internal error                                                |
| `422`                                            | Validation failed                                                                                                               | `{ "detail": [{ "loc", "msg", "type", "input", "ctx" }] }` | Report each entry as `<loc>: <msg>`, joined with `; `, e.g. `questions.urgency.criteria: Field required`. Do not retry     |
| `408`, `429`, `5xx` (including `529 Overloaded`) | Timeout, rate limit, or server problem                                                                                          | —                                                          | Retry with backoff (see below), then report                                                                                |
| Network error or timeout                         | —                                                                                                                               | —                                                          | Retry with backoff, then report                                                                                            |

Redirects aren't followed. A `3xx` is reported as an unexpected response and isn't retried, so the API key is never sent to another location, and a `2xx` from a login page can't pass for a real answer.

For `422`, the `loc` path starts with `"body"`. We drop that prefix and join the rest with dots so the path matches the field names the caller used.

If a `400` or `422` body doesn't have the documented shape, or its `detail` or every `msg` is empty, the error is reported as `unexpected API response (HTTP <status>)`. So is a `2xx` response without a usable answer, using the message `unexpected API response: it has no usable answer`. Neither is retried, and `--verbose` shows the body.

**Not implemented yet:** the pass-through for unknown question types arrives with `ask_jev`.

With `--verbose`, errors are printed in full (status code and raw body), which helps when the API's error doesn't fit the mapping above.

## Retries

Only two settings can be configured: `retries` (default 2) and `timeout` (default 30 s, covering the whole operation including retries). Both are set in the config file, and `timeout` can also be overridden with `--timeout`. See [`configuration.md`](configuration.md) for how they interact.

The backoff between attempts is fixed and matches TypeSafe's official SDKs:

| Behavior                                 | Value                                                 |
| ---------------------------------------- | ----------------------------------------------------- |
| Initial backoff                          | 0.5 s, doubling each time                             |
| Maximum backoff                          | 5 s                                                   |
| Jitter                                   | Up to 25% subtracted from each delay                  |
| `Retry-After` / `retry-after-ms` headers | Honored. `Retry-After` can be seconds or an HTTP date |

If the next delay would go past the timeout, we stop and report the last error. If the timeout is reached during a request, that request is cancelled.

## Exit codes and output

Every error exits with code 1 and writes a message to stderr, with nothing on stdout. See [`output-and-exit-codes.md`](output-and-exit-codes.md).
