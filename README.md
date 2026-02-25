# Happi (Telegram Mini App MVP)

Mobile-first coaching + goals tracker for Telegram:

- Rust `axum` backend API (goals, progress logs, mood logs, reminders)
- Next.js Telegram Web App UI using Telegram UI kit

## Run locally (dev)

Backend:

```sh
cd backend
cargo run
```

Web App:

```sh
cd webapp
npm run dev
```

## Run backend with Docker

Using compose:

```sh
docker compose up --build backend
```

Or standalone:

```sh
docker build -t happi-backend ./backend
docker run --rm -p 8080:8080 -e TELEGRAM_BOT_TOKEN=... happi-backend
```

### Local auth

The backend accepts either:

- `x-telegram-init-data` (production path for Telegram Web Apps)
- `x-user-id` (dev fallback)

For local development without Telegram, set:

```sh
export NEXT_PUBLIC_DEV_USER_ID=1
```

and the webapp will send `x-user-id: 1` on API calls.

## Environment

Backend (`backend/.env.example`):

- `BIND_ADDR` default `0.0.0.0:8080`
- `DATABASE_URL` default `sqlite://./happi.db?mode=rwc`
- `TELEGRAM_BOT_TOKEN` required to validate `x-telegram-init-data`
- `CORS_ALLOW_ORIGIN` optional; if unset CORS is permissive for dev

Webapp (`webapp/.env.example`):

- `NEXT_PUBLIC_API_BASE_URL` default `http://localhost:8080`
- `NEXT_PUBLIC_DEV_USER_ID` optional dev fallback

## Docs

- `docs/ui/telegram-miniapp-design.md`
- `docs/ui/thumb-reach.md`
- `deploy/nginx/happiiiiibot.ammarfaisal.me.conf`
