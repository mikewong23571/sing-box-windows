#!/usr/bin/env bash
# SSOT: backend line coverage gate (full corpus = lib tests + tests/*).
# AC1 ignore set ONLY: test sources, generated/test_support, lib/main glue, OS system-proxy shell.
# See docs/backend-test-coverage-plan.md (D-COV / D-DENOM / D-LIB / D-PROXY-SHELL).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/src-tauri"

# Minimal policy ignore (AC1). Do NOT add process/proxy/lifecycle/etc. here.
readonly DEFAULT_IGNORE='(\.tests\.rs$|/tests/|test_support/|e2e_tests\.rs$|types/generated/|(^|/)src/lib\.rs$|(^|/)src/main\.rs$|utils/proxy_util\.rs$)'
COVERAGE_IGNORE_REGEX="${COVERAGE_IGNORE_REGEX:-$DEFAULT_IGNORE}"
FAIL_UNDER="${COVERAGE_FAIL_UNDER:-0}"

mkdir -p target/llvm-cov

if [[ "${FAIL_UNDER}" == "0" ]]; then
  echo "[coverage-backend] report-only: COVERAGE_FAIL_UNDER=0 (set env to enforce gate)"
fi

export PATH="${HOME}/.cargo/bin:${PATH}"
# TempWorkspace serializes env; serial tests reduce poison races under llvm-cov.
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"
# Short fake-kernel lifetime keeps hermetic tests fast (real sing-box is not spawned).
export FAKE_KERNEL_RUN_SECS="${FAKE_KERNEL_RUN_SECS:-5}"

echo "[coverage-backend] running tests once (fail-under=${FAIL_UNDER})..."
cargo llvm-cov test --features test-util \
  --fail-under-lines "${FAIL_UNDER}" \
  --ignore-filename-regex "${COVERAGE_IGNORE_REGEX}" \
  --no-report

echo "[coverage-backend] generating html..."
cargo llvm-cov report \
  --ignore-filename-regex "${COVERAGE_IGNORE_REGEX}" \
  --html --output-dir target/llvm-cov/html

echo "[coverage-backend] generating lcov + summary..."
cargo llvm-cov report \
  --ignore-filename-regex "${COVERAGE_IGNORE_REGEX}" \
  --lcov --output-path target/llvm-cov/lcov.info \
  --summary-only
