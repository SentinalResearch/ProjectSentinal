# shellcheck shell=bash
# macOS only: sign sentinal and sentinald with one local code-signing identity
# that stays the same across updates.
#
# Why: release binaries are ad-hoc signed, and macOS privacy (TCC) keys an
# ad-hoc binary by its hash. Every update is a "new app", so the Desktop /
# Documents / Downloads prompts (and any Full Disk Access grant) come back,
# and a Session started from the phone waits on a dialog nobody can click.
# Signed with a stable identity, one grant lasts across updates.

readonly SENTINAL_SIGNING_NAME='Sentinal Local Code Signing'

# Run "$@" for at most 30 s: a locked keychain may put up a dialog, and an
# unattended update must never wait on one.
sentinal_signing_bounded() {
  perl -e 'alarm 30; exec @ARGV or exit 127' "$@"
}

# The keychain to use: $SENTINAL_SIGNING_KEYCHAIN (tests) or the login keychain.
sentinal_signing_keychain() {
  echo "${SENTINAL_SIGNING_KEYCHAIN:-$HOME/Library/Keychains/login.keychain-db}"
}

# True when the identity is in the keychain (trusted or not; either signs).
sentinal_signing_identity_exists() {
  sentinal_signing_bounded security find-identity -p codesigning "$(sentinal_signing_keychain)" 2>/dev/null |
    grep -qF "\"$SENTINAL_SIGNING_NAME\""
}

# Create the identity: a self-signed code-signing certificate whose key
# codesign may use without a prompt. No trust setting is needed: macOS keys
# privacy grants on the identifier and the certificate itself.
sentinal_signing_create() {
  local keychain work password
  keychain="$(sentinal_signing_keychain)"
  sentinal_signing_identity_exists && return 0
  work="$(mktemp -d "${TMPDIR:-/tmp}/sentinal-signing.XXXXXX")"
  password="$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24 || true)"
  printf '%s\n' '[req]' 'distinguished_name = dn' 'x509_extensions = ext' 'prompt = no' \
    '[dn]' "CN = $SENTINAL_SIGNING_NAME" '[ext]' 'basicConstraints = critical, CA:false' \
    'keyUsage = critical, digitalSignature' 'extendedKeyUsage = critical, codeSigning' >"$work/cert.conf"
  if ! /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$work/cert.conf" \
    -keyout "$work/key.pem" -out "$work/cert.pem" >/dev/null 2>&1 ||
    ! /usr/bin/openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
      -name "$SENTINAL_SIGNING_NAME" -out "$work/identity.p12" -passout "pass:$password" >/dev/null 2>&1 ||
    ! sentinal_signing_bounded security import "$work/identity.p12" -k "$keychain" -P "$password" -T /usr/bin/codesign >/dev/null 2>&1; then
    rm -rf -- "$work"
    return 1
  fi
  rm -rf -- "$work"
  echo "Created the '$SENTINAL_SIGNING_NAME' identity in $keychain"
}

# Sign $1 under the fixed identifier dev.sentinal.$2, so its designated
# requirement (identifier + certificate) never changes between releases.
# Creates the identity on first use. A locked keychain (an install over SSH)
# leaves the program ad-hoc signed, as before.
sentinal_signing_sign() {
  local program="$1" name="$2"
  if ! sentinal_signing_create; then
    echo "Could not create a signing identity (is the keychain locked?); $name stays ad-hoc signed" >&2
    return 1
  fi
  if ! sentinal_signing_bounded codesign --force --sign "$SENTINAL_SIGNING_NAME" --keychain "$(sentinal_signing_keychain)" \
    --identifier "dev.sentinal.$name" "$program" >/dev/null 2>&1; then
    echo "Could not sign $name with '$SENTINAL_SIGNING_NAME'; it stays ad-hoc signed" >&2
    return 1
  fi
}
