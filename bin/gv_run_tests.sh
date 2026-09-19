#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
GV="${ROOT_DIR}/target/debug/gv"

# ── Colors ───────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m' # No Color

# ── Helpers ──────────────────────────────────────────────────────────
pass() { echo -e "${GREEN}✓ PASS${NC}: $1"; }
fail() { echo -e "${RED}✗ FAIL${NC}: $1"; FAILURES=$((FAILURES + 1)); }

FAILURES=0
RAN=0
VERIFY=false

# ── Test Definitions ─────────────────────────────────────────────────
#
# Each test has two functions:
#   test_<name>        — runs gv to generate the gradle view (default mode)
#   verify_<name>      — runs the gradle build to verify it works (--verify mode)

test_copybara_buildozer() {
  local desc="copybara: generate view for //java/com/google/copybara/buildozer"
  rm -rf "${ROOT_DIR}/e2e/gv_copybara"
  if "${GV}" e2e/copybara e2e/gv_copybara //java/com/google/copybara/buildozer; then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

verify_copybara_buildozer() {
  local desc="copybara: gradle build :java:com:google:copybara:buildozer:buildozer:build"
  if (cd "${ROOT_DIR}/e2e/gv_copybara" && ./gradlew :java:com:google:copybara:buildozer:buildozer:build); then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

test_android_jetpack_compose() {
  local desc="android: generate view for jetpack-compose app"
  rm -rf "${ROOT_DIR}/e2e/gv_android"
  if "${GV}" e2e/examples/android/jetpack-compose e2e/gv_android e2e/examples/android/jetpack-compose/app/src/main:all; then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

verify_android_jetpack_compose() {
  local desc="android: gradle assembleDebug"
  if (cd "${ROOT_DIR}/e2e/gv_android" && ./gradlew :android:jetpack-compose:app:src:main:app:assembleDebug); then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

# ── Test Registry ────────────────────────────────────────────────────
ALL_TESTS=(copybara_buildozer android_jetpack_compose)

# ── Usage ────────────────────────────────────────────────────────────
usage() {
  echo -e "${BOLD}Usage:${NC} $0 [--verify] [test_name ...]"
  echo ""
  echo "Run gv integration tests."
  echo ""
  echo -e "${BOLD}Modes:${NC}"
  echo "  (default)   Run gv to generate gradle views"
  echo "  --verify    Also run the gradle builds to verify generated views"
  echo ""
  echo -e "${BOLD}Available tests:${NC}"
  for t in "${ALL_TESTS[@]}"; do
    echo "  ${t}"
  done
  echo ""
  echo -e "${BOLD}Examples:${NC}"
  echo "  $0                                  # generate all views"
  echo "  $0 --verify                         # generate all views + gradle builds"
  echo "  $0 copybara_buildozer               # generate copybara view only"
  echo "  $0 --verify copybara_buildozer      # generate + verify copybara only"
  exit 0
}

# ── Parse args & select tests ────────────────────────────────────────
SELECTED_TESTS=()

for arg in "$@"; do
  case "$arg" in
    -h|--help) usage ;;
    --verify) VERIFY=true ;;
    *)
      # Validate test name
      local_found=false
      for t in "${ALL_TESTS[@]}"; do
        if [[ "$t" == "$arg" ]]; then
          local_found=true
          break
        fi
      done
      if [[ "$local_found" == false ]]; then
        echo -e "${RED}Unknown test:${NC} ${arg}"
        echo "Run '$0 --help' to list available tests."
        exit 1
      fi
      SELECTED_TESTS+=("$arg")
      ;;
  esac
done

if [ ${#SELECTED_TESTS[@]} -eq 0 ]; then
  SELECTED_TESTS=("${ALL_TESTS[@]}")
fi

# ── Setup ────────────────────────────────────────────────────────────
echo -e "${BOLD}Setting up test fixtures...${NC}"
"${SCRIPT_DIR}/gv_setup_tests.sh"
echo ""

echo -e "${BOLD}Building gv...${NC}"
cargo build --manifest-path "${ROOT_DIR}/Cargo.toml"
echo ""

# ── Run tests ────────────────────────────────────────────────────────
if [[ "$VERIFY" == true ]]; then
  echo -e "${BOLD}Running ${#SELECTED_TESTS[@]} test(s) with verify...${NC}"
else
  echo -e "${BOLD}Running ${#SELECTED_TESTS[@]} test(s)...${NC}"
fi
echo ""

for test_name in "${SELECTED_TESTS[@]}"; do
  RAN=$((RAN + 1))
  echo -e "${YELLOW}── ${test_name} ──${NC}"
  "test_${test_name}"
  if [[ "$VERIFY" == true ]]; then
    RAN=$((RAN + 1))
    "verify_${test_name}"
  fi
  echo ""
done

# ── Summary ──────────────────────────────────────────────────────────
echo -e "${BOLD}════════════════════════════════════════${NC}"
if [ ${FAILURES} -eq 0 ]; then
  echo -e "${GREEN}All ${RAN} test(s) passed.${NC}"
else
  echo -e "${RED}${FAILURES} of ${RAN} test(s) failed.${NC}"
  exit 1
fi
