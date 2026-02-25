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
