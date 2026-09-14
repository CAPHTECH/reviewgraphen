#!/bin/sh
set -eu

REPOSITORY_URL=${REVIEWGRAPHEN_RELEASE_BASE_URL:-https://github.com/CAPHTECH/reviewgraphen}
VERSION=latest
TARGETS=cli,codex,claude
BIN_DIR=${REVIEWGRAPHEN_BIN_DIR:-"$HOME/.local/bin"}
CODEX_ROOT=${CODEX_HOME:-"$HOME/.codex"}
CLAUDE_ROOT=${CLAUDE_CONFIG_DIR:-"$HOME/.claude"}
FORCE=0

usage() {
    cat <<'EOF'
Install ReviewGraphen from GitHub Releases.

Usage: sh install.sh [options]

Options:
  --version VERSION     Install VERSION (for example 0.1.0 or v0.1.0); default: latest
  --targets LIST        Comma-separated cli,codex,claude; default: all three
  --bin-dir DIR         CLI destination; default: $HOME/.local/bin
  --codex-home DIR      Codex home; default: $CODEX_HOME or $HOME/.codex
  --claude-home DIR     Claude config root; default: $CLAUDE_CONFIG_DIR or $HOME/.claude
  --force               Replace an unmanaged skill, preserving it as a backup
  -h, --help            Show this help

Environment:
  REVIEWGRAPHEN_RELEASE_BASE_URL  Override the repository URL (tests/mirrors)
  REVIEWGRAPHEN_BIN_DIR           Override the default CLI destination
EOF
}

die() {
    printf 'reviewgraphen installer: %s\n' "$*" >&2
    exit 1
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --version)
            [ "$#" -ge 2 ] || die "--version requires a value"
            VERSION=$2
            shift 2
            ;;
        --targets)
            [ "$#" -ge 2 ] || die "--targets requires a value"
            TARGETS=$2
            shift 2
            ;;
        --bin-dir)
            [ "$#" -ge 2 ] || die "--bin-dir requires a value"
            BIN_DIR=$2
            shift 2
            ;;
        --codex-home)
            [ "$#" -ge 2 ] || die "--codex-home requires a value"
            CODEX_ROOT=$2
            shift 2
            ;;
        --claude-home)
            [ "$#" -ge 2 ] || die "--claude-home requires a value"
            CLAUDE_ROOT=$2
            shift 2
            ;;
        --force)
            FORCE=1
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *) die "unknown option: $1" ;;
    esac
done

want_target() {
    case ",$TARGETS," in
        *",$1,"*) return 0 ;;
        *) return 1 ;;
    esac
}

case ",$TARGETS," in
    *,all,*) TARGETS=cli,codex,claude ;;
esac
old_ifs=$IFS
IFS=,
for requested in $TARGETS; do
    case "$requested" in
        cli|codex|claude) ;;
        *) die "unsupported target: $requested" ;;
    esac
done
IFS=$old_ifs

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

valid_semver_core() {
    candidate=$1
    case "$candidate" in
        ''|*[!0-9.]*|.*|*.|*..*) return 1 ;;
    esac
    old_semver_ifs=$IFS
    IFS=.
    set -- $candidate
    IFS=$old_semver_ifs
    [ "$#" -eq 3 ] || return 1
    for component in "$@"; do
        case "$component" in
            0|[1-9]|[1-9][0-9]*) ;;
            *) return 1 ;;
        esac
    done
}

case "$VERSION" in
    latest)
        RELEASE_URL="$REPOSITORY_URL/releases/latest/download"
        ;;
    v*)
        valid_semver_core "${VERSION#v}" || die "invalid version: $VERSION"
        RELEASE_URL="$REPOSITORY_URL/releases/download/$VERSION"
        VERSION=${VERSION#v}
        ;;
    *)
        valid_semver_core "$VERSION" || die "invalid version: $VERSION"
        RELEASE_URL="$REPOSITORY_URL/releases/download/v$VERSION"
        ;;
esac

tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/reviewgraphen-install.XXXXXX")
trap 'rm -rf "$tmp_dir"' EXIT HUP INT TERM

download() {
    curl --fail --location --silent --show-error --retry 3 \
        --proto '=https,file' --tlsv1.2 \
        "$RELEASE_URL/$1" -o "$tmp_dir/$1"
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        die "sha256sum or shasum is required"
    fi
}

download_verified() {
    asset_name=$1
    download "$asset_name"
    download "$asset_name.sha256"
    expected=$(awk 'NR == 1 { print $1 }' "$tmp_dir/$asset_name.sha256")
    case "$expected" in
        *[!0-9a-fA-F]*|'') die "invalid checksum file for $asset_name" ;;
    esac
    [ "${#expected}" -eq 64 ] || die "invalid checksum length for $asset_name"
    actual=$(sha256_file "$tmp_dir/$asset_name")
    [ "$actual" = "$expected" ] || die "checksum mismatch for $asset_name"
}

install_skill() {
    product=$1
    root=$2
    source_dir="$tmp_dir/skill/reviewgraphen"
    parent="$root/skills"
    destination="$parent/reviewgraphen"
    stage="$parent/.reviewgraphen.new.$$"

    mkdir -p "$parent"
    cp -R "$source_dir" "$stage"
    printf '%s\n' "$VERSION" > "$stage/.reviewgraphen-version"

    if [ -e "$destination" ]; then
        if [ ! -f "$destination/.reviewgraphen-version" ] && [ "$FORCE" -ne 1 ]; then
            rm -rf "$stage"
            die "$product skill already exists and is not installer-managed: $destination (use --force to preserve and replace it)"
        fi
        backup="$parent/.reviewgraphen.backup.$(date +%Y%m%d%H%M%S).$$"
        mv "$destination" "$backup"
        if ! mv "$stage" "$destination"; then
            mv "$backup" "$destination"
            die "could not install $product skill"
        fi
        printf 'Updated %s skill: %s (previous copy: %s)\n' "$product" "$destination" "$backup"
    else
        mv "$stage" "$destination"
        printf 'Installed %s skill: %s\n' "$product" "$destination"
    fi
}

if [ "$VERSION" = latest ]; then
    tag=$(curl --fail --location --silent --show-error --retry 3 \
        --proto '=https,file' --tlsv1.2 \
        "$REPOSITORY_URL/releases/latest/download/reviewgraphen-version")
    VERSION=${tag#v}
    valid_semver_core "$VERSION" || die "release returned an invalid version: $VERSION"
fi

if want_target cli; then
    case "$(uname -s):$(uname -m)" in
        Linux:x86_64|Linux:amd64) platform=x86_64-unknown-linux-gnu ;;
        Darwin:arm64|Darwin:aarch64) platform=aarch64-apple-darwin ;;
        *) die "no released binary for $(uname -s) $(uname -m)" ;;
    esac
    binary_archive="reviewgraphen-v$VERSION-$platform.tar.gz"
    download_verified "$binary_archive"
    mkdir -p "$tmp_dir/binary"
    tar -xzf "$tmp_dir/$binary_archive" -C "$tmp_dir/binary"
    binary="$tmp_dir/binary/reviewgraphen-v$VERSION-$platform/reviewgraphen"
    [ -f "$binary" ] || die "binary archive has an unexpected layout"
    [ "$("$binary" --version)" = "reviewgraphen $VERSION" ] || die "binary version check failed"
    mkdir -p "$BIN_DIR"
    staged_binary="$BIN_DIR/.reviewgraphen.new.$$"
    cp "$binary" "$staged_binary"
    chmod 0755 "$staged_binary"
    mv "$staged_binary" "$BIN_DIR/reviewgraphen"
    printf 'Installed CLI: %s\n' "$BIN_DIR/reviewgraphen"
fi

if want_target codex || want_target claude; then
    skill_archive="reviewgraphen-skill-v$VERSION.tar.gz"
    download_verified "$skill_archive"
    mkdir -p "$tmp_dir/skill"
    tar -xzf "$tmp_dir/$skill_archive" -C "$tmp_dir/skill"
    [ -f "$tmp_dir/skill/reviewgraphen/SKILL.md" ] || die "skill archive has an unexpected layout"
    want_target codex && install_skill Codex "$CODEX_ROOT"
    want_target claude && install_skill Claude "$CLAUDE_ROOT"
fi

printf 'ReviewGraphen %s installation complete.\n' "$VERSION"
