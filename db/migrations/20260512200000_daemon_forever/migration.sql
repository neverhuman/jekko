-- PR4 of the enchanted-clock plan: tables that back the daemon-side bridge to
-- the jekko-runner Rust crate from PR3.
--
-- All changes are additive. The `daemon_worker` table gains three nullable
-- columns; existing rows continue to read as-is and writers don't break.
-- Six brand-new tables capture: classified findings, batched waves, edges
-- between findings, concept lineage, concept links, and regression cycles.
--
-- rollback: drop tables in reverse order; the three new daemon_worker
--          columns can stay (nullable, no constraints).

ALTER TABLE `daemon_worker` ADD COLUMN `pool_id` text;--> statement-breakpoint
ALTER TABLE `daemon_worker` ADD COLUMN `batch_id` text;--> statement-breakpoint
ALTER TABLE `daemon_worker` ADD COLUMN `last_commit_sha` text;--> statement-breakpoint

CREATE TABLE `daemon_finding` (
    `id` text PRIMARY KEY NOT NULL,
    `run_id` text NOT NULL,
    `iteration` integer NOT NULL DEFAULT 0,
    `rule_id` text NOT NULL,
    `fingerprint` text NOT NULL,
    `severity` text NOT NULL,
    `paths_json` text NOT NULL DEFAULT '[]',
    `cap` text,
    `status` text NOT NULL DEFAULT 'queued',
    `attempt_count` integer NOT NULL DEFAULT 0,
    `batch_id` text,
    `last_error` text,
    `time_created` integer NOT NULL,
    `time_updated` integer NOT NULL,
    CONSTRAINT `fk_daemon_finding_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE INDEX `daemon_finding_run_status_idx` ON `daemon_finding` (`run_id`, `status`);--> statement-breakpoint
CREATE INDEX `daemon_finding_run_severity_idx` ON `daemon_finding` (`run_id`, `severity`);--> statement-breakpoint
CREATE INDEX `daemon_finding_fp_idx` ON `daemon_finding` (`run_id`, `fingerprint`);--> statement-breakpoint

CREATE TABLE `daemon_finding_batch` (
    `id` text PRIMARY KEY NOT NULL,
    `run_id` text NOT NULL,
    `wave_index` integer NOT NULL,
    `lane` text NOT NULL DEFAULT 'parallel',
    `worker_id` text,
    `status` text NOT NULL DEFAULT 'queued',
    `started_at` integer,
    `ended_at` integer,
    `result_json` text,
    `time_created` integer NOT NULL,
    `time_updated` integer NOT NULL,
    CONSTRAINT `fk_daemon_finding_batch_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE INDEX `daemon_finding_batch_run_wave_idx` ON `daemon_finding_batch` (`run_id`, `wave_index`);--> statement-breakpoint
CREATE INDEX `daemon_finding_batch_status_idx` ON `daemon_finding_batch` (`run_id`, `status`);--> statement-breakpoint

CREATE TABLE `daemon_finding_edge` (
    `run_id` text NOT NULL,
    `parent_id` text NOT NULL,
    `child_id` text NOT NULL,
    `kind` text NOT NULL DEFAULT 'path_overlap',
    `time_created` integer NOT NULL,
    PRIMARY KEY (`run_id`, `parent_id`, `child_id`),
    CONSTRAINT `fk_daemon_finding_edge_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE INDEX `daemon_finding_edge_run_idx` ON `daemon_finding_edge` (`run_id`);--> statement-breakpoint
CREATE INDEX `daemon_finding_edge_child_idx` ON `daemon_finding_edge` (`run_id`, `child_id`);--> statement-breakpoint

CREATE TABLE `daemon_concept` (
    `id` text PRIMARY KEY NOT NULL,
    `run_id` text NOT NULL,
    `concept_id` text NOT NULL,
    `definition` text NOT NULL,
    `derived_from_json` text,
    `proof_refs_json` text,
    `confidence` real NOT NULL DEFAULT 0.5,
    `invalidated_at` integer,
    `invalidated_reason` text,
    `time_created` integer NOT NULL,
    `time_updated` integer NOT NULL,
    CONSTRAINT `fk_daemon_concept_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE UNIQUE INDEX `daemon_concept_run_concept_idx` ON `daemon_concept` (`run_id`, `concept_id`);--> statement-breakpoint
CREATE INDEX `daemon_concept_invalidated_idx` ON `daemon_concept` (`run_id`, `invalidated_at`);--> statement-breakpoint

CREATE TABLE `daemon_concept_link` (
    `run_id` text NOT NULL,
    `parent_concept` text NOT NULL,
    `child_concept` text NOT NULL,
    `relation` text NOT NULL DEFAULT 'derived_from',
    `time_created` integer NOT NULL,
    PRIMARY KEY (`run_id`, `parent_concept`, `child_concept`),
    CONSTRAINT `fk_daemon_concept_link_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE INDEX `daemon_concept_link_parent_idx` ON `daemon_concept_link` (`run_id`, `parent_concept`);--> statement-breakpoint
CREATE INDEX `daemon_concept_link_child_idx` ON `daemon_concept_link` (`run_id`, `child_concept`);--> statement-breakpoint

CREATE TABLE `daemon_regression_cycle` (
    `id` text PRIMARY KEY NOT NULL,
    `run_id` text NOT NULL,
    `iteration` integer NOT NULL,
    `baseline_score` real,
    `current_score` real,
    `hard_delta` integer NOT NULL DEFAULT 0,
    `soft_delta` integer NOT NULL DEFAULT 0,
    `caps_delta` integer NOT NULL DEFAULT 0,
    `status` text NOT NULL DEFAULT 'pass',
    `result_json` text,
    `time_created` integer NOT NULL,
    `time_updated` integer NOT NULL,
    CONSTRAINT `fk_daemon_regression_cycle_run` FOREIGN KEY (`run_id`) REFERENCES `daemon_run`(`id`) ON DELETE RESTRICT
);--> statement-breakpoint
CREATE INDEX `daemon_regression_cycle_run_iter_idx` ON `daemon_regression_cycle` (`run_id`, `iteration`);--> statement-breakpoint
CREATE INDEX `daemon_regression_cycle_status_idx` ON `daemon_regression_cycle` (`run_id`, `status`);
