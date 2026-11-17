.PHONY: run verify-quick package
run:
	cargo run --locked

verify-quick:
	cargo fmt --check
	cargo clippy --locked --all-targets -- -D warnings
	cargo test --locked --no-default-features --test acceptance handles_large_stdin_timeout_and_unknown_settings -- --exact
	cargo test --locked --no-default-features --test acceptance changing_models_preserves_external_repo_edits -- --exact
	PYTHONDONTWRITEBYTECODE=1 python3 tests/test_acceptance.py

package:
	sh tools/package-macos.sh
