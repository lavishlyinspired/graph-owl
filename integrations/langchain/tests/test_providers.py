"""`agent_service.providers` — the provider/model registry backing the
console's model picker. Two providers today: "opencode" (the existing
fixed `LLM_API_BASE_URL`/`LLM_MODEL`/`LLM_FALLBACK_MODEL` env convention
`gst_investigation_agent.py` already establishes) and "ollama" (a
locally-running Ollama server, its model list read live via `/api/tags`
— Ollama itself decides what it's serving, local model or one of its own
cloud-proxied `:cloud` models alike; this module only asks, never
hardcodes a list).

A provider absent from `list_providers()`'s result — unconfigured
"opencode", unreachable "ollama" — is a real, common state, not an error:
the picker should simply not offer it, rather than showing something with
nothing behind it.
"""

from __future__ import annotations

import asyncio
import sys
from pathlib import Path

import httpx
import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from agent_service.providers import list_providers, resolve_model  # noqa: E402


def test_opencode_provider_lists_its_configured_models(monkeypatch):
    monkeypatch.setenv("LLM_API_BASE_URL", "https://opencode.example/v1")
    monkeypatch.setenv("LLM_MODEL", "deepseek-v4-flash-free")
    monkeypatch.setenv("LLM_FALLBACK_MODEL", "laguna-s-2.1-free")
    monkeypatch.setattr("agent_service.providers._ollama_transport", _refusing_transport())

    providers = asyncio.run(list_providers())

    opencode = next(p for p in providers if p.id == "opencode")
    assert {m.id for m in opencode.models} == {"deepseek-v4-flash-free", "laguna-s-2.1-free"}


def test_opencode_provider_dedupes_when_model_and_fallback_are_identical(monkeypatch):
    monkeypatch.setenv("LLM_API_BASE_URL", "https://opencode.example/v1")
    monkeypatch.setenv("LLM_MODEL", "same-model")
    monkeypatch.setenv("LLM_FALLBACK_MODEL", "same-model")
    monkeypatch.setattr("agent_service.providers._ollama_transport", _refusing_transport())

    providers = asyncio.run(list_providers())

    opencode = next(p for p in providers if p.id == "opencode")
    assert [m.id for m in opencode.models] == ["same-model"]


def test_opencode_provider_absent_when_unconfigured(monkeypatch):
    monkeypatch.delenv("LLM_MODEL", raising=False)
    monkeypatch.delenv("LLM_FALLBACK_MODEL", raising=False)
    monkeypatch.setattr("agent_service.providers._ollama_transport", _refusing_transport())

    providers = asyncio.run(list_providers())

    assert all(p.id != "opencode" for p in providers)


def _refusing_transport() -> httpx.MockTransport:
    def handler(request: httpx.Request) -> httpx.Response:
        raise httpx.ConnectError("connection refused", request=request)

    return httpx.MockTransport(handler)


def test_ollama_provider_lists_models_from_its_own_api(monkeypatch):
    def handler(request: httpx.Request) -> httpx.Response:
        assert request.url.path == "/api/tags"
        return httpx.Response(
            200,
            json={"models": [{"name": "qwen3.6:latest"}, {"name": "gpt-oss:20b-cloud"}]},
        )

    monkeypatch.setattr("agent_service.providers._ollama_transport", httpx.MockTransport(handler))
    monkeypatch.delenv("LLM_MODEL", raising=False)
    monkeypatch.delenv("LLM_FALLBACK_MODEL", raising=False)

    providers = asyncio.run(list_providers())

    ollama = next(p for p in providers if p.id == "ollama")
    assert {m.id for m in ollama.models} == {"qwen3.6:latest", "gpt-oss:20b-cloud"}


def test_ollama_provider_absent_when_unreachable(monkeypatch):
    monkeypatch.setattr("agent_service.providers._ollama_transport", _refusing_transport())

    providers = asyncio.run(list_providers())

    assert all(p.id != "ollama" for p in providers)


def test_ollama_provider_absent_when_it_reports_no_models(monkeypatch):
    def handler(request: httpx.Request) -> httpx.Response:
        return httpx.Response(200, json={"models": []})

    monkeypatch.setattr("agent_service.providers._ollama_transport", httpx.MockTransport(handler))

    providers = asyncio.run(list_providers())

    assert all(p.id != "ollama" for p in providers)


def test_resolve_model_opencode_reads_the_configured_base_url_and_key(monkeypatch):
    monkeypatch.setenv("LLM_API_BASE_URL", "https://opencode.example/v1")
    monkeypatch.setenv("LLM_API_KEY", "sk-real-key")

    resolved = resolve_model("opencode", "deepseek-v4-flash-free")

    assert resolved.base_url == "https://opencode.example/v1"
    assert resolved.model == "deepseek-v4-flash-free"
    assert resolved.api_key == "sk-real-key"


def test_resolve_model_opencode_without_configured_base_url_raises(monkeypatch):
    monkeypatch.delenv("LLM_API_BASE_URL", raising=False)

    with pytest.raises(ValueError):
        resolve_model("opencode", "deepseek-v4-flash-free")


def test_resolve_model_ollama_points_at_the_local_server_not_opencode(monkeypatch):
    monkeypatch.setenv("LLM_API_BASE_URL", "https://opencode.example/v1")
    monkeypatch.setenv("LLM_API_KEY", "sk-real-key")

    resolved = resolve_model("ollama", "qwen3.6:latest")

    assert resolved.base_url == "http://localhost:11434/v1"
    assert resolved.model == "qwen3.6:latest"
    # Ollama takes no real credential — must not leak the opencode key across.
    assert resolved.api_key != "sk-real-key"


def test_resolve_model_unknown_provider_raises():
    with pytest.raises(ValueError):
        resolve_model("made-up-provider", "whatever")
