# Thumb-Reach Rules (Mobile First)

This repo targets Telegram Mini Apps on mobile, so primary interactions stay in the **natural thumb zone** (bottom portion of the screen).

## Practical layout rules

- Put primary navigation at the bottom. Avoid hamburger menus in the top corners.
- Put the primary CTA in a fixed bottom area, above the bottom safe area inset.
- Keep touch targets large: aim for ~44pt (iOS) / 48dp (Android) minimum.
- Keep spacing generous to avoid mis-taps (the "fat finger" problem).
- Avoid destructive actions in the easiest-to-hit area; put them behind a confirm step.

## Safe area

Telegram clients expose safe-area insets. In the webapp we bind them to CSS vars:

- `--tg-viewport-safe-area-inset-bottom` etc.

Use these for bottom padding so the tabbar/CTAs never sit under system UI.

