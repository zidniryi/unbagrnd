CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    last_used_at TEXT,
    revoked_at TEXT,
    request_count INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_api_keys_key_hash ON api_keys (key_hash);
