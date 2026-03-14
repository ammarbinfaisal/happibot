# Happi Agent Architecture

## Mission

Optimize the user's happiness through meaningful goals. Help them set, track, and
connect short-term and long-term goals to lasting wellbeing. Progress toward
meaningful goals IS happiness.

## Message Flow

```
User sends text/voice message in Telegram
│
├─ Voice? → Download from Telegram → Whisper transcription → text
│
▼
handle_user_message()
│
├─ 1. Ensure user exists (auto-create + default reminders on first message)
│
├─ 2. Assemble context:
│   ├─ Recent chat history (configurable window, default 10 messages)
│   ├─ Active goals (title, why, cadence, deadline)
│   ├─ Active observations (LLM's private notes, timestamped)
│   └─ Semantic search results (embed user message → cosine search → top-K past conversations)
│
├─ 3. Coaching decision (Responses API call with full context):
│   ├─ Prompt contract includes:
│   │   ├─ Mission: improve happiness via meaningful goal progress
│   │   ├─ Default follow-through: act when intent is clear and action is reversible
│   │   ├─ Grounding rules: use only supplied context, no invented memories/facts
│   │   ├─ Output contract: strict JSON schema, no free-form prose
│   │   └─ Reply quality bar: warm, concise, specific, action-oriented
│   ├─ mood     → log happiness/energy/stress
│   ├─ progress → log progress on matched goal
│   ├─ create_goal → create new goal
│   └─ chat     → conversational reply
│
├─ 4. Backend verifies + executes intent
│   ├─ Validate JSON shape and enum branch
│   ├─ Match referenced goal where needed
│   └─ Persist mood/progress/goal updates
│
├─ 5. Send coaching reply to user
│
└─ 6. Async post-message pipeline (does not block reply):
    ├─ Embed conversation turn (user + assistant) → store in embeddings table
    └─ Maybe generate observations (every N messages):
        ├─ Load recent chat + existing observations + active goals
        ├─ LLM generates 0-3 durable observations via strict JSON schema
        ├─ Store observations (with optional supersession of old ones)
        └─ Embed observations for future semantic retrieval
```

## Agentic Prompting Principles

The agent prompt is tuned around GPT-5.4 prompt-guidance patterns:

- **Structured output first**: both intent parsing and observation generation use
  strict JSON schemas, not best-effort free-form parsing.
- **Default follow-through**: if the user's intent is clear and the action is
  reversible, the agent proceeds without asking.
- **Emotional attunement before optimization**: when the user is discouraged,
  overwhelmed, or vulnerable, the agent prioritizes empathy and the smallest
  useful next step instead of forcing logging.
- **Grounded memory use**: the model may use goals, observations, and retrieved
  context only when they materially improve coaching relevance.
- **Durable memory only**: observation generation prefers stable patterns,
  motivational drivers, risks, coaching preferences, milestones, and
  mood-to-goal connections. It avoids obvious one-off facts.

## Coaching Policy

The primary decision rule is:

- If the user provides enough information to log mood/progress/create a goal,
  do it.
- If key information is missing and cannot be inferred reliably, ask one short
  clarifying question.
- If the user mainly needs support, reflection, or encouragement, reply as a
  coach instead of forcing data capture.

The coaching reply should usually:

- acknowledge the user's current state honestly,
- connect progress or setbacks to the user's broader goals and wellbeing,
- suggest the smallest meaningful next step when appropriate.

## Memory Architecture

### Three layers of memory:

1. **Chat History** (short-term)
   - All messages stored indefinitely (no cap)
   - Recent N messages loaded as direct context for the LLM
   - Each conversation turn embedded for semantic search

2. **Semantic Search** (retrieval)
   - OpenAI embeddings stored as BLOBs in SQLite
   - Cosine similarity computed in Rust (no external vector DB needed)
   - Searches across both chat history and observations
   - Brings relevant past context into the current conversation

3. **Observations** (long-term)
   - LLM-generated private notes about the user
   - Categories: pattern, insight, preference, risk, milestone, connection
   - Timestamped and supersession-linked (evolving understanding)
   - Can be global or goal-specific
   - Confidence-scored (0.0-1.0)
   - Always included in context (up to configurable limit)

### Observation Evolution

Observations form a chain. When the LLM's understanding changes:

```
[2026-03-14] [pattern] User exercises in the morning
    ↓ superseded by
[2026-03-21] [pattern] User exercises in the morning but skips on high-stress days
    ↓ superseded by
[2026-04-01] [insight] User uses exercise as stress relief — skipping on high-stress days is counterproductive. They've acknowledged this.
```

Only the latest (non-superseded) observations appear in context, but the full
chain is preserved in the database for future analysis.

## Conversation Behavior

The agent is designed for natural, asynchronous conversation:

- **Multi-turn awareness**: If the LLM asked a question, the user's next message
  is interpreted in that context
- **Graceful pivots**: If the user ignores the LLM's question and sends something
  unrelated, the agent pivots without insisting
- **Temporal gaps**: The user may go silent for hours/days and return with a new
  topic. The agent treats each message in context but doesn't force continuity.
- **Observation of communication style**: The observation system notes when users
  ignore questions, prefer brevity, etc., and adapts over time

## Reminder System

Default reminders auto-created for new users:
- Daily mood check-in (9am in user's timezone)
- Evening goal nudge (7pm)
- Weekly review (Sunday 6pm)

Quiet hours respected. Minimum 30min between any reminders (enforced by cron schedules).

## Configuration

All LLM parameters are configurable via environment variables:

| Variable | Default | Description |
|---|---|---|
| `HAPPI_CHAT_MODEL` | `gpt-5.4-pro` | Model for intent parsing + coaching replies |
| `HAPPI_OBSERVATION_MODEL` | `gpt-5.4-pro` | Model for observation generation |
| `HAPPI_EMBEDDING_MODEL` | `text-embedding-3-large` | Embedding model (3072 dims) |
| `HAPPI_CHAT_REASONING_EFFORT` | `medium` | Reasoning effort for coaching decision + reply |
| `HAPPI_OBSERVATION_REASONING_EFFORT` | `high` | Reasoning effort for durable observation generation |
| `HAPPI_CHAT_VERBOSITY` | `low` | Response verbosity for user-facing coaching output |
| `HAPPI_OBSERVATION_VERBOSITY` | `low` | Response verbosity for structured observation output |
| `HAPPI_CHAT_HISTORY_WINDOW` | `10` | Recent messages in direct context |
| `HAPPI_SEMANTIC_TOP_K` | `5` | Semantically retrieved past conversations |
| `HAPPI_OBSERVATION_INTERVAL` | `6` | Min messages between observation rounds |
| `HAPPI_MAX_OBSERVATIONS` | `15` | Max active observations in context |

Notes:
- GPT-5.4-pro is used through the **Responses API**, not legacy chat
  completions.
- Temperature is not the primary control surface here; reasoning effort,
  structured schemas, and verbosity are the main behavior levers.

### Cost Considerations

With `text-embedding-3-large` and `gpt-5.4-pro`:
- Cost is configurable via model selection env vars
- Defaults favor coaching quality and stronger follow-through over minimum cost
- If latency/cost becomes an issue, the safest first knobs are
  `HAPPI_CHAT_REASONING_EFFORT`, `HAPPI_OBSERVATION_REASONING_EFFORT`, or
  moving observations to a cheaper model before downgrading the main coach

## Database Schema (memory-related)

### embeddings
| Column | Type | Description |
|---|---|---|
| id | INTEGER PK | Auto-increment |
| user_id | INTEGER FK | User reference |
| source_type | TEXT | 'chat' or 'observation' |
| source_id | TEXT | References chat_history.id or observations.id |
| embedding | BLOB | f32 vector (model-dependent dimensions) |
| content_hash | TEXT | SHA256 for dedup |

### observations
| Column | Type | Description |
|---|---|---|
| id | TEXT PK | UUID |
| user_id | INTEGER FK | User reference |
| goal_id | TEXT FK NULL | NULL=global, non-NULL=goal-specific |
| category | TEXT | pattern/insight/preference/risk/milestone/connection |
| content | TEXT | The observation |
| confidence | REAL | 0.0-1.0 |
| superseded_by | TEXT NULL | Points to newer observation |

## Bot Commands

| Command | Description |
|---|---|
| `/start` | Welcome message with mini app button |
| `/app` | Open the Happi mini app |
| `/goals` | List active goals |
| `/checkin` | Start a mood check-in |
