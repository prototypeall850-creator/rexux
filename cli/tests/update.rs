use anyhow::Result;
use predicates::str::contains;
use std::path::Path;
use tempfile::TempDir;

fn rexux_command(rexux_home: &Path) -> Result<assert_cmd::Command> {
    let mut cmd = assert_cmd::Command::new(rexux_utils_cargo_bin::cargo_bin("codex")?);
    cmd.env("REXUX_HOME", rexux_home);
    Ok(cmd)
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn update_does_not_start_interactive_prompt() -> Result<()> {
    let rexux_home = TempDir::new()?;

    rexux_command(rexux_home.path())?
        .arg("update")
        .assert()
        .failure()
        .stderr(contains("`codex update` is not available in debug builds"));

    Ok(())
}
