use crate::{AppConfig, read_object};
use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Local, NaiveDate, Utc};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    process::Command,
};

#[derive(Default, Debug)]
pub struct Snapshot {
    pub observed_at: String,
    pub loop_state: Option<Value>,
    pub events: Vec<Value>,
    pub warnings: Vec<String>,
    pub summary: Summary,
}
#[derive(Default, Debug)]
pub struct Summary {
    pub passed: usize,
    pub failed: usize,
    pub dispatch_started: usize,
    pub verified: usize,
    pub verification_failed: usize,
    pub fallback: usize,
    pub uncertain: usize,
    pub tokens: Option<u64>,
}
fn timestamp(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z"))
        .ok()
        .map(|t| t.with_timezone(&Utc))
}
pub fn snapshot(config: &AppConfig, repo: &Path, since: &str) -> Result<Snapshot> {
    let since = if since.is_empty() {
        None
    } else {
        Some(
            NaiveDate::parse_from_str(since, "%Y-%m-%d")
                .context("시작일은 YYYY-MM-DD 형식이어야 합니다")?,
        )
    };
    let root = repo.canonicalize().context("레포 폴더를 찾지 못했습니다")?;
    let key = format!("{:x}", Sha1::digest(root.to_string_lossy().as_bytes()));
    let path = config.harness_home.join("harness-state").join(format!(
        "loop-{}-{}.json",
        root.file_name().unwrap_or_default().to_string_lossy(),
        &key[..12]
    ));
    let mut out = Snapshot {
        observed_at: Local::now().format("%Y-%m-%d %H:%M:%S %:z").to_string(),
        ..Default::default()
    };
    if path.exists() {
        match read_object(&path) {
            Ok(value) => out.loop_state = Some(value),
            Err(_) => out
                .warnings
                .push("루프 상태 파일을 해석하지 못했습니다".into()),
        }
    }
    let file = match File::open(&config.metrics) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            out.warnings.push("계측 파일이 아직 없습니다".into());
            return Ok(out);
        }
        Err(e) => return Err(e).context("계측 파일을 읽지 못했습니다"),
    };
    ensure!(
        file.metadata()?.len() <= 128 * 1024 * 1024,
        "계측 파일이 128MB를 넘었습니다. 별도 파일로 나눠 연결하세요"
    );
    let mut rows = Vec::new();
    let mut bad = 0;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(value) if value.is_object() => rows.push(value),
            _ => bad += 1,
        }
    }
    if bad > 0 {
        out.warnings
            .push(format!("손상된 계측 {bad}줄을 제외했습니다"));
    }
    let remote = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["remote", "get-url", "origin"])
        .output()?;
    let metric_name = if remote.status.success() {
        let url = String::from_utf8(remote.stdout)?;
        url.trim()
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
            .rsplit(':')
            .next()
            .unwrap_or("")
            .trim_end_matches(".git")
            .to_owned()
    } else {
        root.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    };
    let named = rows
        .iter()
        .filter(|r| r["repo"].as_str() == Some(&metric_name))
        .count();
    if named > 0 {
        out.warnings.push("레포명 계측은 같은 이름의 워크트리를 함께 집계합니다. 경로별 분리는 원본 기록에 없어 확인할 수 없습니다.".into());
    }
    let matches_repo = |v: &Value| {
        v["repo"].as_str().is_some_and(|r| {
            r == metric_name
                || Path::new(r) == root
                || (Path::new(r).is_absolute()
                    && Path::new(r).canonicalize().is_ok_and(|p| p == root))
        })
    };
    let ids: HashSet<String> = rows
        .iter()
        .filter(|r| matches_repo(r))
        .filter_map(|r| r["dispatch_id"].as_str().map(str::to_owned))
        .collect();
    for row in rows {
        let belongs = matches_repo(&row)
            || (row.get("repo").is_none()
                && row["hook"] == "dispatch"
                && row["dispatch_id"]
                    .as_str()
                    .is_some_and(|id| ids.contains(id)));
        if !belongs {
            continue;
        }
        let Some(ts) = row["ts"].as_str().and_then(timestamp) else {
            out.warnings.push("날짜 없는 계측을 제외했습니다".into());
            continue;
        };
        if since.is_some_and(|s| ts.with_timezone(&Local).date_naive() < s) {
            continue;
        }
        if row["hook"] == "stop_verify" {
            if row["decision"] == "pass" && !(row["steps_run"] == 0 && row.get("files").is_some()) {
                out.summary.passed += 1;
            }
            if row["decision"] == "block" {
                out.summary.failed += 1;
            }
        }
        if row["hook"] == "dispatch" {
            match row["decision"].as_str().unwrap_or("") {
                "started" => out.summary.dispatch_started += 1,
                "fallback" => out.summary.fallback += 1,
                "reconcile" => out.summary.uncertain += 1,
                "verified" => match row["outcome"].as_str().unwrap_or("") {
                    "passed" => out.summary.verified += 1,
                    "failed" => out.summary.verification_failed += 1,
                    _ => (),
                },
                _ => (),
            }
        }
        out.events.push(row);
    }
    out.events
        .sort_by_key(|r| r["ts"].as_str().and_then(timestamp));
    Ok(out)
}
