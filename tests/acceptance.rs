use boorstar::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn repo(path: &Path) {
    fs::create_dir_all(path).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}
fn fixture() -> (TempDir, AppConfig, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo space 한글");
    repo(&root);
    let config = AppConfig {
        harness_bin: dir.path().join("bin"),
        harness_home: dir.path().join("home"),
        metrics: dir.path().join("metrics.jsonl"),
        ..Default::default()
    };
    fs::create_dir_all(&config.harness_bin).unwrap();
    (dir, config, root)
}
#[test]
fn t01() {
    let (dir, mut c, root) = fixture();
    let file = dir.path().join("config.json");
    c.add_repo(&root).unwrap();
    c.add_repo(&root.join(".")).unwrap();
    c.save(&file).unwrap();
    let mut loaded = AppConfig::load(&file).unwrap();
    assert_eq!(loaded.repos, vec![root.canonicalize().unwrap()]);
    assert!(loaded.add_repo(dir.path()).is_err());
    loaded.remove_repo(&root);
    loaded.save(&file).unwrap();
    assert!(AppConfig::load(&file).unwrap().repos.is_empty());
}
#[test]
fn t02() {
    let (_dir, c, root) = fixture();
    fs::write(c.harness_bin.join("doctor.py"), "import sys\nfrom pathlib import Path\np=Path(sys.argv[sys.argv.index('--repo')+1])\nprint('준비됨' if (p/'AGENTS.md').exists() else 'AGENTS.md 없음')\nraise SystemExit(0 if (p/'AGENTS.md').exists() else 1)\n").unwrap();
    assert!(doctor(&c, &root).unwrap().contains("AGENTS.md 없음"));
    fs::write(root.join("AGENTS.md"), "지침").unwrap();
    assert!(doctor(&c, &root).unwrap().contains("준비됨"));
}
#[test]
fn t03() {
    let (_dir, c, root) = fixture();
    assert_eq!(
        effective_dispatch(&c.harness_home, &root).unwrap()["mode"],
        "off"
    );
    save_dispatch(&c.harness_home, &root, true, Some("observe"), Some(false)).unwrap();
    assert_eq!(
        effective_dispatch(&c.harness_home, &root).unwrap()["mode"],
        "observe"
    );
    save_dispatch(&c.harness_home, &root, false, Some("apply"), None).unwrap();
    assert_eq!(
        effective_dispatch(&c.harness_home, &root).unwrap()["source"],
        "repo"
    );
    save_dispatch(&c.harness_home, &root, true, None, Some(true)).unwrap();
    assert_eq!(
        effective_dispatch(&c.harness_home, &root).unwrap()["mode"],
        "off"
    );
    assert!(save_dispatch(&c.harness_home, &root, false, Some("bad"), None).is_err());
    save_dispatch(&c.harness_home, &root, false, None, None).unwrap();
    save_dispatch(&c.harness_home, &root, true, None, Some(false)).unwrap();
    assert_eq!(
        effective_dispatch(&c.harness_home, &root).unwrap()["mode"],
        "observe"
    );
}
#[test]
fn t04() {
    let (dir, mut c, root) = fixture();
    fs::write(c.harness_bin.join("dispatch.py"),"CANDIDATES={}\ndef dispatch(root,task):\n return {'candidates':CANDIDATES,'root':str(root),'summary':task['summary']}\n").unwrap();
    assert_eq!(c.models.claude.model, "opus");
    assert_eq!(c.models.codex.model, "gpt-6-astra");
    c.models.codex.model = "test-model".into();
    c.models.codex.effort = "high".into();
    c.save(&dir.path().join("config.json")).unwrap();
    let c = AppConfig::load(&dir.path().join("config.json")).unwrap();
    let output = dispatch(
        &c,
        &root,
        json!({"original":"codex","kind":"analysis","summary":"한글 ' $()","spec":"읽기"}),
    )
    .unwrap();
    assert_eq!(output["candidates"]["codex"]["model"], "test-model");
    assert_eq!(output["candidates"]["codex"]["effort"], "high");
    assert_eq!(output["summary"], "한글 ' $()");
    assert_eq!(
        fs::read_to_string(c.harness_bin.join("dispatch.py"))
            .unwrap()
            .lines()
            .next(),
        Some("CANDIDATES={}")
    );
}
#[test]
fn t05() {
    use sha1::{Digest, Sha1};
    let (_dir, c, root) = fixture();
    let empty = snapshot(&c, &root, "").unwrap();
    assert!(empty.loop_state.is_none());
    assert!(empty.events.is_empty());
    assert!(!empty.observed_at.is_empty());
    let key = format!(
        "{:x}",
        Sha1::digest(root.canonicalize().unwrap().to_string_lossy().as_bytes())
    );
    let state = c.harness_home.join("harness-state");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join(format!(
            "loop-{}-{}.json",
            root.file_name().unwrap().to_str().unwrap(),
            &key[..12]
        )),
        json!({"active":true,"round":2,"repo":root,"last_missing":["rust-desktop:T-01"]})
            .to_string(),
    )
    .unwrap();
    let event = json!({"ts":"2026-11-17T01:00:00+0000","repo":root,"hook":"stop_verify","decision":"block"});
    fs::write(&c.metrics, format!("{}\n{{broken\n", event)).unwrap();
    let s = snapshot(&c, &root, "").unwrap();
    assert_eq!(s.loop_state.unwrap()["round"], 2);
    assert_eq!(s.events.len(), 1);
    assert!(!s.warnings.is_empty());
    fs::write(&c.metrics,format!("{}\n",json!({"ts":"2026-11-17T02:00:00+0000","repo":root,"hook":"stop_verify","decision":"pass"}))).unwrap();
    assert_eq!(snapshot(&c, &root, "").unwrap().summary.passed, 1);
}
#[test]
fn t06() {
    let (_dir, c, root) = fixture();
    assert!(
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args([
                "remote",
                "add",
                "origin",
                "https://example.invalid/team/harness-demo.git"
            ])
            .status()
            .unwrap()
            .success()
    );
    let rows = [
        json!({"ts":"2026-11-17T01:00:00+0000","repo":"harness-demo","hook":"stop_verify","decision":"pass"}),
        json!({"ts":"2026-11-17T01:00:00+0000","repo":root,"hook":"stop_verify","decision":"pass"}),
        json!({"ts":"2026-11-17T01:01:00+0000","repo":root,"hook":"stop_verify","decision":"block"}),
        json!({"ts":"2026-11-17T01:02:00+0000","repo":root,"hook":"stop_verify","decision":"pass","steps_run":0,"files":1}),
        json!({"ts":"2026-11-17T01:03:00+0000","repo":root,"hook":"dispatch","decision":"started"}),
        json!({"ts":"2026-11-17T01:04:00+0000","repo":root,"hook":"dispatch","decision":"verified","outcome":"passed"}),
        json!({"ts":"2026-11-16T01:00:00+0000","repo":root,"hook":"stop_verify","decision":"pass"}),
        json!({"ts":"2026-11-17T01:00:00+0000","repo":"/other","hook":"stop_verify","decision":"pass"}),
    ];
    fs::write(
        &c.metrics,
        rows.iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let s = snapshot(&c, &root, "2026-11-17").unwrap().summary;
    assert_eq!(
        (s.passed, s.failed, s.dispatch_started, s.verified),
        (2, 1, 1, 1)
    );
    assert_eq!(s.tokens, None);
    assert!(snapshot(&c, &root, "not-a-date").is_err());
}
#[test]
fn t07() {
    let (dir, c, root) = fixture();
    let file = dir.path().join("config.json");
    fs::write(&file, r#"{"keep":{"a":1},"mode":"off"}"#).unwrap();
    patch_json(&file, json!({"mode":"observe"})).unwrap();
    let saved: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["keep"]["a"], 1);
    fs::write(&file, "{broken").unwrap();
    assert!(patch_json(&file, json!({"mode":"off"})).is_err());
    assert_eq!(fs::read_to_string(&file).unwrap(), "{broken");
    assert!(AppConfig::load(&file).is_err());
    assert!(doctor(&c, &root).is_err());
    fs::write(c.harness_bin.join("doctor.py"), "raise SystemExit(2)\n").unwrap();
    assert!(doctor(&c, &root).is_err());
    fs::create_dir_all(&c.harness_home).unwrap();
    fs::write(
        c.harness_home.join("dispatch.json"),
        r#"{"disabled":"yes"}"#,
    )
    .unwrap();
    assert!(effective_dispatch(&c.harness_home, &root).is_err());
}

#[test]
fn handles_large_stdin_timeout_and_unknown_settings() {
    let (dir, c, root) = fixture();
    let file = dir.path().join("config.json");
    fs::write(&file,r#"{"custom":{"keep":true},"models":{"codex":{"extra":7,"model":"gpt-6-astra","effort":"low"},"claude":{"model":"opus","effort":"high"}}}"#).unwrap();
    c.save(&file).unwrap();
    let loaded: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(loaded["custom"]["keep"], true);
    assert_eq!(loaded["models"]["codex"]["extra"], 7);
    let mut child = Command::new("python3");
    child.args(["-I", "-c", "import time; time.sleep(2)"]);
    let start = std::time::Instant::now();
    assert!(run(child, Some(&vec![b'a'; 65536]), 0).is_err());
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    fs::write(&c.metrics,format!("{}\n{}\n{}\n",
        json!({"ts":"2026-11-16T01:00:00+0000","repo":root,"hook":"dispatch","decision":"started","dispatch_id":"d1"}),
        json!({"ts":"2026-11-17T01:00:00+0000","hook":"dispatch","decision":"verified","outcome":"passed","dispatch_id":"d1"}),
        json!({"ts":"2026-11-17T01:00:00+0000","hook":"dispatch","decision":"verified","outcome":"passed","dispatch_id":"other"})
    )).unwrap();
    let snap = snapshot(&c, &root, "2026-11-17").unwrap();
    assert_eq!(snap.summary.dispatch_started, 0);
    assert_eq!(snap.summary.verified, 1);
}

#[test]
#[ignore = "설치된 하네스 경로를 명시해 로컬 계약을 검증"]
fn installed_harness_contract() {
    let (_dir, mut c, root) = fixture();
    c.harness_bin = std::env::var_os("BOORSTAR_HARNESS_BIN")
        .expect("하네스 bin 경로 필요")
        .into();
    assert!(doctor(&c, &root).unwrap().contains("verify:quick"));
    let r=dispatch(&c,&root,json!({"original":"codex","kind":"analysis","summary":"격리된 계약 테스트","spec":"실행하지 않음"})).unwrap();
    assert_eq!(r["action"], "fallback");
    assert_eq!(r["mode"], "off");
    assert_eq!(snapshot(&c, &root, "").unwrap().summary.fallback, 1);
    assert_eq!(model_report(&c, &[]).unwrap()["missing_sessions"], 0);
    assert_eq!(progress(&c, &root).unwrap()["enabled"], false);
}

#[test]
fn changing_models_preserves_external_repo_edits() {
    let (dir, c, root) = fixture();
    let file = dir.path().join("settings.json");
    c.save(&file).unwrap();
    let mut external = c.clone();
    external.add_repo(&root).unwrap();
    external.save(&file).unwrap();
    let mut changed = c.clone();
    changed.models.codex.model = "new-model".into();
    let saved = c.save_changes(&changed, &file).unwrap();
    assert_eq!(saved.repos, external.repos);
    assert_eq!(saved.models.codex.model, "new-model");
    assert!(c.save_changes(&changed, &file).is_err());
}
