#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/reviewgraphen-installer-test.XXXXXX")
trap 'rm -rf "$work"' EXIT HUP INT TERM

version=0.1.0
release="$work/releases/download/v$version"
case "$(uname -s):$(uname -m)" in
    Linux:x86_64|Linux:amd64) platform=x86_64-unknown-linux-gnu ;;
    Darwin:arm64|Darwin:aarch64) platform=aarch64-apple-darwin ;;
    *) printf 'unsupported installer test host\n' >&2; exit 1 ;;
esac
mkdir -p "$release/binary/reviewgraphen-v$version-$platform" \
    "$release/skill/reviewgraphen"

cat > "$release/binary/reviewgraphen-v$version-$platform/reviewgraphen" <<'EOF'
#!/bin/sh
test "$1" = --version
printf 'reviewgraphen 0.1.0\n'
EOF
chmod +x "$release/binary/reviewgraphen-v$version-$platform/reviewgraphen"
printf '%s\n' '---' 'name: reviewgraphen' 'description: test fixture' '---' > "$release/skill/reviewgraphen/SKILL.md"

(cd "$release/binary" && tar -czf "$release/reviewgraphen-v$version-$platform.tar.gz" "reviewgraphen-v$version-$platform")
(cd "$release/skill" && tar -czf "$release/reviewgraphen-skill-v$version.tar.gz" reviewgraphen)

checksum() {
    file=$1
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$file" > "$file.sha256"
    else
        shasum -a 256 "$file" > "$file.sha256"
    fi
}
checksum "$release/reviewgraphen-v$version-$platform.tar.gz"
checksum "$release/reviewgraphen-skill-v$version.tar.gz"

latest="$work/releases/latest/download"
mkdir -p "$latest"
cp "$release/reviewgraphen-v$version-$platform.tar.gz" \
    "$release/reviewgraphen-v$version-$platform.tar.gz.sha256" \
    "$release/reviewgraphen-skill-v$version.tar.gz" \
    "$release/reviewgraphen-skill-v$version.tar.gz.sha256" "$latest/"
printf 'v%s\n' "$version" > "$latest/reviewgraphen-version"

HOME="$work/home" REVIEWGRAPHEN_RELEASE_BASE_URL="file://$work" \
    sh "$repo_root/install.sh" --version "$version"

test -x "$work/home/.local/bin/reviewgraphen"
test -f "$work/home/.codex/skills/reviewgraphen/SKILL.md"
test -f "$work/home/.claude/skills/reviewgraphen/SKILL.md"
test "$(cat "$work/home/.codex/skills/reviewgraphen/.reviewgraphen-version")" = "$version"

printf '%s\n' 'manually managed' > "$work/home/.codex/skills/reviewgraphen/SKILL.md"
rm -f "$work/home/.codex/skills/reviewgraphen/.reviewgraphen-version"
if HOME="$work/home" REVIEWGRAPHEN_RELEASE_BASE_URL="file://$work" \
    sh "$repo_root/install.sh" --version "$version" --targets codex >/dev/null 2>&1; then
    printf 'unmanaged skill replacement unexpectedly succeeded\n' >&2
    exit 1
fi
grep -q 'manually managed' "$work/home/.codex/skills/reviewgraphen/SKILL.md"

HOME="$work/home" REVIEWGRAPHEN_RELEASE_BASE_URL="file://$work" \
    sh "$repo_root/install.sh" --version "$version" --targets codex --force >/dev/null
grep -q 'description: test fixture' "$work/home/.codex/skills/reviewgraphen/SKILL.md"
set -- "$work/home/.codex/skills"/.reviewgraphen.backup.*
[ -d "$1" ]

HOME="$work/latest-home" REVIEWGRAPHEN_RELEASE_BASE_URL="file://$work" \
    sh "$repo_root/install.sh" --targets cli,claude >/dev/null
test -x "$work/latest-home/.local/bin/reviewgraphen"
test "$(cat "$work/latest-home/.claude/skills/reviewgraphen/.reviewgraphen-version")" = "$version"

printf '%064d  %s\n' 0 "reviewgraphen-skill-v$version.tar.gz" \
    > "$release/reviewgraphen-skill-v$version.tar.gz.sha256"
if HOME="$work/other-home" REVIEWGRAPHEN_RELEASE_BASE_URL="file://$work" \
    sh "$repo_root/install.sh" --version "$version" --targets claude >/dev/null 2>&1; then
    printf 'checksum mismatch unexpectedly succeeded\n' >&2
    exit 1
fi
test ! -e "$work/other-home/.claude/skills/reviewgraphen"

printf 'installer tests passed\n'
