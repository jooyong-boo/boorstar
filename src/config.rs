use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub repos: Vec<PathBuf>,
    pub harness_bin: PathBuf,
    pub harness_home: PathBuf,
    pub metrics: PathBuf,
    pub models: Models,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Model {
    pub model: String,
    pub effort: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Models {
    pub claude: Model,
    pub codex: Model,
}
impl Default for Models {
    fn default() -> Self {
        Self {
            claude: Model {
                model: "opus".into(),
                effort: "high".into(),
            },
            codex: Model {
                model: "gpt-6-astra".into(),
                effort: "low".into(),
            },
        }
    }
}
impl Models {
    pub fn validate(&self) -> Result<()> {
        for m in [&self.claude, &self.codex] {
            ensure!(
                !m.model.trim().is_empty()
                    && m.model.len() <= 128
                    && !m.model.chars().any(char::is_control),
                "모델 이름을 확인하세요"
            );
            ensure!(
                ["low", "medium", "high", "xhigh", "max"].contains(&m.effort.as_str()),
                "지원하지 않는 effort입니다"
            );
        }
        Ok(())
    }
}
impl AppConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let config: Self = serde_json::from_value(read_object(path)?)
            .context("앱 설정 형식이 올바르지 않습니다")?;
        config.models.validate()?;
        Ok(config)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        self.models.validate()?;
        patch_json(path, serde_json::to_value(self)?)
    }
    pub fn save_changes(&self, next: &Self, path: &Path) -> Result<Self> {
        next.models.validate()?;
        if !path.exists() {
            next.save(path)?;
            return Ok(next.clone());
        }
        let current = Self::load(path)?;
        let current_json = serde_json::to_value(&current)?;
        let before = serde_json::to_value(self)?;
        let after = serde_json::to_value(next)?;
        let mut patch = json!({});
        for (key, value) in after.as_object().context("앱 설정 형식 오류")? {
            if before[key] != *value {
                ensure!(
                    current_json[key] == before[key],
                    "다른 프로그램이 {key} 설정을 변경했습니다. 앱을 다시 열어 최신 설정을 불러오세요"
                );
                patch[key] = value.clone();
            }
        }
        patch_json(path, patch)?;
        Self::load(path)
    }
    pub fn add_repo(&mut self, path: &Path) -> Result<()> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--show-toplevel"]);
        let (code, output) = crate::harness::run(command, None, 10)?;
        ensure!(code == 0, "Git 레포 폴더를 선택하세요");
        let root = PathBuf::from(output.trim_end_matches(['\r', '\n'])).canonicalize()?;
        if !self.repos.contains(&root) {
            self.repos.push(root);
        }
        Ok(())
    }
    pub fn remove_repo(&mut self, path: &Path) {
        let root = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.repos.retain(|p| p != &root);
    }
}
pub fn read_object(path: &Path) -> Result<Value> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(e) => {
            return Err(e).with_context(|| format!("설정을 읽지 못했습니다: {}", path.display()));
        }
    };
    let value: Value = serde_json::from_slice(&bytes)
        .with_context(|| format!("JSON 설정이 손상됐습니다: {}", path.display()))?;
    ensure!(
        value.is_object(),
        "설정은 JSON 객체여야 합니다: {}",
        path.display()
    );
    Ok(value)
}
fn merge(base: &mut Value, patch: Value) {
    if let (Some(base), Value::Object(patch)) = (base.as_object_mut(), patch) {
        for (key, value) in patch {
            if value.is_null() {
                base.remove(&key);
            } else if value.is_object() && base.get(&key).is_some_and(Value::is_object) {
                merge(base.get_mut(&key).expect("existing key"), value);
            } else {
                base.insert(key, value);
            }
        }
    }
}
/// 알 수 없는 필드를 보존하고 같은 디렉터리에서 원자적으로 교체한다.
pub fn patch_json(path: &Path, patch: Value) -> Result<()> {
    ensure!(patch.is_object(), "설정 변경은 JSON 객체여야 합니다");
    let path = if path.is_symlink() {
        path.canonicalize()?
    } else {
        path.to_path_buf()
    };
    let before = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut value = read_object(&path)?;
    merge(&mut value, patch);
    let parent = path.parent().context("설정 경로에 상위 폴더가 없습니다")?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(meta) = fs::metadata(&path) {
        temp.as_file().set_permissions(meta.permissions())?;
    }
    serde_json::to_writer_pretty(&mut temp, &value)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    ensure!(
        fs::read(&path).ok() == before,
        "다른 프로그램이 설정을 변경했습니다. 다시 불러온 뒤 저장하세요"
    );
    temp.persist(&path).context("설정을 저장하지 못했습니다")?;
    Ok(())
}
fn dispatch_config(path: &Path) -> Result<Value> {
    let value = read_object(path)?;
    if let Some(mode) = value.get("mode") {
        ensure!(
            mode.as_str()
                .is_some_and(|s| ["off", "observe", "apply"].contains(&s)),
            "위임 모드 설정이 잘못됐습니다: {}",
            path.display()
        );
    }
    if let Some(disabled) = value.get("disabled") {
        ensure!(
            disabled.is_boolean(),
            "강제 중지 설정이 잘못됐습니다: {}",
            path.display()
        );
    }
    Ok(value)
}
pub fn effective_dispatch(home: &Path, repo: &Path) -> Result<Value> {
    let global = dispatch_config(&home.join("dispatch.json"))?;
    let local = dispatch_config(&repo.join(".harness/dispatch.json"))?;
    let disabled = global["disabled"].as_bool().unwrap_or(false);
    let (mode, source) = if disabled {
        ("off", "disabled")
    } else if let Some(m) = local["mode"].as_str() {
        (m, "repo")
    } else {
        (global["mode"].as_str().unwrap_or("off"), "global")
    };
    Ok(json!({"mode":mode,"source":source,"disabled":disabled,"global":global,"repo":local}))
}
pub fn save_dispatch(
    home: &Path,
    repo: &Path,
    global: bool,
    mode: Option<&str>,
    disabled: Option<bool>,
) -> Result<()> {
    if let Some(mode) = mode {
        ensure!(
            ["off", "observe", "apply"].contains(&mode),
            "지원하지 않는 위임 모드입니다"
        );
    }
    if !global && disabled.is_some() {
        bail!("강제 중지는 전역에서 설정하세요");
    }
    let path = if global {
        home.join("dispatch.json")
    } else {
        repo.join(".harness/dispatch.json")
    };
    dispatch_config(&path)?;
    let mut patch = json!({});
    if let Some(mode) = mode {
        patch["mode"] = mode.into();
    } else if !global {
        patch["mode"] = Value::Null;
    }
    if let Some(disabled) = disabled {
        patch["disabled"] = disabled.into();
    }
    patch_json(&path, patch)
}
