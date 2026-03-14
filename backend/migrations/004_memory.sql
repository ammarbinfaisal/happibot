-- Embeddings for semantic search over chat history and observations
CREATE TABLE IF NOT EXISTS embeddings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL,
  source_type TEXT NOT NULL,       -- 'chat', 'observation'
  source_id TEXT NOT NULL,         -- chat_history.id or observations.id
  embedding BLOB NOT NULL,         -- f32 x 1536 = 6144 bytes per vector
  content_hash TEXT NOT NULL,      -- SHA256 of embedded text (dedup)
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS embeddings_user_source_idx ON embeddings(user_id, source_type);
CREATE UNIQUE INDEX IF NOT EXISTS embeddings_source_idx ON embeddings(source_type, source_id);

-- LLM's private observations about the user
CREATE TABLE IF NOT EXISTS observations (
  id TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL,
  goal_id TEXT NULL,               -- NULL = global, non-NULL = goal-specific
  category TEXT NOT NULL,          -- 'pattern', 'insight', 'preference', 'risk', 'milestone'
  content TEXT NOT NULL,
  confidence REAL NOT NULL DEFAULT 0.8,
  superseded_by TEXT NULL,         -- points to newer observation if updated
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  FOREIGN KEY (user_id) REFERENCES users(user_id) ON DELETE CASCADE,
  FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS observations_user_idx ON observations(user_id, goal_id);
CREATE INDEX IF NOT EXISTS observations_active_idx ON observations(user_id) WHERE superseded_by IS NULL;
