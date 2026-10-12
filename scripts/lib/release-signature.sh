# Release signature helpers (sourced). Verification uses OpenSSH's
# `ssh-keygen -Y verify`; no other tool is trusted for signatures.

readonly SENTINAL_RELEASE_IDENTITY="sentinal-release"
readonly SENTINAL_RELEASE_NAMESPACE="sentinal-release"
readonly SENTINAL_MAX_SUMS_BYTES=65536
readonly SENTINAL_MAX_SIGNATURE_BYTES=8192

sentinal_signature_die() {
  echo "sentinal signature: $*" >&2
  exit 1
}

# True when FILE pins at least one sentinal-release key.
sentinal_release_keys_present() {
  local file="$1"
  [[ -f "$file" && ! -L "$file" ]] || return 1
  grep -Eq "^${SENTINAL_RELEASE_IDENTITY}[[:space:]]+namespaces=\"${SENTINAL_RELEASE_NAMESPACE}\"[[:space:]]+(ssh-ed25519|sk-ssh-ed25519@openssh.com)[[:space:]]+[A-Za-z0-9+/=]+" "$file"
}

sentinal_require_ssh_keygen_signatures() {
  command -v ssh-keygen >/dev/null 2>&1 ||
    sentinal_signature_die "ssh-keygen is required to verify release signatures (install OpenSSH 8.2 or newer)"
  # OpenSSH with `-Y verify` names the operation when arguments are missing;
  # older versions reject `-Y` and print only the usage text.
  local probe
  probe="$(ssh-keygen -Y verify 2>&1 || true)"
  [[ "$probe" == *"for verify"* ]] ||
    sentinal_signature_die "this ssh-keygen cannot verify signatures (needs OpenSSH 8.2 or newer)"
}

# Strict SHA256SUMS: only "<64 lowercase hex>  <name>" lines with safe names.
sentinal_validate_sums_file() {
  local sums="$1" bytes
  [[ -f "$sums" && ! -L "$sums" ]] || sentinal_signature_die "SHA256SUMS '$sums' is not a regular file"
  bytes="$(wc -c <"$sums" | tr -d '[:space:]')"
  ((bytes > 0 && bytes <= SENTINAL_MAX_SUMS_BYTES)) || sentinal_signature_die "SHA256SUMS is empty or larger than $SENTINAL_MAX_SUMS_BYTES bytes"
  if grep -qvE '^[0-9a-f]{64}  [A-Za-z0-9._-]+$' "$sums"; then
    sentinal_signature_die "SHA256SUMS is malformed; nothing was trusted"
  fi
  if [[ -n "$(awk '{print $2}' "$sums" | sort | uniq -d)" ]]; then
    sentinal_signature_die "SHA256SUMS names a file more than once"
  fi
}

# Verify SIG over SUMS with the keys pinned in ALLOWED. Fails closed.
sentinal_verify_release_sums() {
  local sums="$1" signature="$2" allowed="$3" bytes
  sentinal_release_keys_present "$allowed" ||
    sentinal_signature_die "no release signing keys are pinned in '$allowed'; nothing can be trusted"
  sentinal_validate_sums_file "$sums"
  [[ -f "$signature" && ! -L "$signature" ]] || sentinal_signature_die "release signature is missing; this release was not signed by the owner"
  bytes="$(wc -c <"$signature" | tr -d '[:space:]')"
  ((bytes > 0 && bytes <= SENTINAL_MAX_SIGNATURE_BYTES)) || sentinal_signature_die "release signature has an invalid size"
  sentinal_require_ssh_keygen_signatures
  ssh-keygen -Y verify -f "$allowed" -I "$SENTINAL_RELEASE_IDENTITY" -n "$SENTINAL_RELEASE_NAMESPACE" \
    -s "$signature" <"$sums" >/dev/null 2>&1 ||
    sentinal_signature_die "SHA256SUMS is not signed by a trusted Sentinal release key; nothing was installed"
}

# Print the digest SUMS lists for exactly NAME (empty when absent).
sentinal_sums_entry() {
  awk -v name="$2" '$2 == name { print $1 }' "$1"
}
