-- HLT-030-SQL-BAD-BEHAVIOR proof and rollback notes:
-- rollback: SQLite cannot drop columns without table rewrite; the
-- embedding column defaults to NULL so a forward-only deploy is safe.
-- backup/row-count evidence
SELECT (SELECT COUNT(*) FROM `daemon_memory_capsule`) AS `pre_rows_daemon_memory_capsule`;

-- Phase E2 substrate (foamy-koala plan): adds the embedding column so
-- cross-run semantic retrieval can land without a follow-up migration.
-- Vectors are stored as little-endian f32 byte blobs (1536 dims for
-- text-embedding-3-small; 3072 for text-embedding-3-large) — see
-- `zyal-core` for the canonical encoding helpers when E2's
-- `OpenAICompatibleEmbedder` lands.

ALTER TABLE `daemon_memory_capsule` ADD COLUMN `embedding` blob;

SELECT (SELECT COUNT(*) FROM `daemon_memory_capsule`) AS `post_rows_daemon_memory_capsule`;
