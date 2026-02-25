-- Users are identified by Telegram user_id (int64).
CREATE TABLE IF NOT EXISTS users (
  user_id INTEGER PRIMARY KEY,
  timezone TEXT NOT NULL DEFAULT 'UTC',
  reminder_window_start TEXT NULL, -- "HH:MM"
  reminder_window_end TEXT NULL,   -- "HH:MM"
  quiet_hours_start TEXT NULL,     -- "HH:MM"
  quiet_hours_end TEXT NULL,       -- "HH:MM"
  onboarding_state TEXT NOT NULL DEFAULT 'new',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS goals (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  title TEXT NOT NULL,
  why TEXT NULL,
  metric TEXT NULL,
  target_kind TEXT NOT NULL DEFAULT 'number', -- number|boolean|habit
  target_value REAL NULL,
  target_text TEXT NULL,
  deadline TEXT NULL, -- "YYYY-MM-DD"
  cadence TEXT NULL,  -- e.g. daily|weekly|custom
  tags_json TEXT NOT NULL DEFAULT '[]',
  ikigai_alignment_json TEXT NULL,
  status TEXT NOT NULL DEFAULT 'active', -- active|paused|completed|archived
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS goals_user_status_idx ON goals(user_id, status);

CREATE TABLE IF NOT EXISTS progress_logs (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  goal_id TEXT NOT NULL,
  date TEXT NOT NULL, -- "YYYY-MM-DD"
  value REAL NULL,
  value_text TEXT NULL,
  note TEXT NULL,
  confidence INTEGER NULL, -- 1..5
  idempotency_key TEXT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS progress_logs_goal_date_idx ON progress_logs(goal_id, date);
CREATE UNIQUE INDEX IF NOT EXISTS progress_logs_idem_idx ON progress_logs(user_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS mood_logs (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  date TEXT NOT NULL, -- "YYYY-MM-DD"
  happiness INTEGER NOT NULL,
  energy INTEGER NOT NULL,
  stress INTEGER NOT NULL,
  note TEXT NULL,
  idempotency_key TEXT NULL,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX IF NOT EXISTS mood_logs_user_date_idx ON mood_logs(user_id, date);
CREATE UNIQUE INDEX IF NOT EXISTS mood_logs_idem_idx ON mood_logs(user_id, idempotency_key) WHERE idempotency_key IS NOT NULL;

CREATE TABLE IF NOT EXISTS reminders (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  type TEXT NOT NULL, -- daily_checkin|weekly_review|custom
  schedule_kind TEXT NOT NULL DEFAULT 'rrule', -- rrule|cron
  schedule TEXT NOT NULL, -- rrule/cron string (interpretation is bot/backend-specific)
  payload_json TEXT NOT NULL DEFAULT '{}',
  quiet_hours_json TEXT NULL,
  start_date TEXT NULL, -- "YYYY-MM-DD"
  next_run_at TEXT NULL, -- RFC3339 UTC
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS reminders_user_enabled_idx ON reminders(user_id, enabled);

CREATE TABLE IF NOT EXISTS ikigai_profiles (
  user_id INTEGER PRIMARY KEY,
  mission TEXT NULL,
  themes_json TEXT NOT NULL DEFAULT '[]',
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS goal_alignment (
  goal_id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  alignment_score INTEGER NOT NULL,
  quadrants_json TEXT NOT NULL DEFAULT '[]',
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE
);

