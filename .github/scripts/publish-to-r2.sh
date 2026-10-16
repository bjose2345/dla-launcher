#!/usr/bin/env bash
# Uploads one finished release to this app's folder in the shared R2 bucket
# (dla-releases, one folder per DLA tool), downloads every file again to check
# it byte for byte, then makes it the current release (latest.json) and
# removes the release it replaces.
#
#   publish-to-r2.sh ASSETS_DIR MANIFEST TAG
#
# The environment provides AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY,
# R2_ENDPOINT, R2_BUCKET and R2_PREFIX.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: publish-to-r2.sh ASSETS_DIR MANIFEST TAG" >&2
  exit 2
fi
assets_dir=$1
manifest=$2
current_tag=$3

: "${AWS_ACCESS_KEY_ID:?Missing R2_RELEASES_ACCESS_KEY_ID}"
: "${AWS_SECRET_ACCESS_KEY:?Missing R2_RELEASES_SECRET_ACCESS_KEY}"
: "${R2_ENDPOINT:?Missing R2_RELEASES_ENDPOINT}"
: "${R2_BUCKET:?Missing R2_BUCKET}"
: "${R2_PREFIX:?Missing R2_PREFIX}"
R2_ENDPOINT="${R2_ENDPOINT%/}"
# The prefix ends up in a recursive delete, so it must be one plain folder name.
if [[ ! "$R2_PREFIX" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
  echo "Unsafe R2 prefix: $R2_PREFIX" >&2
  exit 1
fi

if [[ ! "$current_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "Unsafe release tag: $current_tag" >&2
  exit 1
fi

manifest_tag="$(node --print "JSON.parse(require('fs').readFileSync(process.argv[1], 'utf8')).tag" "$manifest")"
if [[ "$manifest_tag" != "$current_tag" ]]; then
  echo "Manifest tag $manifest_tag does not match $current_tag." >&2
  exit 1
fi

mkdir r2-verification
previous_tag=""
if aws s3 cp "s3://$R2_BUCKET/$R2_PREFIX/latest.json" r2-verification/previous-latest.json \
  --endpoint-url "$R2_ENDPOINT" --only-show-errors; then
  # The single-quoted program is JavaScript, so shell expansion is intentionally disabled.
  # shellcheck disable=SC2016
  previous_tag="$(node -e '
    const fs = require("fs");
    const tag = JSON.parse(fs.readFileSync(process.argv[1], "utf8")).tag;
    if (!/^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(tag)) {
      throw new Error(`Unsafe previous release tag: ${tag}`);
    }
    process.stdout.write(tag);
  ' r2-verification/previous-latest.json)"
fi

content_type() {
  case "$1" in
    *.tar.gz) echo "application/gzip" ;;
    *.zip) echo "application/zip" ;;
    *.rpm) echo "application/x-rpm" ;;
    *.deb) echo "application/vnd.debian.binary-package" ;;
    *.msi) echo "application/x-msi" ;;
    *.exe) echo "application/vnd.microsoft.portable-executable" ;;
    *.AppImage) echo "application/vnd.appimage" ;;
    *.txt) echo "text/plain; charset=utf-8" ;;
    *) echo "application/octet-stream" ;;
  esac
}

while IFS= read -r -d '' asset; do
  name="$(basename "$asset")"
  key="$R2_PREFIX/releases/$current_tag/$name"
  aws s3 cp "$asset" "s3://$R2_BUCKET/$key" \
    --endpoint-url "$R2_ENDPOINT" --only-show-errors \
    --cache-control "public, max-age=31536000, immutable" \
    --content-type "$(content_type "$name")" \
    --content-disposition "attachment; filename=\"$name\""
done < <(find "$assets_dir" -maxdepth 1 -type f -print0 | sort -z)

while IFS= read -r -d '' asset; do
  name="$(basename "$asset")"
  key="$R2_PREFIX/releases/$current_tag/$name"
  expected_size="$(stat --format='%s' "$asset")"
  remote_size="$(aws s3api head-object --bucket "$R2_BUCKET" --key "$key" \
    --endpoint-url "$R2_ENDPOINT" --query ContentLength --output text)"
  if [[ "$remote_size" != "$expected_size" ]]; then
    echo "R2 size mismatch for $name: expected $expected_size, received $remote_size." >&2
    exit 1
  fi
  aws s3 cp "s3://$R2_BUCKET/$key" "r2-verification/$name" \
    --endpoint-url "$R2_ENDPOINT" --only-show-errors
  expected_sha="$(sha256sum "$asset" | cut -d ' ' -f 1)"
  actual_sha="$(sha256sum "r2-verification/$name" | cut -d ' ' -f 1)"
  if [[ "$actual_sha" != "$expected_sha" ]]; then
    echo "R2 checksum mismatch for $name." >&2
    exit 1
  fi
done < <(find "$assets_dir" -maxdepth 1 -type f -print0 | sort -z)

aws s3 cp "$manifest" "s3://$R2_BUCKET/$R2_PREFIX/latest.json" \
  --endpoint-url "$R2_ENDPOINT" --only-show-errors \
  --cache-control "no-store" --content-type "application/json; charset=utf-8"
aws s3 cp "s3://$R2_BUCKET/$R2_PREFIX/latest.json" r2-verification/published-latest.json \
  --endpoint-url "$R2_ENDPOINT" --only-show-errors
cmp --silent "$manifest" r2-verification/published-latest.json

if [[ -n "$previous_tag" && "$previous_tag" != "$current_tag" ]]; then
  aws s3 rm "s3://$R2_BUCKET/$R2_PREFIX/releases/$previous_tag/" \
    --recursive --endpoint-url "$R2_ENDPOINT" --only-show-errors
fi

echo "Published $current_tag to $R2_PREFIX/ (previous: ${previous_tag:-none})."
