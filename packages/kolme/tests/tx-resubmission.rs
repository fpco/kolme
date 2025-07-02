use std::time::Duration;
use tokio::process::Command;

#[tokio::test]
async fn tx_resubmission_test() {
    let mut validator = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg("../examples/kademlia-discovery/Cargo.toml")
        .arg("--")
        .arg("validators")
        .arg("2001")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start validator");

    let mut client = Command::new("cargo")
        .arg("run")
        .arg("--manifest-path")
        .arg("../examples/kademlia-discovery/Cargo.toml")
        .arg("--")
        .arg("client")
        .arg("--validator")
        .arg("/ip4/127.0.0.1/tcp/2001")
        .arg("--continous")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to start client");

    // Enough time to receive several broadcasts.
    tokio::time::sleep(Duration::from_secs(20)).await;

    validator.kill().await.expect("Failed to kill validator");
    client.kill().await.expect("Failed to kill client");

    let validator_output = validator
        .wait_with_output()
        .await
        .expect("Failed to get validator output");

    let validator_stdout = String::from_utf8_lossy(&validator_output.stdout);
    let validator_stderr = String::from_utf8_lossy(&validator_output.stderr);

    let republish_message = "Not publishing a message that has already been published";
    let resubmission_count = validator_stdout.matches(republish_message).count()
        + validator_stderr.matches(republish_message).count();
    // We are allowing 1 since Notification: NewBlock message seems to
    // be resubmitted once. This should be fixed subsequently.
    if resubmission_count > 1
    {
        panic!("Resubmission of tx messages in validator");
    }
}
