#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURES_DIR="${ROOT_DIR}/.test_fixtures"
COPYBARA_DIR="${FIXTURES_DIR}/copybara"
COPYBARA_REPO="https://github.com/google/copybara.git"

mkdir -p "${FIXTURES_DIR}"

if [ ! -d "${COPYBARA_DIR}/.git" ]; then
  echo "Cloning google/copybara into ${COPYBARA_DIR}..."
  git clone --depth 1 "${COPYBARA_REPO}" "${COPYBARA_DIR}"
else
  echo "google/copybara test repository already exists at ${COPYBARA_DIR}."
  echo "Updating to latest master..."
  git -C "${COPYBARA_DIR}" pull --rebase || true
fi

echo "Copybara test fixture is ready at ${COPYBARA_DIR}."
