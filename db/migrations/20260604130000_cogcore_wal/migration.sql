-- cogcore_wal: durable mirror of the in-RAM cogcore WAL.
-- Lets `jekko-memory` persist the WAL tail (Core::wal_since) and replay it on
-- restart (Core::from_wal_ops) with a byte-identical export_state_hash.
--
-- Rollback:
--   DROP TABLE `cogcore_wal`;
--
-- Pre-flight: SELECT name FROM sqlite_master WHERE type='table' AND name='cogcore_wal';
--
-- This migration adds one new table only; it does not alter existing tables.

CREATE TABLE `cogcore_wal` (
    `scope` text NOT NULL,
    `seq` integer NOT NULL,
    `op_json` text NOT NULL,
    `prev_hash` text NOT NULL,
    `hash` text NOT NULL,
    PRIMARY KEY (`scope`, `seq`)
);
