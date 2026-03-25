# API (Tool Surface)

Base URL: `http://localhost:8080`

Auth headers:

- `x-telegram-init-data: <initData>` (Telegram Web App)
- or `x-user-id: <int64>` (dev fallback)

## Endpoints (v1)

- `GET /v1/profile` -> user profile (timezone, onboarding state)
- `GET /v1/goals?status=active` -> list goals
- `POST /v1/goals` -> create goal
- `POST /v1/goals/{goal_id}` -> update goal (MVP patch)
- `POST /v1/progress` -> log progress toward a goal
- `POST /v1/mood` -> log mood (upsert by day)
- `POST /v1/reminders` -> create reminder schedule record
- `GET /v1/checkins/due?date_from=YYYY-MM-DD&date_to=YYYY-MM-DD` -> due checkins (MVP: per-user)
- `GET /v1/reviews/weekly` -> weekly stats (last 7 days in MVP)
- `GET /v1/ikigai` -> read the cached ikigai profile
- `POST /v1/ikigai` -> save the cached/manual ikigai profile
- `POST /v1/ikigai/force` -> regenerate a fresh ikigai profile + SVG from live user data, persist it, and return the snapshot

## Stopped mode

Set `HAPPI_STOPPED=1` to put the bot into stopped mode. The HTTP webhook still
returns `200 OK` so Telegram does not retry, but every incoming message receives
a static self-host notice instead of going through the coaching pipeline. Unset
the variable (or set it to `0`) and restart to resume normal operation.
