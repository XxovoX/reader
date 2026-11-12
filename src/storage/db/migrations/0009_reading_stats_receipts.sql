-- Receipt and both aggregates are committed in the same transaction.
CREATE TABLE IF NOT EXISTS reading_stats_receipts (
    user_ns TEXT NOT NULL,
    event_id TEXT NOT NULL,
    PRIMARY KEY (user_ns, event_id)
);
