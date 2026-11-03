#!/usr/bin/env bash
# The installer puts its bin directory on PATH for new terminals, once, and
# uninstall takes it out again without touching the rest of the file.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/scripts/lib/shell-path.sh"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-shell-path-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT

fail() {
  echo "shell PATH test failed: $*" >&2
  exit 1
}

home="$test_root/home dir"
bin="$home/.local/bin"
mkdir -p "$home/.config/fish"
printf 'alias ll="ls -l"\n' >"$home/.bashrc"

# zsh login shell on Linux: .zshrc is created, .bashrc (exists) and fish get it.
sentinal_add_path_to_profiles "$bin" "$home" /bin/zsh linux >/dev/null
sentinal_add_path_to_profiles "$bin" "$home" /bin/zsh linux >/dev/null
for file in .zshrc .bashrc; do
  count="$(grep -c '^# >>> sentinal path >>>$' "$home/$file")"
  [[ "$count" == 1 ]] || fail "$file has $count PATH blocks after two installs"
done
grep -qF 'fish_add_path --global --move' "$home/.config/fish/conf.d/sentinal.fish" || fail 'fish was not set up'

found="$(env -i HOME="$home" PATH=/usr/bin:/bin "$BASH" -c 'source "$HOME/.bashrc"; command -v sentinal-probe || echo "$PATH"')"
[[ "$found" == "$bin:/usr/bin:/bin" ]] || fail "a new bash did not get PATH: $found"
again="$(env -i HOME="$home" PATH="$bin:/usr/bin" "$BASH" -c 'source "$HOME/.bashrc"; echo "$PATH"')"
[[ "$again" == "$bin:/usr/bin" ]] || fail "PATH was added twice: $again"

# macOS bash reads .bash_profile.
mac_home="$test_root/mac"
mkdir -p "$mac_home"
listed="$(sentinal_path_profiles "$mac_home" /bin/bash macos)"
[[ "$listed" == "$mac_home/.bash_profile" ]] || fail "macOS bash profile: $listed"

sentinal_remove_path_from_profiles "$home"
[[ "$(cat "$home/.bashrc")" == "$(printf 'alias ll="ls -l"\n')" ]] || {
  # The added block starts with a blank line; only that line may remain.
  [[ "$(grep -v '^$' "$home/.bashrc")" == 'alias ll="ls -l"' ]] || fail "uninstall changed .bashrc: $(cat "$home/.bashrc")"
}
! grep -q sentinal "$home/.zshrc" || fail 'uninstall left the zsh block'
[[ ! -e "$home/.config/fish/conf.d/sentinal.fish" ]] || fail 'uninstall left the fish file'

echo "shell PATH test passed"
