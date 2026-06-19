use tempfile::tempdir;

use super::*;
use crate::model_policy::ModelTaskKind;

#[tokio::test]
async fn fake_model_success_receipt_is_deterministic() {
    let dir = tempdir().unwrap();
    let client = FakeModelClient::success("planned");
    let receipt = client
        .complete(ModelTaskKind::PhaseFinalize, "prompt", dir.path())
        .await
        .unwrap();
    assert!(receipt.success);
    assert_eq!(receipt.provider, "fake");
    assert_eq!(receipt.model, "fake-model");
    assert_eq!(receipt.response.as_deref(), Some("planned"));
}

#[tokio::test]
async fn fake_model_failure_receipt_records_error() {
    let dir = tempdir().unwrap();
    let client = FakeModelClient::failure("no provider configured");
    let receipt = client
        .complete(ModelTaskKind::PhaseFinalize, "prompt", dir.path())
        .await
        .unwrap();
    assert!(!receipt.success);
    assert!(receipt
        .error
        .as_deref()
        .unwrap()
        .contains("no provider configured"));
}

#[tokio::test]
async fn budgeted_client_blocks_after_limit() {
    let dir = tempdir().unwrap();
    let client = BudgetedModelClient::new(FakeModelClient::success("{}"), 1, 1, false);
    let first = client
        .complete(ModelTaskKind::Frame, "prompt", dir.path())
        .await
        .unwrap();
    let second = client
        .complete(ModelTaskKind::Frame, "prompt", dir.path())
        .await
        .unwrap();
    assert!(first.success);
    assert!(!second.success);
    assert_eq!(second.budget_remaining, Some(0));
    assert!(second.error.unwrap().contains("budget exhausted"));
}

#[tokio::test]
async fn budgeted_client_rejects_fake_when_live_required() {
    let dir = tempdir().unwrap();
    let client = BudgetedModelClient::new(FakeModelClient::success("{}"), 2, 1, true);
    let receipt = client
        .complete(ModelTaskKind::Frame, "prompt", dir.path())
        .await
        .unwrap();
    assert!(!receipt.success);
    assert!(receipt
        .error
        .unwrap()
        .contains("deterministic model receipt rejected"));
}

#[test]
fn runtime_client_routes_by_policy_without_override() {
    let client = JekkoRuntimeModelClient::with_policy(None, None, Default::default());
    assert_eq!(client.selected_model(ModelTaskKind::StageBrainstorm), None);
    assert_eq!(client.selected_model(ModelTaskKind::StageReduce), None);
    let override_client = JekkoRuntimeModelClient::with_policy(
        None,
        Some("explicit/model".into()),
        Default::default(),
    );
    assert_eq!(
        override_client.selected_model(ModelTaskKind::StageReduce),
        Some("explicit/model".to_string())
    );
}

#[test]
fn runtime_client_does_not_pass_model_when_route_model_is_unspecified() {
    let dir = tempdir().unwrap();
    let client = JekkoRuntimeModelClient::with_policy(None, None, Default::default());
    let args = client.argv_for_test(ModelTaskKind::StageBrainstorm, dir.path(), "prompt");
    assert!(!args.iter().any(|arg| arg == "--model"));
}
