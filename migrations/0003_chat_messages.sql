-- ── Chat History ─────────────────────────────────────────────────────────────
-- Apply manually: psql $DATABASE_URL -f migrations/0003_chat_messages.sql
-- ON DELETE CASCADE means deleting a case automatically wipes its chat history.

CREATE TABLE IF NOT EXISTS chat_messages (
    id          BIGSERIAL   PRIMARY KEY,
    case_id     TEXT        NOT NULL REFERENCES cases(case_id) ON DELETE CASCADE,
    role        TEXT        NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT        NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Composite index: fast lookup by case + chronological order
CREATE INDEX IF NOT EXISTS idx_chat_messages_case_id
    ON chat_messages(case_id, created_at ASC);
