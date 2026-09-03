#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_MANIFEST="apps/desktop/src-tauri/Cargo.toml"
VERSION_FILES=(
  "Cargo.toml"
  "Cargo.lock"
  "apps/desktop/package.json"
  "apps/desktop/package-lock.json"
  "$SOURCE_MANIFEST"
  "apps/desktop/src-tauri/Cargo.lock"
)

usage() {
  cat <<'EOF'
Usage:
  scripts/bump-version.sh MAJOR.MINOR.PATCH
  scripts/bump-version.sh --check [MAJOR.MINOR.PATCH]

The desktop Cargo manifest is the version source of truth. --check verifies
that all manifests and lockfiles match it (or the optional expected version)
without changing any files. Pre-release and build metadata are not accepted.
EOF
}

is_release_semver() {
  [[ "$1" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]
}

manifest_version() {
  sed -n 's/^version = "\([^"]*\)"$/\1/p' "$ROOT_DIR/$1" | head -n 1
}

package_version() {
  node -e 'console.log(JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8")).version)' "$ROOT_DIR/$1"
}

lock_version() {
  awk -v package="$2" '
    $0 == "[[package]]" { in_package = 0 }
    $0 == "name = \"" package "\"" { in_package = 1; next }
    in_package && /^version = "/ {
      value = $0
      sub(/^version = "/, "", value)
      sub(/"$/, "", value)
      print value
      exit
    }
  ' "$ROOT_DIR/$1"
}

check_value() {
  local label="$1"
  local actual="$2"
  local expected="$3"

  if [[ "$actual" != "$expected" ]]; then
    printf 'version mismatch: %s is %s (expected %s)\n' "$label" "${actual:-<missing>}" "$expected" >&2
    return 1
  fi
}

check_versions() {
  local expected="$1"
  local failed=0

  check_value "$SOURCE_MANIFEST" "$(manifest_version "$SOURCE_MANIFEST")" "$expected" || failed=1
  check_value "Cargo.toml" "$(manifest_version "Cargo.toml")" "$expected" || failed=1
  check_value "apps/desktop/package.json" "$(package_version "apps/desktop/package.json")" "$expected" || failed=1
  check_value "apps/desktop/package-lock.json" "$(package_version "apps/desktop/package-lock.json")" "$expected" || failed=1
  check_value "Cargo.lock (sessionsmith)" "$(lock_version "Cargo.lock" "sessionsmith")" "$expected" || failed=1
  check_value "apps/desktop/src-tauri/Cargo.lock (sessionsmith)" \
    "$(lock_version "apps/desktop/src-tauri/Cargo.lock" "sessionsmith")" "$expected" || failed=1
  check_value "apps/desktop/src-tauri/Cargo.lock (sessionsmith-desktop)" \
    "$(lock_version "apps/desktop/src-tauri/Cargo.lock" "sessionsmith-desktop")" "$expected" || failed=1

  if ! node -e '
    const config = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"));
    process.exit(Object.hasOwn(config, "version") && config.version === null ? 0 : 1);
  ' "$ROOT_DIR/apps/desktop/src-tauri/tauri.conf.json"; then
    printf 'version mismatch: apps/desktop/src-tauri/tauri.conf.json must set "version": null\n' >&2
    failed=1
  fi

  return "$failed"
}

mode="update"
if [[ "${1:-}" == "--check" ]]; then
  mode="check"
  shift
fi

if [[ "$#" -gt 1 ]] || { [[ "$mode" == "update" ]] && [[ "$#" -ne 1 ]]; }; then
  usage >&2
  exit 2
fi

source_version="$(manifest_version "$SOURCE_MANIFEST")"
version="${1:-$source_version}"
if ! is_release_semver "$version"; then
  printf 'invalid version: %s (expected MAJOR.MINOR.PATCH without pre-release or build metadata)\n' "$version" >&2
  exit 2
fi

if [[ "$mode" == "check" ]]; then
  check_versions "$version"
  printf 'Version check passed: %s\n' "$version"
  exit 0
fi

if ! is_release_semver "$source_version"; then
  printf 'invalid source version in %s: %s\n' "$SOURCE_MANIFEST" "${source_version:-<missing>}" >&2
  exit 1
fi
check_versions "$source_version"

backup_dir="$(mktemp -d)"
for file in "${VERSION_FILES[@]}"; do
  mkdir -p "$backup_dir/$(dirname "$file")"
  cp -p "$ROOT_DIR/$file" "$backup_dir/$file"
done

cleanup() {
  local exit_code=$?
  trap - EXIT
  if [[ "$exit_code" -ne 0 ]]; then
    for file in "${VERSION_FILES[@]}"; do
      cp -p "$backup_dir/$file" "$ROOT_DIR/$file"
    done
    printf 'Version update failed; restored all version files.\n' >&2
  fi
  rm -rf "$backup_dir"
  exit "$exit_code"
}
trap cleanup EXIT

VERSION="$version" perl -0pi -e '
  $count = s/(\[package\]\n(?:[^\n]*\n)*?version = ")[^"]+/$1$ENV{VERSION}/;
  die "expected one [package] version in $ARGV\n" unless $count == 1;
' "$ROOT_DIR/Cargo.toml" "$ROOT_DIR/$SOURCE_MANIFEST"

(cd "$ROOT_DIR/apps/desktop" && npm version "$version" --no-git-tag-version --allow-same-version --ignore-scripts)

cargo update --offline --manifest-path "$ROOT_DIR/Cargo.toml" \
  --package sessionsmith --precise "$version"
cargo update --offline --manifest-path "$ROOT_DIR/$SOURCE_MANIFEST" \
  --package sessionsmith --precise "$version"
cargo update --offline --manifest-path "$ROOT_DIR/$SOURCE_MANIFEST" \
  --package sessionsmith-desktop --precise "$version"

check_versions "$version"
printf 'Version synchronized to %s. Review and commit these changes before tagging v%s.\n' "$version" "$version"