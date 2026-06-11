use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod baseline_diff;
mod beta;
mod close_issues;
mod close_stale_prs;
mod commands;
mod compliance_close;
mod contributor_label;
mod duplicate_issues;
mod github_run;
mod live_prod;
mod migrations;
mod notify_discord;
mod pr_compliance;
mod pr_info;
mod pr_management;
mod pr_standards;
mod pr_workflow_contract;
mod publish_docker_image;
mod publish_release;
mod publish_release_artifacts;
mod publish_version;
mod runtime_checks;
mod shared;
mod triage;

pub(crate) use baseline_diff::{baseline_diff, BaselineDiffFormat};
pub(crate) use live_prod::{live_prod, live_prod_init};
pub(crate) use runtime_checks::{guard_forbidden_runtime, run_preflight, GuardMode};
pub(crate) use shared::{
    current_github_event_context, github_event, host_binary_path, json_field, json_lookup,
    migrations_json, package_manager_version, repo_root, schema_check,
};

#[derive(Parser, Debug)]
#[command(name = "xtask", about = "Jekko workspace automation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    GuardForbiddenRuntime {
        #[arg(long, value_enum, default_value_t = GuardMode::Advisory)]
        mode: GuardMode,
    },
    GithubEvent {
        #[arg(help = "Field path such as target.number or pull_request.title")]
        field: String,
    },
    CloseStalePrs {
        #[arg(long)]
        dry_run: Option<bool>,
    },
    CloseIssues,
    ComplianceClose,
    ContributorLabel,
    DuplicateIssues,
    PrManagement,
    PullRequestField {
        #[arg(long)]
        number: u64,
        field: String,
    },
    PrStandards,
    PrCompliance,
    PrWorkflowContract,
    NotifyDiscord,
    PublishVersion,
    PublishReleaseInit,
    PublishReleaseFinalize {
        #[arg(long, env = "JEKKO_VERSION")]
        version: String,
        #[arg(long, env = "GH_REPO")]
        repo: Option<String>,
    },
    PublishDockerImage {
        #[arg(long, env = "JEKKO_VERSION")]
        version: String,
        #[arg(long, env = "JEKKO_CHANNEL")]
        channel: String,
    },
    PublishReleaseArtifacts {
        #[arg(long, env = "JEKKO_VERSION")]
        version: String,
        #[arg(long, env = "JEKKO_CHANNEL")]
        channel: String,
    },
    MigrationsJson {
        #[arg(long, default_value = "db/migrations")]
        root: PathBuf,
    },
    GithubRun,
    Triage,
    PackageManagerVersion,
    JankuraiGate {
        #[arg(long)]
        score: PathBuf,
    },
    ProofReceipt {
        #[arg(long)]
        lane: String,
        #[arg(long)]
        status: String,
        #[arg(long)]
        out: PathBuf,
    },
    SecurityLane {
        #[arg(long, default_value = "target/security")]
        out: PathBuf,
        #[arg(long, value_enum, default_value_t = commands::security_lane::SecurityProfile::Local)]
        profile: commands::security_lane::SecurityProfile,
    },
    SplitFamilyCheck,
    GitHook {
        hook: String,
    },
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
    JsonField {
        path: PathBuf,
        field: String,
    },
    HostBinaryPath,
    LiveProdInit,
    LiveProd,
    Schema {
        #[arg(long)]
        emit: bool,
    },
    BaselineDiff {
        #[arg(long, default_value = "target/tuiwright-jekko/baseline")]
        baseline: PathBuf,
        #[arg(long, default_value = "target/tuiwright-jekko/rust")]
        rust: PathBuf,
        #[arg(long, value_enum, default_value_t = BaselineDiffFormat::Text)]
        format: BaselineDiffFormat,
        #[arg(long)]
        threshold: Option<f64>,
    },
    CiFast,
    DbMigrationSmoke,
    CliHelpParity {
        #[arg(long)]
        strict: bool,
    },
    ToolSchemaParity {
        #[arg(long)]
        strict: bool,
    },
    SessionFixtureParity {
        #[arg(long)]
        strict: bool,
    },
    BackendContract {
        #[arg(long)]
        strict: bool,
    },
    HttpapiParity {
        #[arg(long)]
        strict: bool,
    },
    OpenapiCheck {
        #[arg(long)]
        strict: bool,
    },
    Preflight,
    Package {
        #[arg(long)]
        skip_build: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        baseline: bool,
        #[arg(long, default_value = "dist")]
        dist_root: PathBuf,
    },
    PublishBuildCli {
        #[arg(long)]
        skip_build: bool,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        baseline: bool,
        #[arg(long, default_value = "packages/jekko/dist")]
        dist_root: PathBuf,
    },
    Beta,
}

#[derive(Subcommand, Debug)]
enum ReleaseCommand {
    Package {
        #[arg(long)]
        target: String,
        #[arg(long)]
        dry_run: bool,
    },
    Attach {
        #[arg(long)]
        version: String,
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GuardForbiddenRuntime { mode } => guard_forbidden_runtime(mode),
        Command::GithubEvent { field } => github_event(field),
        Command::CloseStalePrs { dry_run } => close_stale_prs::run(dry_run),
        Command::CloseIssues => close_issues::run(),
        Command::ComplianceClose => compliance_close::run(),
        Command::ContributorLabel => contributor_label::run(),
        Command::DuplicateIssues => duplicate_issues::run(),
        Command::PrManagement => pr_management::run(),
        Command::PullRequestField { number, field } => pr_info::run(number, field),
        Command::PrStandards => pr_standards::run(),
        Command::PrCompliance => pr_compliance::run(),
        Command::PrWorkflowContract => pr_workflow_contract::run(),
        Command::NotifyDiscord => notify_discord::run(),
        Command::PublishVersion => publish_version::run(),
        Command::PublishReleaseInit => publish_release::init(),
        Command::PublishReleaseFinalize { version, repo } => {
            publish_release::finalize(&repo_root()?, &version, repo.as_deref())
        }
        Command::PublishDockerImage { version, channel } => {
            publish_docker_image::run(&version, &channel)
        }
        Command::PublishReleaseArtifacts { version, channel } => {
            publish_release_artifacts::run(&repo_root()?, &version, &channel)
        }
        Command::MigrationsJson { root } => migrations_json(&root),
        Command::GithubRun => github_run::run(),
        Command::Triage => triage::run(),
        Command::PackageManagerVersion => package_manager_version(),
        Command::JankuraiGate { score } => commands::jankurai_gate::run(&score),
        Command::ProofReceipt { lane, status, out } => {
            commands::proof_receipt::run(&lane, &status, &out)
        }
        Command::SecurityLane { out, profile } => commands::security_lane::run(&out, profile),
        Command::SplitFamilyCheck => commands::split_family::run(&repo_root()?),
        Command::GitHook { hook } => commands::git_hook::run(&hook),
        Command::Release { command } => match command {
            ReleaseCommand::Package { target, dry_run } => {
                commands::release::package(&repo_root()?, &target, dry_run)
            }
            ReleaseCommand::Attach { version, dry_run } => {
                commands::release::attach(&version, dry_run)
            }
        },
        Command::JsonField { path, field } => json_field(path, field),
        Command::HostBinaryPath => {
            println!("{}", host_binary_path()?);
            Ok(())
        }
        Command::LiveProdInit => live_prod_init(),
        Command::LiveProd => live_prod(),
        Command::Schema { emit } => {
            schema_check()?;
            if emit {
                let root = repo_root()?;
                let n = commands::schema::emit(&root)?;
                println!("schema: emitted {n} JSON Schema document(s)");
            }
            Ok(())
        }
        Command::BaselineDiff {
            baseline,
            rust,
            format,
            threshold,
        } => baseline_diff(&baseline, &rust, format, threshold),
        Command::CiFast => commands::ci_fast::run(&repo_root()?),
        Command::DbMigrationSmoke => {
            let sample = std::env::var("JEKKO_DB_SAMPLE").ok();
            let tmp = std::env::temp_dir()
                .join(format!("xtask-db-migration-smoke-{}", std::process::id()));
            let (applied, idempotent) = commands::db_migration_smoke::run(sample.as_deref(), &tmp)?;
            let _ = std::fs::remove_dir_all(&tmp);
            println!(
                "db-migration-smoke: {applied} migrations applied, idempotent {}",
                if idempotent { "✓" } else { "✗" }
            );
            Ok(())
        }
        Command::CliHelpParity { strict } => commands::cli_help_parity::run(&repo_root()?, strict),
        Command::ToolSchemaParity { strict } => {
            commands::tool_schema_parity::run(&repo_root()?, strict)
        }
        Command::SessionFixtureParity { strict } => {
            commands::session_fixture_parity::run(&repo_root()?, strict)
        }
        Command::BackendContract { strict } => {
            commands::backend_contract::run(&repo_root()?, strict)
        }
        Command::HttpapiParity { strict } => commands::httpapi_parity::run(&repo_root()?, strict),
        Command::OpenapiCheck { strict } => commands::openapi_check::run(&repo_root()?, strict),
        Command::Preflight => run_preflight(),
        Command::Package {
            skip_build,
            target,
            baseline,
            dist_root,
        } => {
            let report = commands::package::run(
                &repo_root()?,
                skip_build,
                target.as_deref(),
                baseline,
                &dist_root,
            )?;
            println!("package: dist dir {}", report.dist_dir.display());
            Ok(())
        }
        Command::PublishBuildCli {
            skip_build,
            target,
            baseline,
            dist_root,
        } => {
            let report = commands::package::run(
                &repo_root()?,
                skip_build,
                target.as_deref(),
                baseline,
                &dist_root,
            )?;
            println!(
                "publish-build-cli: staged {} (dist {})",
                report.binary_path.display(),
                report.dist_dir.display()
            );
            Ok(())
        }
        Command::Beta => beta::run(),
    }
}
