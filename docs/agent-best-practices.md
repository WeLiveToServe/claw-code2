# Future Agent Best Practices

This document is a handoff reference for future coding agents and engineers working in `claw-code2`.

The fork goal is to make the `claw` harness cleaner and more provider-agnostic without losing source discipline, secret hygiene, or reviewability.

## Working assumptions

- This repo is a fork of `ultraworkers/claw-code`, not a greenfield project.
- Anthropic is part of the support surface, but should not be treated as the conceptual center of every design decision.
- OpenAI, Anthropic, xAI, Gemini, OpenAI-compatible MaaS, and local OpenAI-compatible servers are all valid target backends for this fork.

## Never commit these

Do not commit:

- `settings.local.json`
- `.claw` session history
- plugin install artifacts
- local plugin registries
- secret-bearing config
- local `.env` files
- machine-local absolute paths
- API keys, bearer tokens, or copied auth headers

If a file contains credentials or local runtime state, it should stay ignored or remain untracked.

## Git and worktree discipline

Use explicit staging. Do not use blanket `git add .` in this repo unless you have just audited every untracked file.

Preferred workflow:

1. inspect `git status --short`
2. inspect untracked `.claw` and local artifact paths
3. stage only intended files explicitly
4. review `git diff --cached`
5. run a quick secret/path sanity check before commit

When cleaning the worktree:

- preserve user-intended source changes
- remove or ignore runtime-generated local artifacts
- avoid mixing docs, local runtime state, and source changes in one commit unless that grouping is deliberate

## Provider-agnostic design rules

When changing code, keep these concerns separate:

- model identity
- provider routing
- auth resolution
- transport/base URL selection
- runtime prompt/tool policy
- model capability assumptions

Do not let one layer silently stand in for another.

Examples of bad coupling:

- assuming a model prefix implies one auth shape everywhere
- assuming a provider choice implies one prompt strategy
- assuming Qwen always means DashScope
- assuming local or MaaS backends should inherit Anthropic-oriented defaults

## OpenAI-compatible first for local and MaaS

Treat local model servers and MaaS backends as OpenAI-compatible unless a provider-specific path is truly required.

That means:

- local `llama.cpp`, `vLLM`, `Ollama`, and similar backends should usually be wired through the OpenAI-compatible path
- MaaS vendors that expose an OpenAI-like API should also use the OpenAI-compatible path first
- only add provider-special handling when there is a real protocol, auth, or capability difference that cannot be modeled cleanly otherwise

This keeps the harness simpler and reduces unnecessary provider branching.

## Prefix and env-var hygiene

Provider-specific prefixes and auth env vars must not leak across unrelated backends.

Be careful about:

- Anthropic env vars influencing OpenAI-compatible routing
- OpenAI-compatible assumptions being forced onto Anthropic-native paths
- provider inference falling back to the wrong backend just because another key exists in the environment

Model-prefix routing should be explicit and predictable. Ambient credentials should not quietly overpower the user’s stated provider intent.

## Known architectural smell

The repo still contains Anthropic-centric residue.

Current examples include:

- Anthropic-native default model names and alias language
- docs and examples that still lead with Anthropic setup
- naming in some runtime surfaces that makes Anthropic feel “native” and others feel “adapted”
- historical assumptions in auth and routing logic that needed later correction

Document this honestly. Do not paper over it in new changes.

The right response is not to bolt on more special cases. The right response is to keep moving the design toward:

- explicit provider resolution
- centralized model/provider metadata
- normalized auth contracts
- backend capability-driven behavior

## Before commit checklist

- `git status --short` is understandable
- only intended files are staged
- no local `.claw` artifacts are staged
- no secret-bearing files are staged
- no machine-local paths are being added
- docs and code tell the same story

## Before push checklist

- `git diff --cached` has been reviewed
- tracked docs do not contain secrets or local paths
- commit scope is coherent
- local runtime artifacts remain ignored or unstaged
- the branch is intended for `origin`, not accidentally pointed at `upstream`

## Before eval checklist

- provider and model choice are explicit
- auth source is known
- base URL behavior is known
- config-local secrets remain untracked
- benchmark output is interpreted as harness + model behavior, not model behavior alone

## Repo-specific reminder

This fork is trying to become a cleaner multi-provider agent harness. Future work should reduce special-case coupling, not add more of it.
