# Boorstar 작업 지침

개인 프로젝트. Jira 없이 진행한다.

- 화면과 앱 로직은 Rust/egui로 작성한다. 기존 boo-harness의 Python CLI는 연동 대상으로 유지한다.
- boo-harness 소스·설치본을 수정하지 않는다. 모델 재정의는 앱에서 시작한 위임 프로세스에만 적용한다.
- 설정 저장 시 알 수 없는 필드를 보존한다. 손상된 설정을 기본값으로 덮어쓰지 않는다.
- 명령 실행·계측 읽기는 UI 스레드 밖에서 수행한다. 위임 결과가 불명확하면 자동 재시도하지 않는다.
- 테스트는 임시 Git 레포·설정·계측을 사용한다. 실제 인증이나 유료 모델 호출은 하지 않는다.
- 검증: make verify-quick. 테스트 결과는 JUNIT_PATH(기본 /tmp/boorstar-junit.xml).
- 화면 검증은 BOORSTAR_HOME·HARNESS_HOME·HARNESS_METRICS_PATH를 임시 경로로 격리한다.
