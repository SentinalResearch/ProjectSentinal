# shellcheck shell=bash
# Put the Sentinal bin directory on PATH for every new terminal: a marked
# block in the shell start-up files, added once and removed on uninstall.

readonly SENTINAL_PATH_BEGIN='# >>> sentinal path >>>'
readonly SENTINAL_PATH_END='# <<< sentinal path <<<'

# The start-up files to edit under $1 (home), one per line: files that exist,
# plus the file of the login shell ($2, e.g. /bin/zsh) even when absent.
sentinal_path_profiles() {
  local home="$1" login_shell="${2:-}" platform="${3:-}"
  local zsh_rc="$home/.zshrc" bash_rc="$home/.bashrc"
  [[ "$platform" != macos ]] || bash_rc="$home/.bash_profile"
  if [[ -f "$zsh_rc" || "$login_shell" == */zsh ]]; then echo "$zsh_rc"; fi
  if [[ -f "$bash_rc" || "$login_shell" == */bash ]]; then echo "$bash_rc"; fi
  if [[ "$platform" == macos && -f "$home/.bashrc" ]]; then echo "$home/.bashrc"; fi
  if [[ -f "$home/.profile" ]]; then echo "$home/.profile"; fi
  if [[ -d "$home/.config/fish" || "$login_shell" == */fish ]]; then
    echo "$home/.config/fish/conf.d/sentinal.fish"
  fi
}

sentinal_strip_path_block() {
  local file="$1" kept
  [[ -f "$file" && ! -L "$file" ]] || return 0
  grep -qF "$SENTINAL_PATH_BEGIN" "$file" || return 0
  kept="$(mktemp "${file}.sentinal.XXXXXX")"
  awk -v begin="$SENTINAL_PATH_BEGIN" -v end="$SENTINAL_PATH_END" '
    $0 == begin { skipping = 1; next }
    $0 == end { skipping = 0; next }
    !skipping { print }
  ' "$file" >"$kept"
  cat "$kept" >"$file"
  rm -f -- "$kept"
}

# Add (or refresh) the PATH block for $1 (bin dir) in each start-up file.
sentinal_add_path_to_profiles() {
  local bin_dir="$1" home="$2" login_shell="${3:-}" platform="${4:-}" file quoted
  quoted="${bin_dir//\"/\\\"}"
  while IFS= read -r file; do
    [[ -n "$file" ]] || continue
    if [[ -L "$file" ]]; then
      echo "Skipped $file (a symlink); add $bin_dir to PATH there yourself"
      continue
    fi
    if [[ "$file" == *.fish ]]; then
      mkdir -p -- "$(dirname -- "$file")"
      printf '%s\nfish_add_path --global --move "%s"\n%s\n' \
        "$SENTINAL_PATH_BEGIN" "$quoted" "$SENTINAL_PATH_END" >"$file"
    else
      sentinal_strip_path_block "$file"
      printf '\n%s\ncase ":$PATH:" in *":%s:"*) ;; *) export PATH="%s:$PATH" ;; esac\n%s\n' \
        "$SENTINAL_PATH_BEGIN" "$quoted" "$quoted" "$SENTINAL_PATH_END" >>"$file"
    fi
    echo "Added $bin_dir to PATH in $file"
  done < <(sentinal_path_profiles "$home" "$login_shell" "$platform")
}

# Remove the PATH block from every start-up file that has one.
sentinal_remove_path_from_profiles() {
  local home="$1" file
  for file in "$home/.zshrc" "$home/.bashrc" "$home/.bash_profile" "$home/.profile"; do
    sentinal_strip_path_block "$file"
  done
  file="$home/.config/fish/conf.d/sentinal.fish"
  if [[ -f "$file" ]] && grep -qF "$SENTINAL_PATH_BEGIN" "$file"; then rm -f -- "$file"; fi
}
