# Configuration

> **Status:** draft.

Settings live in one config file, so scripts, agents and the MCP server all behave the same way without repeating flags.

## Config file

- **Format:** TOML, parsed with the `toml` crate. It's Rust's standard config format, matches the repo's other config files (`Cargo.toml`, `mise.toml`, `.rumdl.toml`), and keeps value types explicit.
- **Location:** `$XDG_CONFIG_HOME/gutcheck/config.toml`, which defaults to `~/.config/gutcheck/config.toml`.
- **Override:** `--config <path>` reads a different file instead. If the file given with `--config` doesn't exist, the command fails with an error.
- **Optional:** if the file doesn't exist, the defaults below are used.
- **Default location:** empty or relative `XDG_CONFIG_HOME` and `HOME` values are ignored, so a config file can't come from the current directory. If a parent of the path isn't a directory, the file counts as missing.
- **Unknown settings are errors,** so a typo like `base-url` can't silently send the API key to the default server.

```toml
# ~/.config/gutcheck/config.toml
model = "jev-latest"

retries = 2       # retries after the first attempt; 0 disables retrying
timeout = "30s"   # total time for the whole command, including all retries

# base_url = "https://api.typesafe.ai"   # only change this for a proxy or a test server
```

## Settings

| Setting    | Type                                  | Default                     | Meaning                                                                              |
| ---------- | ------------------------------------- | --------------------------- | ------------------------------------------------------------------------------------ |
| `model`    | string                                | `jev-latest`                | Model sent with every request                                                        |
| `retries`  | integer ≥ 0                           | `2`                         | Maximum number of retries after the first attempt                                    |
| `timeout`  | duration (`"500ms"`, `"10s"`, `"1m"`) | `"30s"`                     | Maximum time for the whole operation: every attempt plus every wait between attempts |
| `base_url` | URL                                   | `"https://api.typesafe.ai"` | Where requests are sent                                                              |

## Base URL

`base_url` changes where every request goes. API paths are added to it, so `POST /v1/systemone` goes to `<base_url>/v1/systemone` and `gutcheck auth validate` calls `<base_url>/v1/models`.

- **Uses:** pointing tests at a local mock server ([`testing.md`](testing.md)), or sending requests through a proxy or gateway.
- **Format:** must be an absolute `http` or `https` URL, or loading the config fails. A trailing slash is ignored. It may include a path prefix (for example `https://gateway.example.com/typesafe`). It can't include a username, password, query or fragment.
- **The API key goes with it.** `TYPESAFE_API_KEY` is sent to whatever `base_url` points at, so only set it to a server you trust.

## Retries and timeout

The two settings work together. Whichever limit is reached first wins.

- **`timeout` is a hard limit on the whole command.** Once it's reached, the request in progress is cancelled and the command fails with a timeout error. With `retries = 100` and `timeout = "10s"`, the command runs for at most 10 seconds.
- **`retries` caps the number of attempts.** With `retries = 2`, there are at most 3 attempts, even if time remains.
- **The wait between attempts grows exponentially.** It starts at 0.5 s and doubles each time, up to 5 s, with some random jitter so many clients don't retry at the same moment. These backoff values are fixed and can't be configured.
- **`Retry-After` is honored.** If the API asks us to wait longer than the normal backoff, we wait that long instead.
- **We don't start a wait that can't finish.** If the next wait would go past the timeout, the command stops right away and reports the last error.

Which errors are retried (408, 429, 5xx, network errors) is covered in [`api-client.md`](api-client.md).

Example with `retries = 100` and `timeout = "10s"`, where every attempt fails quickly:

| Attempt | Wait before it                           | Time elapsed (approx.) |
| ------- | ---------------------------------------- | ---------------------- |
| 1       | —                                        | 0 s                    |
| 2       | 0.5 s                                    | 0.5 s                  |
| 3       | 1 s                                      | 1.5 s                  |
| 4       | 2 s                                      | 3.5 s                  |
| 5       | 4 s                                      | 7.5 s                  |
| —       | next wait (5 s) would pass 10 s, so stop | 7.5 s                  |

## Overrides

Two settings can be overridden per command with a flag:

| Setting   | Flag        |
| --------- | ----------- |
| `model`   | `--model`   |
| `timeout` | `--timeout` |

`retries` and `base_url` are set only in the config file. There are no environment variables for settings.

Order of precedence, highest first: **flag → config file → default.**

## API key

The API key comes only from the `TYPESAFE_API_KEY` environment variable, the same one TypeSafe's SDKs use. It can't be set in the config file, so the key is never written to disk in plain text.

Later, the key could also be read from the operating system's secure store: Keychain on macOS, and the Secret Service API (GNOME Keyring, KWallet) on Linux. That isn't part of v1.

## Not in v1

- **Separate MCP timeout:** `gutcheck mcp serve` uses the same `timeout` setting as every other command. If agents need a shorter one, pass `--timeout` in the MCP server's registration (for example `"args": ["mcp", "serve", "--timeout", "10s"]`).
- **`gutcheck config show`:** no command for printing the merged settings.
- **API key in the OS secure store:** see [API key](#api-key).
