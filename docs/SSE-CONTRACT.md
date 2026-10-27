# Streaming contract

Scope: current `v2` implementation. HTTP routes and error status semantics are documented in [ARCHITECTURE.md](../ARCHITECTURE.md#http-api). This file is the authority for frame parsing; do not infer a new event shape from an old UI implementation.

Both `/api/chat/stream` and `/api/orchestrate/stream` use UTF-8 SSE frames (`\n\n` separates frames; JSON is carried on `data:`). A TCP/HTTP chunk is **not** an SSE frame or a UTF-8 character boundary.

- Chat: unnamed `data:` frames contain routing information, `{ "chunk": "..." }` token deltas, and a final `{ "done": true, "content": "..." }` response. The final `content` is the **full** answer, not another delta. Failures are `{ "error": true, "detail": "..." }`.
- Orchestration: named `event:` frames include `token` (`delta`) and `result` (`response`). Early security/validation failures may be unnamed `data:` frames with `{ "error": true, "detail": "..." }`.
- Clients must check HTTP status before parsing, preserve UTF-8 across chunks, handle unnamed frames and propagate `error` frames. An event without `event:` has the SSE default type `message`. Multiple `data:` lines in one frame are joined with `\n`; lines beginning with `:` are comments/heartbeats. A completed stream without its expected final event is incomplete, not a successful empty answer.

The browser and TUI parse these independently in TypeScript; the CLI parses the same frames in Rust. Keep the contract tests in `webui/tests/api.test.ts` and the CLI UTF-8 split test up to date if the server event shape changes. Manual acceptance steps are in [TEST-GUIDE.md](../TEST-GUIDE.md).
