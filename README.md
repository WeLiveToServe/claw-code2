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

- Config and env loading improvements:
  - repo-local `.claw` config is loaded more consistently
  - config-defined `env` values can be applied into runtime credential lookup
  - prompt/status paths now resolve the effective model more consistently from config

- Auth-health and provider-awareness improvements:
  - doctor/auth checks now look at the resolved model/provider rather than assuming Anthropic first
  - OpenAI-compatible auth reporting is clearer when the selected model is not Anthropic

- Benchmark and eval work:
  - `claw-code2` was benchmarked against OpenAI models including `openai/gpt-5.4`
  - canonical Claude Code was benchmarked against `claude-sonnet-4-6`
  - the eval work showed that harness behavior, not just model quality, materially affects outcomes

## Current state

The direction is provider-agnostic. The current implementation is not fully there yet.

What is already true:

- the harness can route beyond Anthropic
- OpenAI-compatible usage is real, not hypothetical
- local and MaaS backends can reasonably be treated as OpenAI-compatible targets
- eval work is already being used to compare harness behavior across providers

What is not fully solved yet:

- docs still skew Anthropic-first in tone and examples
- some naming and defaults remain Anthropic-native
- provider, auth, model identity, and transport concerns are still too intertwined in places
- some provider-specific assumptions still leak into UX and code structure

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
- keep commits source-focused and reviewable
- never commit local secrets, sessions, plugin install artifacts, or machine-local `.claw` runtime state
- prefer repo-safe shared config in tracked files and local credentials in ignored local config

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

In practice, that means commercial APIs, MaaS endpoints, and local model servers should all fit into the harness cleanly without Anthropic-specific assumptions leaking into unrelated paths.

## Important limitation statement

If you are reading this repo as if it were already a perfectly neutral multi-provider harness, do not assume that yet.

It is better to think of `claw-code2` as:

- a strong Rust agent harness
- already improved for multi-provider use
- still carrying Anthropic-centric residue that should be removed carefully

That is the fork’s current engineering reality.
