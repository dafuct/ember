#!/usr/bin/env bash
# Create a stable, self-signed "Ember Dev" code-signing identity in the login keychain so the
# `tauri build` output keeps a constant code-signature designated requirement across rebuilds —
# which lets macOS TCC permissions (Microphone, Screen Recording) persist instead of resetting.
# Local developer-experience only: NOT a Developer ID, NOT notarized, NOT for distribution.
set -euo pipefail

IDENTITY="Ember Dev"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/.signing"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

# Idempotent: do nothing if the identity already exists.
if security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "✓ Code-signing identity \"$IDENTITY\" already present. Nothing to do."
  exit 0
fi

mkdir -p "$DIR"
CNF="$DIR/openssl-codesign.cnf"
# Use an OpenSSL config file (portable across the stock LibreSSL and Homebrew OpenSSL) to set the
# codeSigning extended-key-usage — `req -addext` is not reliable on LibreSSL.
cat > "$CNF" <<'EOF'
[req]
distinguished_name = dn
x509_extensions = v3_codesign
prompt = no
[dn]
CN = Ember Dev
[v3_codesign]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = codeSigning
EOF

echo "→ Generating self-signed code-signing certificate \"$IDENTITY\" (10 years)..."
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$DIR/ember-dev-key.pem" -out "$DIR/ember-dev-cert.pem" \
  -config "$CNF"
# Use legacy PBE algorithms so macOS security(1) can import the PKCS12.
# A local-only passphrase protects the bundle during import; the cert/key material
# is developer-machine-only and gitignored via scripts/.signing/.
P12_PASS="ember-dev-local"
openssl pkcs12 -export -inkey "$DIR/ember-dev-key.pem" -in "$DIR/ember-dev-cert.pem" \
  -out "$DIR/ember-dev.p12" -passout "pass:$P12_PASS" -name "$IDENTITY" \
  -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg SHA1

echo "→ Importing into the login keychain (grants /usr/bin/codesign access)..."
security import "$DIR/ember-dev.p12" -k "$KEYCHAIN" -P "$P12_PASS" -T /usr/bin/codesign

echo "→ Marking certificate as trusted for code signing..."
security add-trusted-cert -d -r trustRoot -k "$KEYCHAIN" "$DIR/ember-dev-cert.pem"

echo
if security find-identity -v -p codesigning | grep -q "$IDENTITY"; then
  echo "✓ \"$IDENTITY\" is now a valid code-signing identity."
  echo "  Next: npm run tauri build  — click \"Always Allow\" on the first codesign keychain prompt."
else
  echo "✗ Import finished but \"$IDENTITY\" is not listed by find-identity — see output above." >&2
  exit 1
fi
