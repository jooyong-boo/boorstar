"""Rust 통합 테스트를 기존 Python spec-trace/JUnit 계약에 연결한다."""
import os
from pathlib import Path
import subprocess
import sys
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]

class Acceptance(unittest.TestCase):
    def rust(self, name):
        p = subprocess.run(['cargo', 'test', '--locked', '--no-default-features', '--test', 'acceptance', name, '--', '--exact'], cwd=ROOT, text=True, capture_output=True)
        self.assertEqual(p.returncode, 0, p.stdout + p.stderr)
        self.assertIn('1 passed', p.stdout)

    def test_repositories(self):
        """[task:rust-desktop:T-01] 저장·재시작·중복·제거."""
        self.rust('t01')
    def test_doctor(self):
        """[task:rust-desktop:T-02] 실제 프로세스 도입 점검."""
        self.rust('t02')
    def test_dispatch_settings(self):
        """[task:rust-desktop:T-03] 전역·레포 우선순위와 중지."""
        self.rust('t03')
    def test_models(self):
        """[task:rust-desktop:T-04] 저장된 모델이 다음 앱 위임에 전달된다."""
        self.rust('t04')
    def test_monitor(self):
        """[task:rust-desktop:T-05] 빈 상태·루프·계측 갱신."""
        self.rust('t05')
    def test_analysis(self):
        """[task:rust-desktop:T-06] 기간·레포 필터와 실제 검증 집계."""
        self.rust('t06')
    def test_errors(self):
        """[task:rust-desktop:T-07] 손상 설정 보존·명령 실패."""
        self.rust('t07')

if __name__ == '__main__':
    suite = unittest.defaultTestLoader.loadTestsFromTestCase(Acceptance)
    names = [t.id() for t in suite]
    result = unittest.TextTestRunner(verbosity=2).run(suite)
    failures = {t.id(): text for t, text in result.failures + result.errors}
    root = ET.Element('testsuite', tests=str(len(names)), failures=str(len(failures)))
    for name in names:
        case = ET.SubElement(root, 'testcase', classname='test_acceptance.Acceptance', name=name.rsplit('.', 1)[-1])
        if name in failures:
            ET.SubElement(case, 'failure').text = failures[name]
    output = Path(os.environ.get('JUNIT_PATH', '/tmp/boorstar-junit.xml'))
    output.parent.mkdir(parents=True, exist_ok=True)
    ET.ElementTree(root).write(output, encoding='utf-8', xml_declaration=True)
    sys.exit(not result.wasSuccessful())
