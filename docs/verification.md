# 첫 버전 검증 — 2026-11-17

대상: macOS Apple Silicon, Rust 1.98, egui/eframe 0.33.3, harness-core 1.30.0.

| 구분 | 수행 결과 |
| --- | --- |
| 자동 검증 | fmt·clippy 통과, Rust 인수 테스트 7개·경계 테스트 2개 통과 |
| 실제 하네스 계약 | 임시 레포·계측에서 doctor, 단계 조회, 모델 집계, off 위임·계측 연결 1개 통과 |
| 레포 연결 | 한글·공백 경로 등록, 다른 레포 추가·선택, 재시작 후 목록 유지 |
| 하네스 설정 | 임시 전역 중지 저장, 화면 적용 출처와 파일의 disabled=true 일치 |
| 모델 설정 | 임시 모델명 저장, 미저장 표시 해제, 재시작 후 값 유지 |
| 모니터링 | 샘플 검증 통과 2·실패 1·위임 시작 1을 정확히 표시 |
| 분석 | 실행 검증 3, 통과 2/3, 시작일을 다음 날로 변경 시 0, 토큰 기록 없음을 표시 |
| 위임 UI | off·전역 중지에서 fallback/disabled 수신, 외부 호출 없음, 레포를 바꿨다 돌아와도 응답 유지 |
| 패키지 | 로컬 ad-hoc 서명된 Boorstar.app 실행·서명 검증 |
| 미검증 | 실제 Jev 추천·유료 Claude/Codex 워커 실행, Windows/Linux, 배포용 공증 |

기본 Computer Use 도구는 native pipe 시작에 실패했다. 사용자 요청에 따라 Orca로 전환하고, 별도 동의를 받은 전면 입력으로 확인했다. 화면 이미지와 AX 관찰은 직접 대조했다. 모든 설정 저장과 위임 테스트는 BOORSTAR_HOME·HARNESS_HOME·HARNESS_METRICS_PATH가 격리된 임시 환경에서 수행했다. 최종 패키지에서는 테스트용 LSEnvironment를 제거했다.

스크린샷은 레포 밖에 보관한다.

- /tmp/boorstar-evidence/monitor.png
- /tmp/boorstar-evidence/models.png
- /tmp/boorstar-evidence/analysis.png
- /tmp/boorstar-qa-final-state.json

작업 파일의 T-08은 사람이 확인하는 항목이므로 체크하지 않았다. 위 내용은 에이전트가 수행·관찰한 근거다.

## 리뷰

별도 harness-reviewer 타입이 이 세션에서 제공되지 않아 현재 세션에서 직접 검토했다. 이 앱은 기존 하네스의 CLI를 호출하고 상태·계측을 표시하는 관리 화면이다.

일반 리뷰에서 수정한 항목:
- 레포 이름과 절대 경로가 섞인 실제 계측 형식을 모두 연결했다.
- 모델 변경 저장 시 외부에서 추가된 레포 목록을 보존하고 같은 필드의 충돌은 거부한다.
- 레포 전환 중 도착한 위임 응답을 해당 레포에 보관한다.
- 큰 stdin 입력에서도 명령 시간 제한이 동작하도록 입력을 임시 파일로 전달한다.
- Python 격리 실행에서도 하네스 내부 모듈 import가 되도록 bin 경로를 명시한다.

ponytail-review 제안(미반영): src/metrics.rs의 항상 None인 Summary.tokens와 대응 테스트 단언은 제거할 수 있다. 실제 토큰 표는 model_report에서 읽으므로 동작에 영향 없다. net: -2 lines possible.
