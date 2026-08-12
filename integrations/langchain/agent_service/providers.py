"""Provider/model registry backing the Agent tab's model picker.

Two providers today, deliberately not a long list of placeholders nothing
behind them answers (the same "adopt/build only what's real" spirit
`00l-build-vs-adopt.md` applies to crates, applied here to config):

- **"opencode"**: the fixed `LLM_API_BASE_URL`/`LLM_MODEL`/
  `LLM_FALLBACK_MODEL`/`LLM_API_KEY` convention `gst_investigation_agent.py`
  already establishes — listed here as whatever is actually configured in
  the environment, never a hardcoded model name.
- **"ollama"**: a locally-running Ollama server. Its model list is read
  live from `GET /api/tags` — local models and Ollama's own cloud-proxied
  `*:cloud` models alike, since Ollama itself decides what it serves; this
  module only asks, never hardcodes a list. Reachable at an OpenAI-
  compatible endpoint under `/v1`, so the same `ChatOpenAI` client this
  service already uses for "opencode" works unchanged, api_key is a
  placeholder Ollama ignores.

A provider absent from `list_providers()` — unconfigured "opencode",
unreachable "ollama" — is a real, common state: the picker simply omits
it rather than offering something with nothing behind it.

`resolve_model` is the only path from a request body's `provider`/`model`
fields to an actual base_url/api_key: a client can only ever name a
provider id and a model id it was already told about via `list_providers`,
never a raw base_url — this is what stops a request from pointing this
service at an arbitrary endpoint.
"""

from __future__ import annotations

import os
from dataclasses import dataclass, field

import httpx

OLLAMA_BASE_URL = os.environ.get("OLLAMA_BASE_URL", "http://localhost:11434")

# Overridable by tests (`httpx.MockTransport`) so `list_providers()` never
# needs a real Ollama server running to be exercised. `None` means "use
# httpx's own default transport" — a real network call.
_ollama_transport: httpx.AsyncBaseTransport | None = None


@dataclass(frozen=True)
class ModelOption:
    id: str
    label: str


@dataclass(frozen=True)
class ProviderOption:
    id: str
    label: str
    models: list[ModelOption] = field(default_factory=list)


@dataclass(frozen=True)
class ResolvedModel:
    base_url: str
    model: str
    api_key: str


def _opencode_provider() -> ProviderOption | None:
    models: list[ModelOption] = []
    seen: set[str] = set()
    for env_key in ("LLM_MODEL", "LLM_FALLBACK_MODEL"):
        name = os.environ.get(env_key)
        if name and name not in seen:
            seen.add(name)
            models.append(ModelOption(id=name, label=name))
    if not models or not os.environ.get("LLM_API_BASE_URL"):
        return None
    return ProviderOption(id="opencode", label="opencode.ai/zen", models=models)


async def _ollama_provider() -> ProviderOption | None:
    try:
        async with httpx.AsyncClient(timeout=2.0, transport=_ollama_transport) as client:
            response = await client.get(f"{OLLAMA_BASE_URL}/api/tags")
            response.raise_for_status()
            data = response.json()
    except (httpx.HTTPError, ValueError):
        return None
    models = [ModelOption(id=m["name"], label=m["name"]) for m in data.get("models", [])]
    if not models:
        return None
    return ProviderOption(id="ollama", label="Ollama (local)", models=models)


async def list_providers() -> list[ProviderOption]:
    """Every provider that is both configured and currently reachable —
    in that order, so a deployment with only Ollama running still gets a
    working picker with one entry, not an empty one."""
    providers: list[ProviderOption] = []
    opencode = _opencode_provider()
    if opencode is not None:
        providers.append(opencode)
    ollama = await _ollama_provider()
    if ollama is not None:
        providers.append(ollama)
    return providers


def resolve_model(provider: str, model: str) -> ResolvedModel:
    if provider == "opencode":
        base_url = os.environ.get("LLM_API_BASE_URL")
        if not base_url:
            raise ValueError("opencode is not configured (LLM_API_BASE_URL is unset)")
        return ResolvedModel(
            base_url=base_url, model=model, api_key=os.environ.get("LLM_API_KEY", "unused")
        )
    if provider == "ollama":
        return ResolvedModel(base_url=f"{OLLAMA_BASE_URL}/v1", model=model, api_key="ollama")
    raise ValueError(f"unknown provider: {provider}")


def build_chat_model_from(resolved: ResolvedModel):
    """Imports `langchain_openai` lazily, matching
    `gst_investigation_agent.build_chat_model`'s own convention — neither
    this module nor its tests need it installed, only a real run does."""
    from langchain_openai import ChatOpenAI
    from pydantic import SecretStr

    return ChatOpenAI(
        base_url=resolved.base_url, model=resolved.model, api_key=SecretStr(resolved.api_key)
    )
