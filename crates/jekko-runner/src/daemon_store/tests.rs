use jekko_store::daemon;
use jekko_store::db::Db;
use tempfile::tempdir;

use crate::model_client::ModelCallReceipt;
use crate::model_policy::ModelTaskKind;
use crate::port::PortTargetRequest;

use super::*;

#[test]
fn persists_model_receipt_with_seeded_run() {
    let dir = tempdir().unwrap();
    let db = Db::open_in_memory().unwrap();
    let target = PortTargetRequest {
        target: "Reference".into(),
        replacement: "Candidate".into(),
        target_repo: None,
        replacement_repo: None,
        request: "port it".into(),
        worker_cap: 2,
    };
    ensure_daemon_run(&db, dir.path(), "run-1", port_spec(&target)).unwrap();
    let receipt = ModelCallReceipt::fake_success(ModelTaskKind::PhaseFinalize, "ok");
    persist_model_receipt(&db, "run-1", &receipt).unwrap();
    assert_eq!(
        daemon::list_model_outcomes_for_run(db.connection(), "run-1")
            .unwrap()
            .len(),
        1
    );
}
