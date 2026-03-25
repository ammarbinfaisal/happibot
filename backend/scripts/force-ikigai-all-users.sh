#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

read_env_value() {
  local key="$1"
  local env_file="$2"

  grep -E "^[[:space:]]*${key}=" "${env_file}" \
    | tail -n 1 \
    | sed -E "s/^[[:space:]]*${key}=//" \
    | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//' \
    | sed -E 's/^"//; s/"$//' \
    | sed -E "s/^'//; s/'$//"
}

if [[ -f "${BACKEND_DIR}/.env" && -z "${DATABASE_URL:-}" ]]; then
  env_database_url="$(read_env_value "DATABASE_URL" "${BACKEND_DIR}/.env")"
  if [[ -n "${env_database_url}" ]]; then
    DATABASE_URL="${env_database_url}"
  fi
fi

API_BASE_URL="${API_BASE_URL:-http://localhost:8580}"
DATABASE_URL="${DATABASE_URL:-sqlite://./happi.db?mode=rwc}"
CURL_MAX_TIME="${CURL_MAX_TIME:-60}"

if ! command -v sqlite3 >/dev/null 2>&1; then
  echo "sqlite3 is required" >&2
  exit 1
fi

if ! command -v curl >/dev/null 2>&1; then
  echo "curl is required" >&2
  exit 1
fi

db_path="${DATABASE_URL#sqlite://}"
db_path="${db_path%%\?*}"

if [[ -z "${db_path}" ]]; then
  echo "Could not parse DATABASE_URL=${DATABASE_URL}" >&2
  exit 1
fi

if [[ "${db_path}" != /* ]]; then
  db_path="${BACKEND_DIR}/${db_path}"
fi

if [[ ! -f "${db_path}" ]]; then
  fallback_db_path=""

  if [[ -f "${BACKEND_DIR}/happi.db" ]]; then
    fallback_db_path="${BACKEND_DIR}/happi.db"
  elif [[ -f "${BACKEND_DIR}/happi_test.db" ]]; then
    fallback_db_path="${BACKEND_DIR}/happi_test.db"
  else
    mapfile -t db_candidates < <(
      find "${BACKEND_DIR}" -maxdepth 1 -type f \
        \( -name '*.db' -o -name '*.sqlite' -o -name '*.sqlite3' \) \
        | sort
    )
    if [[ "${#db_candidates[@]}" -eq 1 ]]; then
      fallback_db_path="${db_candidates[0]}"
    fi
  fi

  if [[ -n "${fallback_db_path}" ]]; then
    echo "Configured database not found at ${db_path}; using ${fallback_db_path}" >&2
    db_path="${fallback_db_path}"
  else
    echo "SQLite database not found at ${db_path}" >&2
    exit 1
  fi
fi

mapfile -t user_ids < <(sqlite3 "${db_path}" "SELECT user_id FROM users ORDER BY user_id;")

if [[ "${#user_ids[@]}" -eq 0 ]]; then
  echo "No users found in ${db_path}"
  exit 0
fi

success_count=0
failure_count=0

for user_id in "${user_ids[@]}"; do
  [[ -z "${user_id}" ]] && continue

  echo "Refreshing ikigai for user ${user_id}..."
  if response="$(
    curl -sS --fail-with-body --max-time "${CURL_MAX_TIME}" \
      -X POST \
      -H "x-user-id: ${user_id}" \
      "${API_BASE_URL}/v1/ikigai/force"
  )"; then
    success_count=$((success_count + 1))
    if command -v jq >/dev/null 2>&1; then
      mission="$(printf '%s' "${response}" | jq -r '.ikigai.mission // empty')"
      generated_at="$(printf '%s' "${response}" | jq -r '.generated_at // empty')"
      if [[ -n "${mission}" ]]; then
        echo "  ok  mission=${mission} generated_at=${generated_at}"
      else
        echo "  ok"
      fi
    else
      echo "  ok"
    fi
  else
    failure_count=$((failure_count + 1))
    echo "  failed for user ${user_id}" >&2
  fi
done

echo "Finished: success=${success_count} failure=${failure_count}"
[[ "${failure_count}" -eq 0 ]]
