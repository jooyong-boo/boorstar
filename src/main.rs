use boorstar::*;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, RichText};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(17, 20, 25);
const PANEL: Color32 = Color32::from_rgb(24, 29, 36);
const ACCENT: Color32 = Color32::from_rgb(245, 166, 89);
const MUTED: Color32 = Color32::from_rgb(146, 158, 174);
const GREEN: Color32 = Color32::from_rgb(97, 206, 165);
const RED: Color32 = Color32::from_rgb(246, 126, 126);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Repositories,
    Harness,
    Agents,
    Analysis,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "모니터링",
            Self::Repositories => "레포 연결",
            Self::Harness => "하네스 설정",
            Self::Agents => "에이전트",
            Self::Analysis => "분석",
        }
    }
    fn subtitle(self) -> &'static str {
        match self {
            Self::Overview => "여러 작업의 흐름을, 한 곳에서.",
            Self::Repositories => "로컬 레포를 연결하고 준비 상태를 확인하세요.",
            Self::Harness => "전역 기본값과 레포별 설정을 관리하세요.",
            Self::Agents => "작업에 맞는 모델을 선택하고 위임하세요.",
            Self::Analysis => "관측된 결과로 작업 흐름을 돌아보세요.",
        }
    }
}
enum JobResult {
    Doctor(String),
    Dispatch(Value),
    Models(Value),
    Progress(Value),
}
struct Job {
    repo: PathBuf,
    since: String,
    rx: Receiver<Result<JobResult, String>>,
}
struct Monitor {
    repo: PathBuf,
    since: String,
    rx: Receiver<Result<Snapshot, String>>,
}

struct App {
    config: AppConfig,
    config_path: PathBuf,
    selected: Option<PathBuf>,
    page: Page,
    new_repo: String,
    repo_pending: Option<Receiver<Result<PathBuf, String>>>,
    bin: String,
    home: String,
    metrics: String,
    draft_models: Models,
    status: String,
    error: Option<String>,
    config_invalid: bool,
    doctor_text: String,
    state: Option<Snapshot>,
    dispatch_state: Option<Value>,
    global_mode: String,
    repo_mode: String,
    disabled: bool,
    since: String,
    since_input: String,
    monitor: Option<Monitor>,
    last_refresh: Instant,
    auto_refresh: bool,
    job: Option<Job>,
    model_usage: Option<Value>,
    progress: Option<Value>,
    dispatch_results: HashMap<PathBuf, Value>,
    original: String,
    kind: String,
    summary: String,
    spec: String,
}
fn default_bin(home: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("BOORSTAR_HARNESS_BIN") {
        return p.into();
    }
    for base in [
        home.join(".codex/plugins/cache/boo/harness-core"),
        home.join(".claude/plugins/cache/boo/harness-core"),
    ] {
        let mut paths: Vec<_> = std::fs::read_dir(base)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("bin/doctor.py").is_file())
            .collect();
        paths.sort_by_key(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .split('.')
                .map(|x| x.parse::<u32>().unwrap_or(0))
                .collect::<Vec<_>>()
        });
        if let Some(path) = paths.last() {
            return path.join("bin");
        }
    }
    PathBuf::new()
}
impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = FontDefinitions::default();
        if let Ok(data) = std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc") {
            fonts
                .font_data
                .insert("korean".into(), FontData::from_owned(data).into());
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("korean".into());
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = PANEL;
        style.visuals.selection.bg_fill = Color32::from_rgb(96, 65, 39);
        style.visuals.selection.stroke = egui::Stroke::new(1.0_f32, ACCENT);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(36, 43, 53);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(14.0, 9.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
        cc.egui_ctx.set_style(style);
        let user_home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let app_home = std::env::var_os("BOORSTAR_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| user_home.join("Library/Application Support/Boorstar"));
        let config_path = app_home.join("config.json");
        let (mut config, error) = match AppConfig::load(&config_path) {
            Ok(c) => (c, None),
            Err(e) => (AppConfig::default(), Some(e.to_string())),
        };
        if !config_path.exists() {
            config.harness_home = std::env::var_os("HARNESS_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| user_home.join(".claude"));
            config.metrics = std::env::var_os("HARNESS_METRICS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| config.harness_home.join("harness-metrics.jsonl"));
            config.harness_bin = default_bin(&user_home);
        }
        let selected = config.repos.first().cloned();
        let mut app = Self {
            bin: config.harness_bin.display().to_string(),
            home: config.harness_home.display().to_string(),
            metrics: config.metrics.display().to_string(),
            draft_models: config.models.clone(),
            config,
            config_path,
            selected,
            page: Page::Overview,
            new_repo: String::new(),
            repo_pending: None,
            status: "로컬에서 실행 중".into(),
            config_invalid: error.is_some(),
            error,
            doctor_text: String::new(),
            state: None,
            dispatch_state: None,
            global_mode: "off".into(),
            repo_mode: "inherit".into(),
            disabled: false,
            since: String::new(),
            since_input: String::new(),
            monitor: None,
            last_refresh: Instant::now(),
            auto_refresh: true,
            job: None,
            model_usage: None,
            progress: None,
            dispatch_results: HashMap::new(),
            original: "codex".into(),
            kind: "analysis".into(),
            summary: String::new(),
            spec: String::new(),
        };
        app.load_dispatch();
        app.refresh();
        app
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.error = Some(error.to_string());
    }
    fn load_dispatch(&mut self) {
        self.dispatch_state = None;
        if let Some(repo) = &self.selected {
            match effective_dispatch(&self.config.harness_home, repo) {
                Ok(value) => {
                    self.global_mode = value["global"]["mode"].as_str().unwrap_or("off").into();
                    self.repo_mode = value["repo"]["mode"].as_str().unwrap_or("inherit").into();
                    self.disabled = value["disabled"].as_bool().unwrap_or(false);
                    self.dispatch_state = Some(value);
                }
                Err(e) => self.fail(e),
            }
        }
    }
    fn select(&mut self, repo: PathBuf) {
        self.selected = Some(repo);
        self.state = None;
        self.doctor_text.clear();
        self.model_usage = None;
        self.progress = None;

        self.monitor = None;
        self.load_dispatch();
        self.refresh();
    }
    fn refresh(&mut self) {
        if self.monitor.is_some() {
            return;
        }
        if let Some(repo) = self.selected.clone() {
            let config = self.config.clone();
            let since = self.since.clone();
            let (tx, rx) = mpsc::channel();
            self.monitor = Some(Monitor {
                repo: repo.clone(),
                since: since.clone(),
                rx,
            });
            self.last_refresh = Instant::now();
            thread::spawn(move || {
                let _ = tx.send(snapshot(&config, &repo, &since).map_err(|e| e.to_string()));
            });
        }
    }
    fn start_job(
        &mut self,
        action: impl FnOnce(AppConfig, PathBuf) -> anyhow::Result<JobResult> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        if let Some(repo) = self.selected.clone() {
            let config = self.config.clone();
            let (tx, rx) = mpsc::channel();
            self.job = Some(Job {
                repo: repo.clone(),
                since: self.since.clone(),
                rx,
            });
            thread::spawn(move || {
                let _ = tx.send(action(config, repo).map_err(|e| e.to_string()));
            });
        }
    }
    fn poll(&mut self) {
        if let Some(result) = self.repo_pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.repo_pending = None;
            match result {
                Ok(repo) => {
                    let mut next = self.config.clone();
                    if !next.repos.contains(&repo) {
                        next.repos.push(repo.clone());
                    }
                    if self.save_app(next) {
                        self.new_repo.clear();
                        self.select(repo);
                    }
                }
                Err(e) => self.fail(e),
            }
        }

        if let Some(result) = self.monitor.as_ref().and_then(|m| m.rx.try_recv().ok()) {
            let monitor = self.monitor.take().unwrap();
            if self.selected.as_ref() == Some(&monitor.repo) && monitor.since == self.since {
                match result {
                    Ok(s) => {
                        self.state = Some(s);
                    }
                    Err(e) => {
                        self.state = None;
                        self.fail(e);
                    }
                }
            }
        }
        if let Some(result) = self.job.as_ref().and_then(|j| j.rx.try_recv().ok()) {
            let job = self.job.take().unwrap();
            match result {
                Err(error) => self.fail(format!("{}: {error}", job.repo.display())),
                Ok(JobResult::Dispatch(value)) => {
                    self.dispatch_results.insert(job.repo.clone(), value);
                    self.status = format!(
                        "{}의 위임 응답을 받았습니다",
                        job.repo.file_name().unwrap_or_default().to_string_lossy()
                    );
                    self.refresh();
                }
                Ok(value)
                    if self.selected.as_ref() == Some(&job.repo) && job.since == self.since =>
                {
                    match value {
                        JobResult::Doctor(text) => {
                            self.doctor_text = text;
                            self.status = "도입 점검을 마쳤습니다".into();
                        }
                        JobResult::Models(value) => self.model_usage = Some(value),
                        JobResult::Progress(value) => self.progress = Some(value),
                        JobResult::Dispatch(_) => unreachable!(),
                    }
                }
                _ => {
                    self.status = "조회 중 선택 범위가 바뀌어 이전 조회 결과를 제외했습니다".into()
                }
            }
        }

        if self.auto_refresh && self.last_refresh.elapsed() > Duration::from_secs(5) {
            self.refresh();
        }
    }
    fn save_app(&mut self, next: AppConfig) -> bool {
        if self.config_invalid {
            self.fail("손상된 앱 설정을 먼저 복구하고 앱을 다시 실행하세요");
            return false;
        }
        match self.config.save_changes(&next, &self.config_path) {
            Ok(saved) => {
                self.config = saved;
                self.status = "설정을 저장했습니다".into();
                self.error = None;
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(220.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(20))
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(30.0, 32.0), egui::Sense::hover());
                    let c = rect.center();
                    let points = [
                        (0.0, -13.0),
                        (3.0, -3.0),
                        (13.0, 0.0),
                        (3.0, 3.0),
                        (0.0, 13.0),
                        (-3.0, 3.0),
                        (-13.0, 0.0),
                        (-3.0, -3.0),
                    ]
                    .map(|(x, y)| c + egui::vec2(x, y));
                    // 네 개의 삼각형으로 별을 그려 폰트의 기호 지원에 의존하지 않는다.
                    for i in (0..8).step_by(2) {
                        ui.painter().add(egui::Shape::convex_polygon(
                            vec![c, points[(i + 7) % 8], points[i], points[(i + 1) % 8]],
                            ACCENT,
                            egui::Stroke::NONE,
                        ));
                    }
                    ui.label(RichText::new("Boorstar").size(25.0).strong());
                });
                ui.label(
                    RichText::new("나만의 에이전트 작업실")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(36.0);
                for (i, page) in [
                    Page::Overview,
                    Page::Repositories,
                    Page::Harness,
                    Page::Agents,
                    Page::Analysis,
                ]
                .into_iter()
                .enumerate()
                {
                    let label = format!("0{}   {}", i + 1, page.title());
                    if ui
                        .add_sized(
                            [180.0, 43.0],
                            egui::Button::new(RichText::new(label).color(if self.page == page {
                                ACCENT
                            } else {
                                Color32::LIGHT_GRAY
                            }))
                            .selected(self.page == page),
                        )
                        .clicked()
                    {
                        self.page = page;
                    }
                }
                ui.add_space(30.0);
                ui.label(RichText::new("연결된 레포").size(12.0).color(MUTED));
                for repo in self.config.repos.clone() {
                    let name = repo
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if ui
                        .selectable_label(self.selected.as_ref() == Some(&repo), name)
                        .on_hover_text(repo.display().to_string())
                        .clicked()
                    {
                        self.select(repo);
                    }
                }
                if self.config.repos.is_empty() {
                    ui.label(
                        RichText::new("아직 연결된 레포가 없어요")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new("Rust · 로컬 데스크톱")
                            .size(11.0)
                            .color(MUTED),
                    );
                    ui.label(RichText::new("●  BOORSTAR  /  0.1").size(11.0).color(GREEN));
                });
            });
    }
    fn header(&mut self, ui: &mut egui::Ui) {
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new(self.page.title()).size(30.0));
                ui.label(RichText::new(self.page.subtitle()).color(MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("새로고침").clicked() {
                    self.load_dispatch();
                    self.refresh();
                }
                if self.job.is_some() || self.monitor.is_some() || self.repo_pending.is_some() {
                    ui.spinner();
                }
                ui.checkbox(&mut self.auto_refresh, "5초 갱신");
            });
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    self.selected
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "레포를 연결하면 시작할 수 있습니다".into()),
                )
                .size(12.0)
                .color(MUTED),
            );
        });
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(14.0);
        if let Some(error) = self.error.clone() {
            egui::Frame::new()
                .fill(Color32::from_rgb(59, 32, 35))
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(RED, error);
                        if ui.small_button("닫기").clicked() {
                            self.error = None;
                        }
                    });
                });
            ui.add_space(12.0);
        }
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        if self.selected.is_some()
            && ui
                .add_enabled(self.job.is_none(), egui::Button::new("완료 조건·단계 조회"))
                .clicked()
        {
            self.start_job(|c, r| Ok(JobResult::Progress(progress(&c, &r)?)));
        }
        if let Some(value) = &self.progress {
            card(ui, |ui| {
                ui.heading("작업 단계");
                if let Some(tasks) = value["stages"].as_object().filter(|t| !t.is_empty()) {
                    for (name, stages) in tasks {
                        ui.strong(name);
                        if let Some(stages) = stages.as_array() {
                            for stage in stages {
                                ui.label(format!(
                                    "{} {}  ·  {} / {}",
                                    if stage["current"] == true {
                                        "→"
                                    } else {
                                        "·"
                                    },
                                    stage["title"].as_str().unwrap_or(""),
                                    stage["done"],
                                    stage["total"]
                                ));
                            }
                        }
                    }
                } else {
                    ui.label("확정된 작업 단계가 없습니다.");
                }
                ui.small("조회 버튼을 누른 시점의 하네스 판정입니다.");
            });
            ui.add_space(12.0);
        }

        let Some(state) = &self.state else {
            ui.add_space(40.0);
            ui.heading("작업의 흐름을 연결하세요");
            ui.label("로컬 Git 레포를 추가하면 하네스 상태와 최근 검증 결과가 여기에 표시됩니다.");
            if ui.button("레포 연결하기 →").clicked() {
                self.page = Page::Repositories;
            }
            return;
        };
        ui.columns(4, |cols| {
            metric(&mut cols[0], "검증 통과", state.summary.passed, GREEN);
            metric(&mut cols[1], "검증 실패", state.summary.failed, RED);
            metric(
                &mut cols[2],
                "위임 시작",
                state.summary.dispatch_started,
                ACCENT,
            );
            metric(
                &mut cols[3],
                "결과 검증 통과",
                state.summary.verified,
                GREEN,
            );
        });
        ui.add_space(22.0);
        card(ui, |ui| {
            ui.heading("현재 루프");
            if let Some(value) = &state.loop_state {
                let active = value["active"].as_bool().unwrap_or(false);
                ui.horizontal(|ui| {
                    ui.colored_label(
                        if active { GREEN } else { MUTED },
                        if active {
                            "● 진행 중"
                        } else {
                            "● 종료"
                        },
                    );
                    ui.label(format!(
                        "라운드 {} / {}",
                        value["round"].as_u64().unwrap_or(0),
                        value["max_rounds"].as_u64().unwrap_or(0)
                    ));
                    ui.label(format!(
                        "남은 항목 {}",
                        value["last_missing"].as_array().map_or(0, Vec::len)
                    ));
                });
                if let Some(items) = value["last_missing"].as_array() {
                    for item in items.iter().take(8) {
                        ui.label(item.as_str().unwrap_or(""));
                    }
                }
                if let Some(reason) = value["end_reason"].as_str() {
                    ui.label(format!("종료 이유: {reason}"));
                }
            } else {
                ui.colored_label(MUTED, "관측된 루프가 없습니다");
            }
            ui.small(format!(
                "마지막 조회: {} · 현재 선택한 레포의 상태 파일 기준",
                state.observed_at
            ));
        });
        ui.add_space(18.0);
        ui.heading("최근 활동");
        for warning in &state.warnings {
            ui.colored_label(ACCENT, warning);
        }
        if state.events.is_empty() {
            ui.colored_label(MUTED, "선택한 기간에 관측된 계측이 없습니다.");
        }
        egui::Grid::new("events")
            .num_columns(3)
            .striped(true)
            .min_col_width(130.0)
            .show(ui, |ui| {
                ui.strong("시각");
                ui.strong("단계");
                ui.strong("결과");
                ui.end_row();
                for event in state.events.iter().rev().take(50) {
                    ui.small(event["ts"].as_str().unwrap_or("—"));
                    ui.label(event["hook"].as_str().unwrap_or("—"));
                    ui.label(event["decision"].as_str().unwrap_or("—"));
                    ui.end_row();
                }
            });
    }
    fn repositories(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.heading("레포 추가");
            ui.label("Git 레포의 로컬 폴더 경로를 입력하세요. 원격 저장소 복제는 하지 않습니다.");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_repo)
                        .hint_text("/Users/…/my-project")
                        .desired_width(430.0),
                );
                if ui
                    .add_enabled(self.repo_pending.is_none(), egui::Button::new("연결"))
                    .clicked()
                {
                    let path = PathBuf::from(self.new_repo.trim());
                    let (tx, rx) = mpsc::channel();
                    self.repo_pending = Some(rx);
                    thread::spawn(move || {
                        let mut config = AppConfig::default();
                        let result = config
                            .add_repo(&path)
                            .map(|()| config.repos.remove(0))
                            .map_err(|e| e.to_string());
                        let _ = tx.send(result);
                    });
                }
            });
        });
        ui.add_space(18.0);
        if let Some(repo) = self.selected.clone() {
            ui.horizontal(|ui| {
                ui.heading(repo.file_name().unwrap_or_default().to_string_lossy());
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("도입 상태 점검"))
                    .clicked()
                {
                    self.start_job(|c, r| Ok(JobResult::Doctor(doctor(&c, &r)?)));
                }
                if ui
                    .button("목록에서 제거")
                    .on_hover_text("레포 파일은 삭제하지 않습니다")
                    .clicked()
                {
                    let mut next = self.config.clone();
                    next.remove_repo(&repo);
                    if self.save_app(next) {
                        self.selected = self.config.repos.first().cloned();
                        self.state = None;
                        self.model_usage = None;
                        self.progress = None;
                        self.monitor = None;
                        self.doctor_text.clear();
                        self.load_dispatch();
                        self.refresh();
                    }
                }
            });
            if !self.doctor_text.is_empty() {
                card(ui, |ui| {
                    ui.label(&self.doctor_text);
                });
            }
        }
        ui.add_space(24.0);
        ui.collapsing("하네스 연결 경로", |ui| {
            ui.label("설치된 harness-core의 bin 폴더와 계측 위치를 지정합니다.");
            field(ui, "하네스 bin", &mut self.bin);
            field(ui, "하네스 홈", &mut self.home);
            field(ui, "계측 JSONL", &mut self.metrics);
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("연결 경로 저장"))
                .clicked()
            {
                let mut next = self.config.clone();
                next.harness_bin = self.bin.trim().into();
                next.harness_home = self.home.trim().into();
                next.metrics = self.metrics.trim().into();
                if !next.harness_bin.join("doctor.py").is_file() {
                    self.fail("선택한 bin 폴더에 doctor.py가 없습니다");
                } else if !next.harness_home.is_absolute() || !next.metrics.is_absolute() {
                    self.fail("하네스 홈과 계측 파일은 절대 경로로 지정하세요");
                } else if self.save_app(next) {
                    self.monitor = None;
                    self.state = None;
                    self.model_usage = None;
                    self.progress = None;
                    self.doctor_text.clear();
                    self.load_dispatch();
                    self.refresh();
                }
            }
            ui.small(format!("앱 설정: {}", self.config_path.display()));
        });
    }
    fn harness(&mut self, ui: &mut egui::Ui) {
        let Some(repo) = self.selected.clone() else {
            ui.label("먼저 레포를 연결하세요.");
            return;
        };
        let Some(value) = self.dispatch_state.clone() else {
            ui.label("위임 설정을 읽지 못했습니다. 파일을 복구한 뒤 새로고침하세요.");
            return;
        };
        card(ui, |ui| {
            ui.label(RichText::new("실제 적용값").color(MUTED));
            ui.heading(
                RichText::new(value["mode"].as_str().unwrap_or("off"))
                    .color(ACCENT)
                    .size(28.0),
            );
            ui.label(format!(
                "설정 출처: {}",
                match value["source"].as_str() {
                    Some("repo") => "현재 레포",
                    Some("disabled") => "전역 강제 중지",
                    _ => "전역 기본값",
                }
            ));
        });
        ui.add_space(20.0);
        ui.columns(2, |cols| {
            card(&mut cols[0], |ui| {
                ui.heading("전역 기본값");
                combo(
                    ui,
                    "global",
                    &mut self.global_mode,
                    &["off", "observe", "apply"],
                );
                ui.checkbox(&mut self.disabled, "모든 레포의 교차 위임 중지");
                if ui.button("전역 설정 저장").clicked() {
                    match save_dispatch(
                        &self.config.harness_home,
                        &repo,
                        true,
                        Some(&self.global_mode),
                        Some(self.disabled),
                    ) {
                        Ok(()) => {
                            self.status = "전역 설정을 저장했습니다".into();
                            self.load_dispatch();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            });
            card(&mut cols[1], |ui| {
                ui.heading("현재 레포");
                combo(
                    ui,
                    "repo",
                    &mut self.repo_mode,
                    &["inherit", "off", "observe", "apply"],
                );
                ui.label("inherit: 전역 기본값을 따릅니다.");
                if ui.button("레포 설정 저장").clicked() {
                    let mode = if self.repo_mode == "inherit" {
                        None
                    } else {
                        Some(self.repo_mode.as_str())
                    };
                    match save_dispatch(&self.config.harness_home, &repo, false, mode, None) {
                        Ok(()) => {
                            self.status = "레포 설정을 저장했습니다".into();
                            self.load_dispatch();
                        }
                        Err(e) => self.fail(e),
                    }
                }
            });
        });
        ui.add_space(20.0);
        ui.label("off  ·  추천·실행 없이 기존 방식으로 복귀");
        ui.label("observe  ·  Jev 추천만 조회, 에이전트는 실행하지 않음");
        ui.label("apply  ·  Jev 추천에 따라 Orca 워커 실행");
        ui.small("이 설정은 기존 하네스와 공유됩니다. 전역 중지는 레포별 설정보다 우선합니다.");
    }
    fn agents(&mut self, ui: &mut egui::Ui) {
        ui.label("저장한 모델은 Boorstar에서 시작하는 다음 위임에 적용됩니다.");
        ui.colored_label(
            MUTED,
            "기존 터미널·스킬 호출과 이미 실행 중인 에이전트는 변경하지 않습니다.",
        );
        ui.add_space(16.0);
        ui.columns(2, |cols| {
            model_card(&mut cols[0], "Claude", &mut self.draft_models.claude);
            model_card(&mut cols[1], "Codex", &mut self.draft_models.codex);
        });
        ui.horizontal(|ui| {
            if ui.button("모델 설정 저장").clicked() {
                let mut next = self.config.clone();
                next.models = self.draft_models.clone();
                self.save_app(next);
            }
            if self.draft_models != self.config.models {
                ui.colored_label(ACCENT, "저장되지 않은 변경");
            }
        });
        ui.add_space(25.0);
        ui.separator();
        ui.add_space(16.0);
        ui.heading("작업 위임");
        ui.label("observe·apply에서는 작업 요약이 Jev로 전송됩니다. apply는 Orca에 전체 명세를 전달하고 에이전트를 실행합니다.");
        ui.horizontal(|ui| {
            ui.label("기존 담당");
            combo(ui, "original", &mut self.original, &["codex", "claude"]);
            ui.label("종류");
            combo(
                ui,
                "kind",
                &mut self.kind,
                &["analysis", "implement", "review"],
            );
        });
        field(ui, "작업 요약", &mut self.summary);
        ui.add(
            egui::TextEdit::multiline(&mut self.spec)
                .hint_text("대상 레포·변경 범위·완료 조건을 구체적으로 적으세요.")
                .desired_rows(5)
                .desired_width(f32::INFINITY),
        );
        let can_run = self.selected.is_some()
            && self.job.is_none()
            && !self.summary.trim().is_empty()
            && !self.spec.trim().is_empty();
        if ui
            .add_enabled(can_run, egui::Button::new("저장된 설정으로 위임 실행"))
            .clicked()
        {
            let task = json!({"original":self.original,"kind":self.kind,"summary":self.summary,"spec":self.spec});

            self.start_job(move |c, r| Ok(JobResult::Dispatch(dispatch(&c, &r, task)?)));
        }
        if let Some(result) = self
            .selected
            .as_ref()
            .and_then(|repo| self.dispatch_results.get(repo))
        {
            card(ui, |ui| {
                let action = result["action"].as_str().unwrap_or("unknown");
                ui.heading(match action {
                    "started" => "워커 시작 영수증 수신",
                    "fallback" => "기존 방식으로 복귀 — 앱에서 워커를 실행하지 않음",
                    "reconcile" => "실행 여부 확인 필요 — 재실행 전 Orca 확인",
                    _ => "응답 확인",
                });
                for key in ["reason", "mode", "recommended", "selected", "dispatch_id"] {
                    if let Some(s) = result[key].as_str() {
                        ui.label(format!("{key}: {s}"));
                    }
                }
                ui.small("실행 영수증은 구현·검증 완료를 의미하지 않습니다.");
            });
        }
    }
    fn analysis(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("시작일");
            ui.add(
                egui::TextEdit::singleline(&mut self.since_input)
                    .hint_text("YYYY-MM-DD / 비우면 전체")
                    .desired_width(235.0),
            );
            if ui.button("기간 적용").clicked() {
                self.since = self.since_input.trim().to_owned();
                self.state = None;
                self.model_usage = None;
                self.progress = None;
                self.monitor = None;
                self.refresh();
            }
        });
        ui.small("로컬 날짜 기준 · 선택한 레포만 집계 · 모니터링에도 같은 기간 적용");
        ui.add_space(18.0);
        if let Some(state) = &self.state {
            ui.columns(3, |cols| {
                metric(
                    &mut cols[0],
                    "실행한 검증",
                    state.summary.passed + state.summary.failed,
                    ACCENT,
                );
                metric(&mut cols[1], "통과", state.summary.passed, GREEN);
                metric(&mut cols[2], "실패", state.summary.failed, RED);
            });
            ui.add_space(18.0);
            card(ui, |ui| {
                ui.heading("검증 결과 분포");
                let total = state.summary.passed + state.summary.failed;
                if total == 0 {
                    ui.label("아직 집계할 검증이 없습니다.");
                } else {
                    ui.add(
                        egui::ProgressBar::new(state.summary.passed as f32 / total as f32)
                            .fill(GREEN)
                            .text(
                                RichText::new(format!(
                                    "통과 {} / 전체 {}",
                                    state.summary.passed, total
                                ))
                                .color(Color32::from_rgb(12, 45, 34)),
                            ),
                    );
                }
                ui.small(
                    "문서 변경만으로 건너뛴 통과는 제외합니다. 실행 횟수이며 사용자 수가 아닙니다.",
                );
            });
            ui.add_space(16.0);
            ui.label(format!(
                "위임 시작 {} · 복귀 {} · 실행 불명확 {} · 결과 검증 통과 {} · 실패 {}",
                state.summary.dispatch_started,
                state.summary.fallback,
                state.summary.uncertain,
                state.summary.verified,
                state.summary.verification_failed
            ));
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("모델·토큰 정보 조회"))
                .clicked()
            {
                let events = state.events.clone();
                self.start_job(move |c, _| Ok(JobResult::Models(model_report(&c, &events)?)));
            }
        }
        ui.small("선택 기간의 계측에 연결된 세션 전체 토큰입니다. 기간 내 소비량·실제 청구액과 다릅니다.");
        if let Some(value) = &self.model_usage {
            ui.add_space(12.0);
            if let Some(rows) = value["usage"].as_object().filter(|r| !r.is_empty()) {
                egui::Grid::new("usage").striped(true).show(ui, |ui| {
                    ui.strong("에이전트 · 모델");
                    ui.strong("토큰 정보");
                    ui.end_row();
                    for (key, row) in rows {
                        ui.label(key);
                        ui.label(
                            row["total_tokens"]
                                .as_u64()
                                .map(|n| n.to_string())
                                .unwrap_or_else(|| "미수집".into()),
                        );
                        ui.end_row();
                    }
                });
            } else {
                ui.colored_label(MUTED, "연결 가능한 모델·토큰 기록이 없습니다.");
            }
            ui.small(format!(
                "연결되지 않은 세션: {}",
                value["missing_sessions"].as_u64().unwrap_or(0)
            ));
        }
        ui.add_space(18.0);
        ui.colored_label(
            MUTED,
            "작업 난이도가 달라 모델별 결과를 성능 순위로 해석하지 않습니다.",
        );
    }
}
fn card(ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .corner_radius(12)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            f(ui);
        });
}
fn metric(ui: &mut egui::Ui, label: &str, n: usize, color: Color32) {
    card(ui, |ui| {
        ui.label(RichText::new(label).size(13.0).color(MUTED));
        ui.label(RichText::new(n.to_string()).size(34.0).color(color));
    });
}
fn field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
}
fn combo(ui: &mut egui::Ui, id: &str, value: &mut String, options: &[&str]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(value, (*option).into(), *option);
            }
        });
}
fn model_card(ui: &mut egui::Ui, name: &str, model: &mut Model) {
    card(ui, |ui| {
        ui.heading(name);
        field(ui, "모델", &mut model.model);
        ui.label("추론 강도");
        combo(
            ui,
            name,
            &mut model.effort,
            &["low", "medium", "high", "xhigh", "max"],
        );
    });
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll();
        self.sidebar(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(PANEL).inner_margin(8))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.small(&self.status);
                    if self.job.is_some() {
                        ui.spinner();
                        ui.small("명령 실행 중");
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(28))
            .show(ctx, |ui| {
                self.header(ui);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Overview => self.overview(ui),
                        Page::Repositories => self.repositories(ui),
                        Page::Harness => self.harness(ui),
                        Page::Agents => self.agents(ui),
                        Page::Analysis => self.analysis(ui),
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(
            if self.job.is_some() || self.monitor.is_some() || self.repo_pending.is_some() {
                100
            } else {
                1000
            },
        ));
    }
}
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 820.0])
            .with_min_inner_size([900.0, 650.0])
            .with_title("Boorstar"),
        ..Default::default()
    };
    eframe::run_native(
        "Boorstar",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
