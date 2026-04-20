# GPU Skill Handoff — Gemma 2 2B on T4

This document is a session handoff. It gives the next session everything needed to
integrate the running HuggingFace inference endpoint into claw-code2 as a backend,
and to call the gpu-skill-builder from this harness.

---

## Live endpoint (as of 2026-04-14)

| Field        | Value |
|---|---|
| URL          | `https://m6o6b9g0ggy07qcn.us-east-1.aws.endpoints.huggingface.cloud` |
| Model        | `google/gemma-2-2b-it` |
| Hardware     | NVIDIA T4 (16 GB VRAM) |
| Auth         | Bearer — use `HF_TOKEN` from `C:/Users/keith/dev/.env` |
| API shape    | OpenAI-compatible (`/v1/chat/completions`) |
| TTL          | Auto-destroys ~2h after provision. If it's gone, re-provision (see below). |

The endpoint is provisioned by gpu-skill-builder. If it has expired when this session
starts, re-run it — it comes up in about 60 seconds.

---

## Re-provisioning (if endpoint is gone)

```bash
cd C:/Users/keith/dev/gpu-skill-builder
PYTHONIOENCODING=utf-8 python main.py
```

main.py runs in agent mode (no prompts) and polls until running. The new endpoint URL
will be printed as `Endpoint ready: https://...`. Update the backend config below with
the new URL.

The skill lives at `C:/Users/keith/dev/gpu-skill-builder/`. Key files:
- `skill.py` — `run_skill()` entry point, agent mode and interactive mode
- `scheduler.py` — TTL, uptime reporting, stuck-pending watchdog, startup reconciliation
- `config.py` — guardrail settings (max spend, concurrency cap, stuck timeout)

Calling the skill from another agent (agent mode, no prompts):

```python
import asyncio
from skill import run_skill

result = await run_skill(
    instance_name="gpu-skill-poc",
    region="us-east-1",
    max_deployment_hours=2,
    provider="huggingface",
    hardware_slug="nvidia-t4-x1",
    model_repo_id="google/gemma-2-2b-it",
)
# result.instance.endpoint_url has the live URL
```

---

## Wiring into claw-code2 as a custom backend

The HF endpoint is OpenAI-compatible. claw-code2 already has custom backend support
via repo-local config. Add this to your `.claw` config (repo-local, not tracked):

```json
{
  "backend": "gemma-t4",
  "backends": {
    "gemma-t4": {
      "base_url": "https://m6o6b9g0ggy07qcn.us-east-1.aws.endpoints.huggingface.cloud/v1",
      "auth": "bearer",
      "auth_env": "HF_TOKEN",
      "model": "google/gemma-2-2b-it"
    }
  }
}
```

Or override at runtime:

```bash
CLAW_BACKEND=gemma-t4 ./claw "your prompt here"
```

**Model identity note:** HF endpoints may ignore the `model` field in the request body
and serve whatever model was loaded at provision time. If the harness sends
`model: "google/gemma-2-2b-it"` it will work. If it errors on model name mismatch,
try `model: "tgi"` (the HF TGI default) or omit the field.

---

## Operational guardrails already in place

These are enforced by gpu-skill-builder — the next session does not need to add them:

| Guardrail | Setting | Behaviour |
|---|---|---|
| Cost cap | `max_spend_per_instance_usd = $5.00` | Rejects provision if `hours × rate > $5` |
| Idempotency | — | Returns existing instance if same name is already active |
| Concurrency cap | `max_concurrent_instances = 2` | Rejects if 2+ instances already live |
| Stuck-pending watchdog | `stuck_pending_minutes = 15` | Auto-destroys instances stuck in pending > 15 min |
| Startup reconciliation | — | Re-registers TTL for orphaned instances on process restart |

The biggest operational risk when using an LLM as the orchestrator is calling
`run_skill()` in a loop. The idempotency guard (same instance name → returns existing)
and the concurrency cap (max 2 active) are the primary defences. Do not pass
dynamically generated instance names — always use a stable name like `"gpu-skill-poc"`.

---

## Testing the endpoint directly

Quick curl to confirm it's responding:

```bash
curl -s -X POST \
  https://m6o6b9g0ggy07qcn.us-east-1.aws.endpoints.huggingface.cloud/v1/chat/completions \
  -H "Authorization: Bearer $HF_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "google/gemma-2-2b-it",
    "messages": [{"role": "user", "content": "Reply with one word: what is 2+2?"}],
    "max_tokens": 10
  }'
```

Or from Python:

```python
import httpx, os

resp = httpx.post(
    "https://m6o6b9g0ggy07qcn.us-east-1.aws.endpoints.huggingface.cloud/v1/chat/completions",
    headers={"Authorization": f"Bearer {os.environ['HF_TOKEN']}"},
    json={
        "model": "google/gemma-2-2b-it",
        "messages": [{"role": "user", "content": "Reply with one word: what is 2+2?"}],
        "max_tokens": 10,
    },
)
print(resp.json())
```

---

## What is not done yet (natural next steps after integration)

- Health probe in the poll loop (post-provision: confirm inference responds before returning)
- Cross-session spend tracking (current cost cap is per-instance, not cumulative)
- AMD provider (blocked on DigitalOcean support ticket — account has credits but no entitlement)
- Model deployment step (SSH + vLLM) for DO droplets
- Gist-based agent comms (discussed: use GitHub Gists as a message bus between agents)
