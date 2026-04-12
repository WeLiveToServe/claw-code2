# claw-code2

`claw-code2` is a working fork of [`ultraworkers/claw-code`](https://github.com/ultraworkers/claw-code) focused on turning the Rust `claw` CLI into a cleaner, more model-agnostic agent harness.

The practical goal of this fork is not to replace upstream identity or erase the original Anthropic-oriented design history. It is to harden the harness so it can more cleanly host:

- OpenAI
- Anthropic
- xAI
- Gemini
- OpenAI-compatible MaaS providers
- local OpenAI-compatible model servers

This repo should be read as an active engineering fork: useful today, improving quickly, and still carrying some upstream assumptions that need cleanup.

Today, that includes first-class support for explicit backend selection instead of relying only on model-prefix inference.

## Starting point

This repository started from `ultraworkers/claw-code`, whose canonical implementation lives in the Rust workspace under [`rust/`](./rust).

Upstream already provided:

- a substantial Rust CLI/runtime/tooling workspace
- Anthropic-native defaults and aliases
- OpenAI-compatible and xAI support in parts of the provider stack
- a growing parity and roadmap process

The fork inherits that foundation, but it also inherits a bias: parts of the codebase, docs, defaults, naming, and examples still make Anthropic feel like the conceptual center of the harness even when multiple providers are supported.

## Work done so far

Recent work in this fork has been aimed at making provider behavior less surprising and more operationally honest.

- Provider-routing fixes:
  - explicit OpenAI-compatible model families are routed more reliably
  - Gemini-style model selection was tested through the OpenAI-compatible path
  - token-limit handling was improved for provider-prefixed OpenAI models

- Explicit backend support:
  - runtime config now supports `backend` and `backends`
  - the CLI now supports `--backend`
  - `CLAW_BACKEND` can override backend selection from the environment
  - built-in backend presets now include `anthropic`, `openai`, `xai`, `dashscope`, and `openrouter`
  - custom OpenAI-compatible backends can be defined in config for droplet/local/MaaS endpoints without code forks

- Config and env loading improvements:
  - repo-local `.claw` config is loaded more consistently
  - config-defined `env` values can be applied into runtime credential lookup
  - prompt/status paths now resolve the effective model more consistently from config
  - backend-aware default model resolution is now available through config

- Auth-health and provider-awareness improvements:
  - doctor/auth checks now look at the resolved backend/model rather than assuming Anthropic first
  - OpenAI-compatible auth reporting is clearer when the selected model is not Anthropic
  - diagnostics now surface backend id, transport, auth env vars, and base URL source more truthfully
  - OpenRouter now uses `OPENROUTER_API_KEY` as its own first-class auth path rather than piggybacking on `OPENAI_API_KEY`

- Benchmark and eval work:
  - `claw-code2` was benchmarked against OpenAI models including `openai/gpt-5.4`
  - canonical Claude Code was benchmarked against `claude-sonnet-4-6`
  - the eval work showed that harness behavior, not just model quality, materially affects outcomes

- Smoke-test and validation work:
  - API provider-resolution tests pass for legacy model-prefix routing and explicit backend selection
  - the Rust CLI test suite was updated to cover backend precedence and backend-default model behavior
  - the provider smoke suite now validates native Anthropic, native OpenAI, native xAI, and OpenRouter-backed Qwen
  - the Windows test fixtures used by this fork were hardened so backend work can be validated locally without Unix-only assumptions

## Current state

The direction is provider-agnostic, and the harness is materially closer to that shape than it was at the start of this fork.

What is already true:

- the harness can route beyond Anthropic
- OpenAI-compatible usage is real, not hypothetical
- explicit backend selection now exists alongside legacy prefix routing
- OpenRouter is a first-class backend with separate credential handling
- custom OpenAI-compatible backends can be represented directly in config
- local and MaaS backends can reasonably be treated as OpenAI-compatible targets
- eval work is already being used to compare harness behavior across providers
- live smoke coverage currently includes Anthropic, OpenAI, xAI, and OpenRouter lanes

What is not fully solved yet:

- docs still skew Anthropic-first in tone and examples
- some naming and defaults remain Anthropic-native
- provider, auth, model identity, and transport concerns are still too intertwined in places
- some provider-specific assumptions still leak into UX and code structure
- Gemini and native DashScope/Qwen still need live-provider coverage configured in the smoke suite on this machine
- custom backend support is intentionally focused on bearer-token or no-auth OpenAI-compatible APIs for now

That residue is a cleanup target, not something to hide.

## Documentation map

- [`USAGE.md`](./USAGE.md) — current CLI usage and auth/setup guidance
- [`rust/README.md`](./rust/README.md) — Rust workspace overview
- [`PARITY.md`](./PARITY.md) — parity tracking and migration notes
- [`ROADMAP.md`](./ROADMAP.md) — backlog and cleanup roadmap
- [`docs/agent-best-practices.md`](./docs/agent-best-practices.md) — future-agent handoff and repo hygiene guide

## GitHub management guidelines

This fork has two remotes:

- `origin` — your fork, `WeLiveToServe/claw-code2`
- `upstream` — original source project, `ultraworkers/claw-code`

Use that split deliberately:

- treat `upstream` as the baseline for comparison and future sync work
- treat `origin` as the place for fork-specific hardening, eval infrastructure, and provider-agnostic cleanup

Commit hygiene for this repo:

- stage files explicitly; do not rely on blanket `git add .`
- review `git status --short` before staging
- review `git diff --cached` before committing
- keep commits source-focused and reviewable
- never commit local secrets, sessions, plugin install artifacts, or machine-local `.claw` runtime state
- prefer repo-safe shared config in tracked files and local credentials in ignored local config
- split unrelated source, docs, and local-runtime cleanup into separate commits when possible

What should generally be tracked:

- source changes
- tests
- docs
- non-secret shared settings such as tracked examples or shared aliases

What should not be tracked:

- `settings.local.json`
- session transcripts
- plugin install registries or installed plugin copies
- machine-local paths
- API keys or bearer tokens

## Working principle for provider support

The preferred long-term shape of this repo is:

- model identity is separate from provider transport
- provider routing is separate from auth resolution
- auth resolution is separate from runtime prompt/tool policy
- OpenAI-compatible backends are treated as a broad class, not as one-off hacks
- backend selection is explicit whenever possible, with legacy model-prefix inference kept for compatibility rather than treated as the ideal control path

In practice, that means commercial APIs, MaaS endpoints, and local model servers should all fit into the harness cleanly without Anthropic-specific assumptions leaking into unrelated paths.

## Backend model

The preferred way to think about this fork now is:

- pick a backend
- pick a model
- let auth and base URL resolution follow from the chosen backend

Current built-in backend presets:

- `anthropic`
- `openai`
- `xai`
- `dashscope`
- `openrouter`

Custom backends can also be defined in repo-local config when they expose an OpenAI-compatible API. That is the intended path for:

- self-hosted droplet inference
- local `vLLM` servers
- local `llama.cpp` OpenAI-compatible servers
- MaaS providers that expose OpenAI-compatible chat endpoints

Legacy model-prefix routing still works, but it is no longer the only mental model for how the harness is expected to operate.

## Important limitation statement

If you are reading this repo as if it were already a perfectly neutral multi-provider harness, do not assume that yet.

It is better to think of `claw-code2` as:

- a strong Rust agent harness
- already improved for multi-provider use
- still carrying Anthropic-centric residue that should be removed carefully

That is the fork’s current engineering reality.
