#!/usr/bin/env bash
set -euo pipefail

dotenv_path="${DOTENV_PATH:-.env}"
if [[ -f "${dotenv_path}" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "${dotenv_path}"
  set +a
fi

if [[ -z "${TELEGRAM_BOT_TOKEN:-}" ]]; then
  echo "Missing TELEGRAM_BOT_TOKEN." >&2
  echo "Example: TELEGRAM_BOT_TOKEN=123:abc HOOK_URL=https://... bash $0" >&2
  exit 1
fi

if [[ -z "${HOOK_URL:-}" ]]; then
  echo "Missing HOOK_URL." >&2
  echo "Example: TELEGRAM_BOT_TOKEN=123:abc HOOK_URL=https://... bash $0" >&2
  exit 1
fi

if [[ "${HOOK_URL}" != https://* ]]; then
  echo "HOOK_URL must start with https:// (Telegram requires HTTPS for webhooks)." >&2
  exit 1
fi

api_base="${TELEGRAM_API_BASE_URL:-https://api.telegram.org}"
endpoint="${api_base%/}/bot${TELEGRAM_BOT_TOKEN}/setWebhook"

curl_args=(
  --silent
  --show-error
  --fail
  --request POST
  --form "url=${HOOK_URL}"
)

# Optional: Telegram will send `X-Telegram-Bot-Api-Secret-Token` if you set this.
if [[ -n "${WEBHOOK_SECRET_TOKEN:-}" ]]; then
  curl_args+=( --form "secret_token=${WEBHOOK_SECRET_TOKEN}" )
fi

# Optional: drop pending updates on webhook reset.
if [[ "${DROP_PENDING_UPDATES:-}" == "1" || "${DROP_PENDING_UPDATES:-}" == "true" ]]; then
  curl_args+=( --form "drop_pending_updates=true" )
fi

resp="$(curl "${curl_args[@]}" "${endpoint}")"

if command -v jq >/dev/null 2>&1; then
  jq . <<<"${resp}"
else
  echo "${resp}"
fi
