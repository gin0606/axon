mod common;

use common::{TestRepo, assert_failure, assert_success, stdout};

#[test]
fn basic_cli_workflow_and_failure_exit() {
    let repo = TestRepo::new();
    repo.init("smoke");

    let issue = repo.plan("ship the change");
    let show = repo.axon(&["show", &issue, "--skip-command-evaluation"]);
    assert_success(&show);
    assert!(stdout(&show).contains("Progress: NotStarted  Disposition: Accepted"));

    assert_success(&repo.axon(&["start", &issue]));
    assert_success(&repo.axon(&["done", &issue]));
    let ended = repo.axon(&["show", &issue, "--skip-command-evaluation"]);
    assert_success(&ended);
    assert!(stdout(&ended).contains("Progress: Ended  Disposition: Accepted"));

    assert_failure(&repo.axon(&["show", "smoke-missing"]));
}
