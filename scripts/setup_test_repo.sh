#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
FIXTURES_DIR="${ROOT_DIR}/e2e"

mkdir -p "${FIXTURES_DIR}"

setup_fixture() {
  local name="$1"
  local repo_url="$2"
  local target_dir="${FIXTURES_DIR}/${name}"

  if [ ! -d "${target_dir}/.git" ]; then
    echo "Cloning ${name} (${repo_url}) into ${target_dir}..."
    git clone --depth 1 "${repo_url}" "${target_dir}"
  else
    echo "${name} test repository already exists at ${target_dir}."
    echo "Updating ${name} to latest..."
    git -C "${target_dir}" pull --rebase || true
  fi

  echo "Fixture '${name}' is ready at ${target_dir}."
  echo ""
}

# Define and setup fixtures
setup_fixture "copybara" "https://github.com/google/copybara.git"
setup_fixture "examples" "https://github.com/bazelbuild/examples.git"

echo "All test fixtures are ready in ${FIXTURES_DIR}."
