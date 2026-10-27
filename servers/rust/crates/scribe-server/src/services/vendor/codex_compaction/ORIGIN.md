# OpenAI Codex local compaction reuse

Pinned upstream: `https://github.com/openai/codex/tree/6fe77fc12346591b8037a8704640e40196b19044`.

Copyright OpenAI. Licensed under Apache-2.0; original LICENSE and NOTICE are included unchanged.

Vendored unchanged: `codex-rs/prompts/templates/compact/prompt.md`, `summary_prefix.md`, upstream full `codex-rs/core/src/compact.rs` as `upstream_compact.rs.txt`, and upstream `codex-rs/utils/string/src/truncate.rs` as `upstream_truncate.rs.txt`.

Compiled `truncate.rs` is the original std-only truncation implementation, with equivalent local UTF-8 boundary helpers replacing newer std floor/ceil_char_boundary for workspace compiler compatibility. Unbundled upstream test-module declaration is omitted.

Prims `context_compaction.rs::build_compacted_history_with_limit` ports the exact ordering/budget structure of upstream lines 710–815 to Prims ChatMessage: reverse-select user messages within the same 20000 approximate-token default, middle-truncate one boundary message using the vendored original helper, reverse back, append the original summary prefix and model summary as a user message. Codex-specific ResponseItemEnvelope/media annotations are absent because Prims history is plain text. We do not link the large Codex core crate.

Prims adapter: one same-connection summary stream using the unchanged upstream summary prompt; no tools, images or auto retries. Only after successful complete output does the caller receive a replacement history. Existing current user request, pending proposal IDs/states and live read proof remain harness-owned. No OpenAI Responses/compact or Anthropic context_management parameters are sent to third-party endpoints.

Prims agent_plan.rs follows the same pinned Codex `core/src/tools/handlers/plan_spec.rs` / `plan.rs` update_plan argument shape. The bounded public-text validator is Prims-specific. agent_loop_guard.rs is a separate Prims loop guard, not claimed to be copied from Codex.
