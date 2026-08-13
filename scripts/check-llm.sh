#!/usr/bin/env bash
set -euo pipefail

# Provider selection: set LLM_PROVIDER=ollama or LLM_PROVIDER=openai (default: auto).
# Auto-detection: if LLM_API_BASE_URL is unset and OLLAMA_BASE_URL is set (or the
# default Ollama host answers), it treats the target as Ollama.
LLM_PROVIDER="${LLM_PROVIDER:-auto}"
OLLAMA_BASE_URL="${OLLAMA_BASE_URL:-http://127.0.0.1:11434}"
LLM_API_BASE_URL="${LLM_API_BASE_URL:-}"
LLM_MODEL="${LLM_MODEL:-}"
LLM_API_KEY="${LLM_API_KEY:-}"

pick_model() {
  if [[ -n "${LLM_MODEL}" ]]; then
    printf '%s' "${LLM_MODEL}"
    return
  fi
  case "${LLM_PROVIDER}" in
    ollama) printf '%s' "deepseek-v4-flash" ;;
    *)      printf '%s' "deepseek-v4-flash-free" ;;
  esac
}

default_base_url() {
  case "${LLM_PROVIDER}" in
    ollama) printf '%s' "${OLLAMA_BASE_URL}/v1" ;;
    *)      printf '%s' "https://opencode.ai/zen/v1" ;;
  esac
}

LLM_MODEL="$(pick_model)"

resolve_provider() {
  if [[ "${LLM_PROVIDER}" == "auto" ]]; then
    if [[ -n "${LLM_API_BASE_URL}" ]] && [[ ! "${LLM_API_BASE_URL}" == *11434* ]]; then
      LLM_PROVIDER="openai"
    elif [[ -n "${LLM_API_BASE_URL}" ]] && [[ "${LLM_API_BASE_URL}" == *11434* ]]; then
      LLM_PROVIDER="ollama"
    elif curl -sS -m 3 "${OLLAMA_BASE_URL}/api/tags" >/dev/null 2>&1; then
      LLM_PROVIDER="ollama"
    else
      LLM_PROVIDER="openai"
    fi
  fi
}

resolve_provider

if [[ -z "${LLM_API_BASE_URL}" ]]; then
  LLM_API_BASE_URL="$(default_base_url)"
fi

if [[ "${LLM_PROVIDER}" == "openai" ]] && [[ -z "${LLM_API_KEY}" ]]; then
  echo "error: LLM_API_KEY is not set (required for provider openai)" >&2
  exit 1
fi

auth_header=()
if [[ "${LLM_PROVIDER}" == "openai" ]]; then
  auth_header=(-H "Authorization: Bearer ${LLM_API_KEY}")
fi

probe() {
  local payload
  payload=$(cat <<EOF
{"model":"${LLM_MODEL}","messages":[{"role":"user","content":"Reply with exactly: llm-ok"}],"max_tokens":16,"stream":false}
EOF
)
  local curl_args=(-sS -m 120 -w '\n__HTTP_%{http_code}__')
  if [[ "${LLM_PROVIDER}" == "openai" ]]; then
    curl_args+=(-H "Authorization: Bearer ${LLM_API_KEY}")
  fi
  curl "${curl_args[@]}" \
    -X POST "${LLM_API_BASE_URL}/chat/completions" \
    -H "Content-Type: application/json" \
    -d "${payload}"
}

out=$(probe) || {
  echo "error: request failed (exit $?). Base URL: ${LLM_API_BASE_URL}, model: ${LLM_MODEL}, provider: ${LLM_PROVIDER}" >&2
  exit 1
}

code=$(printf '%s' "${out}" | sed -n 's/.*__HTTP_\([0-9]*\)__.*/\1/p')
body=$(printf '%s' "${out}" | sed 's/__HTTP_[0-9]*__//')

if [[ "${code}" != "200" ]]; then
  echo "error: HTTP ${code} from ${LLM_API_BASE_URL} using model ${LLM_MODEL} (provider ${LLM_PROVIDER})" >&2
  printf '%s\n' "${body}" | jq -r '.error.message // .error.type // .' >&2 2>/dev/null || printf '%s\n' "${body}" >&2
  exit 1
fi
out="${body}"

echo "${out}" | jq -e '.choices[0].message.content' >/dev/null 2>&1 || {
  echo "error: unexpected response:\n${out}" >&2
  exit 1
}

echo "ok: ${LLM_MODEL} via ${LLM_API_BASE_URL} (provider ${LLM_PROVIDER})"
echo "${out}" | jq -r '.choices[0].message.content'