# Jev API reference

A summary of the parts of the TypeSafe API that `gutcheck` depends on. The upstream docs are the source of truth.

## Endpoint and auth

- `POST https://api.typesafe.ai/v1/systemone`
- `Authorization: Bearer $TYPESAFE_API_KEY` (create keys in the [TypeSafe console](https://console.typesafe.ai/keys))
- `Content-Type: application/json`

## Request

```json
{
  "state": "text or JSON object to evaluate",
  "model": "jev-latest",
  "questions": {
    "<id>": {
      "type": "choice | score | noul",
      "instructions": "the question",
      "criteria": "see below"
    }
  }
}
```

`model`, `state` and `questions` are required. `instructions` is optional on every question type.

The `criteria` field depends on the question type:

- **Choice:** an object that maps option keys to descriptions. Required, 1–255 options.
- **Score:** an ordered array of rubric levels, scored from 0. Required, 1–10 levels.
- **Noul:** optional. An object with `true` and/or `false` keys that say what counts as yes and no.

## Other endpoints

- `GET /v1/models` lists the models and aliases available to the account. It needs a valid key, so `gutcheck auth validate` uses it.

## Limits and errors

- Body at most 32 MiB, UTF-8 JSON, at most 64 levels of nesting, finite numbers only.
- Status codes: `400`, `401`, `413`, `415`, `422`, `429`, `529`. How we handle each one is in [`api-client.md`](api-client.md).

## Response

```json
{
  "model": "jev-1.13.0",
  "answers": {
    "<id>": { "type": "...", "...": "type-specific fields" }
  },
  "usage": { "input_tokens": 0, "output_tokens": 0 }
}
```

| Type   | Fields                                                                     |
| ------ | -------------------------------------------------------------------------- |
| Choice | `choice`, `probabilities`, `confidence`                                    |
| Score  | `score` (can fall between levels), `legend`, `probabilities`, `confidence` |
| Noul   | `noul` (probability from 0 to 1, no `confidence`)                          |

## Upstream docs

- [Introduction](https://docs.typesafe.ai/introduction)
- [Quick start](https://docs.typesafe.ai/introduction/quickstart.md)
- [Primitives](https://docs.typesafe.ai/primitives)
- [Errors and limits](https://docs.typesafe.ai/api/errors.md)
- [OpenAPI spec](https://api.typesafe.ai/openapi.json)
- [Docs index (llms.txt)](https://docs.typesafe.ai/llms.txt)
