use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use serde::Deserialize;

use super::DeveloperIdSettings;

const POLL_INTERVAL: Duration = Duration::from_secs(10);
const RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
const MAX_TRANSIENT_FAILURES: usize = 6;

#[derive(Debug, Deserialize)]
struct SubmitResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct SubmissionInfo {
    id: String,
    status: String,
}

pub(super) fn notarize_path(path: &Path, settings: &DeveloperIdSettings) -> anyhow::Result<()> {
    let submission = submit(path, settings)?;
    println!(
        "submitted {} for notarization ({})",
        path.display(),
        submission.id
    );
    wait_for_completion(path, &submission.id, settings)
}

fn submit(path: &Path, settings: &DeveloperIdSettings) -> anyhow::Result<SubmitResponse> {
    let stdout = run_json(
        base_command("submit", settings)?
            .arg("--output-format")
            .arg("json")
            .arg("--no-progress")
            .arg(path),
        "submitting artifact for notarization",
    )?;
    parse_json(&stdout, "decoding notarization submit response")
}

fn wait_for_completion(
    path: &Path,
    submission_id: &str,
    settings: &DeveloperIdSettings,
) -> anyhow::Result<()> {
    let started = Instant::now();
    let mut consecutive_failures = 0usize;
    let mut last_status = None::<String>;
    loop {
        if started.elapsed() > MAX_WAIT {
            bail!(
                "notarization submission {submission_id} did not complete within {} minutes; retry later with `xcrun notarytool info {submission_id} ...`",
                MAX_WAIT.as_secs() / 60
            );
        }
        match fetch_info(submission_id, settings) {
            Ok(info) => {
                consecutive_failures = 0;
                if last_status.as_deref() != Some(info.status.as_str()) {
                    println!("notarization {} status: {}", info.id, info.status);
                    last_status = Some(info.status.clone());
                }
                match info.status.as_str() {
                    "Accepted" => return Ok(()),
                    "In Progress" => sleep(POLL_INTERVAL),
                    "Invalid" => return Err(invalid_submission(path, submission_id, settings)),
                    other => bail!(
                        "notarization submission {submission_id} returned unexpected status `{other}`"
                    ),
                }
            }
            Err(error) => {
                consecutive_failures += 1;
                if consecutive_failures > MAX_TRANSIENT_FAILURES {
                    return Err(error).with_context(|| {
                        format!(
                            "polling notarization status for {submission_id} failed {} times in a row",
                            consecutive_failures
                        )
                    });
                }
                eprintln!(
                    "notarization poll {submission_id} failed (attempt {consecutive_failures}/{MAX_TRANSIENT_FAILURES}); retrying in {}s: {error}",
                    RETRY_DELAY.as_secs()
                );
                sleep(RETRY_DELAY);
            }
        }
    }
}

fn fetch_info(
    submission_id: &str,
    settings: &DeveloperIdSettings,
) -> anyhow::Result<SubmissionInfo> {
    let stdout = run_json(
        base_command("info", settings)?
            .arg("--output-format")
            .arg("json")
            .arg("--no-progress")
            .arg(submission_id),
        "fetching notarization status",
    )?;
    parse_json(&stdout, "decoding notarization info response")
}

fn invalid_submission(
    path: &Path,
    submission_id: &str,
    settings: &DeveloperIdSettings,
) -> anyhow::Error {
    match fetch_log(submission_id, settings) {
        Ok(log) => {
            let log_path = log_path_for(path);
            match fs::write(&log_path, log) {
                Ok(()) => anyhow!(
                    "Apple notarization rejected submission {submission_id}; log written to {}",
                    log_path.display()
                ),
                Err(error) => anyhow!(
                    "Apple notarization rejected submission {submission_id}; failed to write log to {}: {error}",
                    log_path.display()
                ),
            }
        }
        Err(error) => anyhow!(
            "Apple notarization rejected submission {submission_id}; failed to fetch notarization log: {error}"
        ),
    }
}

fn fetch_log(submission_id: &str, settings: &DeveloperIdSettings) -> anyhow::Result<String> {
    run_json(
        base_command("log", settings)?
            .arg("--no-progress")
            .arg(submission_id),
        "fetching notarization log",
    )
}

fn base_command(subcommand: &str, settings: &DeveloperIdSettings) -> anyhow::Result<Command> {
    let apple_id = settings.required("APPLE_ID", settings.apple_id.as_deref())?;
    let app_password = settings.required(
        "APPLE_APP_SPECIFIC_PASSWORD",
        settings.apple_app_specific_password.as_deref(),
    )?;
    let team_id = settings.required("TEAM_ID", settings.team_id.as_deref())?;
    let mut command = crate::cmd::tool_command("xcrun");
    command
        .arg("notarytool")
        .arg(subcommand)
        .arg("--apple-id")
        .arg(apple_id)
        .arg("--password")
        .arg(app_password)
        .arg("--team-id")
        .arg(team_id);
    Ok(command)
}

fn run_json(command: &mut Command, description: &str) -> anyhow::Result<String> {
    let output = run_output(command, description)?;
    String::from_utf8(output.stdout)
        .with_context(|| format!("{description}: decoding utf-8 json response"))
}

fn run_output(command: &mut Command, description: &str) -> anyhow::Result<Output> {
    command.stdin(Stdio::null());
    let output = command
        .output()
        .with_context(|| format!("{description}: launching notarytool"))?;
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let details = [stderr, stdout]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if details.is_empty() {
        bail!("{description} failed with status {}", output.status);
    }
    bail!(
        "{description} failed with status {}: {details}",
        output.status
    );
}

fn parse_json<T>(json: &str, description: &str) -> anyhow::Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_str(json.trim()).with_context(|| format!("{description}: {json}"))
}

fn log_path_for(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("{name}.notary-log.json"))
        .unwrap_or_else(|| String::from("notary-log.json"));
    path.with_file_name(file_name)
}

#[cfg(test)]
mod tests {
    use super::{SubmissionInfo, SubmitResponse, log_path_for, parse_json};
    use std::path::Path;

    #[test]
    fn parses_submit_response() {
        let submit: SubmitResponse = parse_json(
            r#"{"id":"1c3c063e-54c1-4623-b97e-65c8b8647792","message":"ok"}"#,
            "submit",
        )
        .expect("parse submit");
        assert_eq!(submit.id, "1c3c063e-54c1-4623-b97e-65c8b8647792");
    }

    #[test]
    fn parses_submission_info() {
        let info: SubmissionInfo = parse_json(
            r#"{"id":"1c3c063e-54c1-4623-b97e-65c8b8647792","status":"In Progress"}"#,
            "info",
        )
        .expect("parse info");
        assert_eq!(info.status, "In Progress");
    }

    #[test]
    fn derives_notary_log_path() {
        let path = Path::new("/tmp/sagens.zip");
        assert_eq!(
            log_path_for(path),
            Path::new("/tmp/sagens.zip.notary-log.json")
        );
    }
}
