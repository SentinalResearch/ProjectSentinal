#!/usr/bin/env bash
# Owner-only, on the owner's own Mac: ship one Sentinal release end to end.
#   1. push main and tag vVERSION (skipped if the tag exists);
#   2. wait for the GitHub "release" workflow to build the unsigned draft;
#   3. download the draft and sign SHA256SUMS with your release key
#      (scripts/sign-release.sh checks every file against SHA256SUMS first);
#   4. upload SHA256SUMS.sig and publish it as the latest normal release.
# Every step is skipped when already done, so re-running after an
# interruption continues where it stopped. The key never leaves this machine:
# its path comes from --key or the gitignored .release.local (key=PATH).
set -euo pipefail

readonly SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
readonly REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

usage() {
  cat <<'EOF'
Usage: scripts/ship-release.sh VERSION [--key KEY] [--allowed-signers FILE] [--work-dir DIR]

  VERSION                 MAJOR.MINOR.PATCH, e.g. 0.2.2 (tag v0.2.2)
  --key KEY               release signing key; default: key=PATH in .release.local
  --allowed-signers FILE  pinned keys (default: packaging/release-keys/allowed_signers)
  --work-dir DIR          where the draft is downloaded (default: dist/release-vVERSION)
EOF
}

die() {
  echo "ship-release: $*" >&2
  exit 1
}

version="${1:-}"
[[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]] || { usage >&2; die "expected a version like 0.2.2, got '$version'"; }
shift
tag="v$version"
key=""
allowed="$REPO_ROOT/packaging/release-keys/allowed_signers"
work="$REPO_ROOT/dist/release-$tag"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --key) key="${2:-}"; shift 2 ;;
    --allowed-signers) allowed="${2:-}"; shift 2 ;;
    --work-dir) work="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown argument '$1'" ;;
  esac
done
if [[ -z "$key" && -f "$REPO_ROOT/.release.local" ]]; then
  key="$(sed -n 's/^key=//p' "$REPO_ROOT/.release.local" | head -n 1)"
  key="${key/#\~/$HOME}"
fi
[[ -n "$key" && -f "$key" ]] || die "no release key: pass --key PATH or put key=PATH in .release.local"
command -v gh >/dev/null 2>&1 || die "the GitHub CLI (gh) is required"

state="$(gh release view "$tag" --json isDraft,isPrerelease,assets \
  --jq '"draft=\(.isDraft)", "prerelease=\(.isPrerelease)", (.assets[].name)' 2>/dev/null || true)"
if grep -qx 'draft=false' <<<"$state" && ! grep -qx 'prerelease=true' <<<"$state" &&
  grep -qx 'SHA256SUMS.sig' <<<"$state"; then
  echo "$tag is already released and signed."
  exit 0
fi

# 1. Tag (only from a clean, pushed main).
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "Tag $tag exists; continuing."
else
  [[ -z "$(git status --porcelain --untracked-files=no)" ]] || die "commit or stash your changes before tagging $tag"
  [[ "$(git rev-parse --abbrev-ref HEAD)" == main ]] || die "tag releases from main"
  git push origin main
  git tag -a "$tag" -m "Sentinal $version"
  git push origin "$tag"
  echo "Tagged and pushed $tag."
fi

# 2. Wait for the build that the tag started.
run=""
for _ in $(seq 1 60); do
  run="$(gh run list --workflow release.yml --branch "$tag" --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)"
  [[ -n "$run" && "$run" != null ]] && break
  sleep 5
done
[[ -n "$run" && "$run" != null ]] || die "no release build started for $tag; check GitHub Actions"
echo "Waiting for build ${run}…"
gh run watch "$run" --exit-status >/dev/null || die "build $run failed; nothing was signed (see: gh run view $run --log-failed)"

# 3. Download the draft and sign it.
rm -rf -- "$work"
mkdir -p "$work"
gh release download "$tag" --dir "$work" --clobber
rm -f -- "$work/SHA256SUMS.sig"
echo "Signing SHA256SUMS (your key may ask for its passphrase or a touch)…"
bash "$SCRIPT_DIR/sign-release.sh" --dir "$work" --key "$key" --allowed-signers "$allowed" >/dev/null

# 4. Upload the signature and publish.
gh release upload "$tag" "$work/SHA256SUMS.sig" --clobber
gh release edit "$tag" --draft=false --prerelease=false --latest
echo "Sentinal $version released: https://github.com/SentinalResearch/ProjectSentinal/releases/tag/$tag"
echo "Controls see it under Settings → Machines → Control release (or: sentinal release update)."
