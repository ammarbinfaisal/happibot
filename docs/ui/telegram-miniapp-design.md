# Telegram Mini App Design Notes

These are quick notes for implementation, not a full restatement of Telegram docs.

## Core expectations

- Respect Telegram theme parameters and colors; do not hardcode a separate theme system.
- Treat the Mini App as embedded: keep headers short, avoid heavy chrome, prefer native-like layouts.
- Handle safe-area insets and support full-screen behavior where available.
- Prefer simple, fast flows: one action per screen, short forms, clear feedback.

## Implementation mapping in this repo

- `webapp/src/components/TgInit.tsx` initializes the Telegram Mini Apps SDK and binds:
  - theme vars: `--tg-theme-*`
  - viewport vars (incl. safe-area): `--tg-viewport-*`
  - mini app vars: `--tg-mini-app-*`
- `webapp/src/app/globals.css` uses `--tg-viewport-safe-area-inset-bottom` to keep bottom UI reachable and unoccluded.

