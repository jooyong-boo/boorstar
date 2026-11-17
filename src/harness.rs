use crate::AppConfig;
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn python(config: &AppConfig) -> Command {
    let mut command = Command::new("python3");
    command
        .args(["-I", "-B"])
        .env("HARNESS_HOME", &config.harness_home)
        .env("HARNESS_METRICS_PATH", &config.metrics);
    command
}
fn script(config: &AppConfig, name: &str) -> Command {
    let mut command = python(config);
    command.args(["-c","import sys,runpy; sys.path.insert(0,sys.argv[1]); sys.argv=sys.argv[2:]; runpy.run_path(sys.argv[0],run_name='__main__')"])
        .arg(&config.harness_bin).arg(config.harness_bin.join(name));
    command
}
pub fn progress(config: &AppConfig, repo: &Path) -> Result<Value> {
    ensure!(
        config.harness_bin.join("spec_trace.py").is_file(),
        "spec_trace.py를 찾지 못했습니다"
    );
    let mut command = script(config, "spec_trace.py");
    command
        .arg("--repo")
        .arg(repo)
        .arg("--summary-json")
        .current_dir(repo);
    let (code, text) = run(command, None, 30)?;
    ensure!(code == 0, "단계 조회 실패 (종료 코드 {code})");
    serde_json::from_str(&text).context("단계 응답을 해석하지 못했습니다")
}
/// 출력은 임시 파일로 받아 pipe 교착을 피한다. 호출은 UI 밖에서 실행한다.
pub fn run(mut command: Command, input: Option<&[u8]>, seconds: u64) -> Result<(i32, String)> {
    let out = tempfile::tempfile()?;
    let err = tempfile::tempfile()?;
    command
        .stdout(Stdio::from(out.try_clone()?))
        .stderr(Stdio::from(err.try_clone()?));
    let stdin = if let Some(input) = input {
        use std::io::{Seek, SeekFrom};
        let mut file = tempfile::tempfile()?;
        file.write_all(input)?;
        file.seek(SeekFrom::Start(0))?;
        Stdio::from(file)
    } else {
        Stdio::null()
    };
    command.stdin(stdin);
    let mut child = command
        .spawn()
        .context("명령을 실행하지 못했습니다. Python·하네스 경로를 확인하세요")?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "명령이 {seconds}초 안에 끝나지 않았습니다. 위임 실행 여부는 불명확하므로 자동 재시도하지 않습니다"
            );
        }
        thread::sleep(Duration::from_millis(40));
    };
    fn text(mut file: File) -> Result<String> {
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        ensure!(
            file.metadata()?.len() <= 8 * 1024 * 1024,
            "명령 출력이 너무 큽니다"
        );
        let mut value = String::new();
        file.read_to_string(&mut value)?;
        Ok(value)
    }
    // stderr는 인증 정보나 작업 원문을 포함할 수 있어 그대로 표시하지 않는다.
    Ok((status.code().unwrap_or(-1), text(out)?))
}
pub fn doctor(config: &AppConfig, repo: &Path) -> Result<String> {
    ensure!(
        config.harness_bin.join("doctor.py").is_file(),
        "하네스 bin 경로에서 doctor.py를 찾지 못했습니다"
    );
    let mut command = script(config, "doctor.py");
    command.arg("--repo").arg(repo).current_dir(repo);
    let (code, output) = run(command, None, 30)?;
    ensure!(
        [0, 1].contains(&code) && !output.trim().is_empty(),
        "도입 점검 실패 (종료 코드 {code})"
    );
    Ok(output)
}
pub fn dispatch(config: &AppConfig, repo: &Path, task: Value) -> Result<Value> {
    config.models.validate()?;
    ensure!(
        config.harness_bin.join("dispatch.py").is_file(),
        "하네스 bin 경로에서 dispatch.py를 찾지 못했습니다"
    );
    ensure!(
        ["claude", "codex"].contains(&task["original"].as_str().unwrap_or("")),
        "기존 담당을 선택하세요"
    );
    ensure!(
        ["analysis", "implement", "review"].contains(&task["kind"].as_str().unwrap_or("")),
        "작업 종류를 선택하세요"
    );
    for key in ["summary", "spec"] {
        ensure!(
            !task[key].as_str().unwrap_or("").trim().is_empty(),
            "작업 요약과 명세를 입력하세요"
        );
    }
    // 기존 실행기를 재사용한다. 후보는 이 자식 프로세스에서만 바뀐다.
    let bridge = "import sys,json,pathlib\nsys.path.insert(0,sys.argv[1])\nimport dispatch\np=json.load(sys.stdin)\ndispatch.CANDIDATES=p['models']\nprint(json.dumps(dispatch.dispatch(pathlib.Path(p['repo']),p['task']),ensure_ascii=False))";
    let bytes = serde_json::to_vec(&json!({"repo":repo,"models":config.models,"task":task}))?;
    ensure!(
        bytes.len() <= 64 * 1024,
        "작업 명세는 64KB 이하로 입력하세요"
    );
    let mut command = python(config);
    command
        .args(["-c", bridge])
        .arg(&config.harness_bin)
        .current_dir(repo);
    let (code, text) = run(command, Some(&bytes), 75)?;
    ensure!(
        code == 0,
        "위임 실행 결과를 확인하지 못했습니다 (종료 코드 {code}). 자동 재시도하지 않습니다"
    );
    serde_json::from_str(&text)
        .context("위임 영수증을 해석하지 못했습니다. 실행 여부를 확인한 뒤 재시도하세요")
}
pub fn model_report(config: &AppConfig, events: &[Value]) -> Result<Value> {
    ensure!(
        config.harness_bin.join("report.py").is_file(),
        "report.py를 찾지 못했습니다"
    );
    let mut file = tempfile::NamedTempFile::new()?;
    for event in events {
        serde_json::to_writer(&mut file, event)?;
        writeln!(file)?;
    }
    file.flush()?;
    let mut command = script(config, "report.py");
    command
        .arg("--file")
        .arg(file.path())
        .args(["--format", "json", "--by-model"]);
    let (code, text) = run(command, None, 60)?;
    ensure!(code == 0, "모델 사용량 집계 실패 (종료 코드 {code})");
    let value: Value = serde_json::from_str(&text)?;
    Ok(value.get("by_model").cloned().unwrap_or(Value::Null))
}
