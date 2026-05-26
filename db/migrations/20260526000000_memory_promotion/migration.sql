-- HLT-030-SQL-BAD-BEHAVIOR proof and rollback notes:
-- rollback: ALTER TABLE drops aren't reversible in SQLite without table
-- rewrites; if you must roll back, restore the schema captured in the
-- preceding migration (20260522130000_zyal_advanced_reasoning/migration.sql)
-- and drop the two indexes added below. The four new columns default-fill
-- existing rows so a forward-only deployment is safe.
-- backup/row-count evidence
SELECT (SELECT COUNT(*) FROM `daemon_memory_capsule`) AS `pre_rows_daemon_memory_capsule`;

-- Phase E1 (foamy-koala plan): add structured-memory + promotion-lifecycle
-- columns to daemon_memory_capsule. New columns:
--   memory_kind        — one of episodic|semantic|procedural|negative
--                        (mirror of zyal_core::MemoryKind)
--   promotion_status   — one of scratch|run_only|project_only|global
--                        (mirror of zyal_core::MemoryPromotionStatus)
--   claim_text         — human-readable claim summary the Memory Curator
--                        writes (replaces opaque payload-only retrieval)
--   approved_by_role   — NULL until Verifier or Reducer signs off; values
--                        are role labels like "verifier" or "reducer"

ALTER TABLE `daemon_memory_capsule` ADD COLUMN `memory_kind` text NOT NULL DEFAULT 'semantic';
ALTER TABLE `daemon_memory_capsule` ADD COLUMN `promotion_status` text NOT NULL DEFAULT 'scratch';
ALTER TABLE `daemon_memory_capsule` ADD COLUMN `claim_text` text NOT NULL DEFAULT '';
ALTER TABLE `daemon_memory_capsule` ADD COLUMN `approved_by_role` text;

-- Retrieval indexes for Phase E2's cross-run filter and the watcher's
-- per-kind queries.
CREATE INDEX IF NOT EXISTS `idx_dmc_promotion_scope` ON `daemon_memory_capsule`(`promotion_status`, `scope`);
CREATE INDEX IF NOT EXISTS `idx_dmc_kind_status` ON `daemon_memory_capsule`(`memory_kind`, `status`);

SELECT (SELECT COUNT(*) FROM `daemon_memory_capsule`) AS `post_rows_daemon_memory_capsule`;
