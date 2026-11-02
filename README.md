# gutcheck

A small command-line tool for asking [TypeSafe's](https://typesafe.ai/) **Jev** model quick, structured questions. It can also run as an MCP server over stdio.

`gutcheck` is an independent project and isn't affiliated with TypeSafe.

Most LLMs return free text that you then have to parse. Jev instead takes some input text and a typed question with every possible answer defined up front, and it returns a structured answer: a selected option, a score, or a probability. It is fast and cheap, and its output is predictable.

`gutcheck` puts a thin interface over Jev for two audiences:

- **Scripts and automation** that need a small judgment, such as "is this log line an error?" or "which team owns this ticket?"
- **Other agents** that want to hand off simple decisions to a cheaper, faster model.

> **Status:** early design. Specs and decisions live in [`docs/`](docs/).

## License

[Apache License 2.0](LICENSE).
