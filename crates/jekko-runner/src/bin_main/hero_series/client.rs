use jekko_runner::hero_judge::HeroJudgeRunbook;
use jekko_runner::model_client::{
    BudgetedModelClient, FakeModelClient, JekkoRuntimeModelClient, ModelClient,
};

use super::super::cli::HeroJudgeRunArgs;

pub(crate) fn hero_judge_client(
    args: &HeroJudgeRunArgs,
    runbook: &HeroJudgeRunbook,
) -> Box<dyn ModelClient> {
    let max_parallel = runbook
        .hero_judge
        .population
        .max_parallel
        .min(runbook.hero_judge.super_reasoning.effective_max_workers())
        .max(1);
    if args.live {
        let live = JekkoRuntimeModelClient::with_policy(
            args.provider.clone(),
            args.model.clone(),
            runbook.hero_judge.model_policy.clone(),
        )
        .with_credential_policy(runbook.hero_judge.super_reasoning.credential_policy);
        Box::new(BudgetedModelClient::new(
            live,
            runbook.hero_judge.budgets.model_calls,
            max_parallel,
            true,
        ))
    } else {
        Box::new(FakeModelClient::success("deterministic hero judge"))
    }
}
