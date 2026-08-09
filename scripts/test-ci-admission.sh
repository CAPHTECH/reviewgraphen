#!/usr/bin/env bash
# Regression checks for the external Cargo-admission branch in ci.sh.
set -euo pipefail

readonly TEST_ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"

# shellcheck source=ci.sh
source "$TEST_ROOT_DIR/scripts/ci.sh"

test_root="$(mktemp -d)"
trap 'rm -rf -- "$test_root"' EXIT
mkdir -p "$test_root/bin"
printf '%s\n' '#!/usr/bin/env bash' 'printf "%s\\n" "$TEST_CI_CARGO"' > "$test_root/bin/mise"
printf '%s\n' '#!/usr/bin/env bash' 'exit 0' > "$test_root/cargo"
chmod 755 "$test_root/bin/mise" "$test_root/cargo"

# An explicitly host-admitted value is retained verbatim after validation.
(
  export REVIEWGRAPHEN_TRUSTED_CARGO="$test_root/cargo"
  admit_ingest_test_cargo
  test "$REVIEWGRAPHEN_TRUSTED_CARGO" = "$test_root/cargo"
)

# The local fallback gets its value only from the external mise harness.
(
  export PATH="$test_root/bin:$PATH"
  export TEST_CI_CARGO="$test_root/cargo"
  unset REVIEWGRAPHEN_TRUSTED_CARGO
  admit_ingest_test_cargo
  test "$REVIEWGRAPHEN_TRUSTED_CARGO" = "$test_root/cargo"
)

# A value unsafe for GitHub's single-line environment-file format is rejected
# before it can be exported or used as an executable path.
if (
  export REVIEWGRAPHEN_TRUSTED_CARGO="$test_root/cargo"$'\n'"injected=value"
  admit_ingest_test_cargo
); then
  printf '%s\n' 'ci admission accepted a newline-bearing path' >&2
  exit 1
fi
