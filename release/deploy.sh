#!/bin/bash
# Copy OTA binaries into the CDN asset git repo, commit, push, then register with OTA API.
#
# Usage:
#   ./deploy.sh <SW_VERSION>
#
# Env (optional):
#   ASSET_REPO_DIR   Root of homin-dev_asset checkout
#                    (default: ~/ws/homin-dev/homin-dev_asset)
#   ASSET_FW_DIR     Subdir under that repo for firmware bins
#                    (default: asset/rusty-hangulclock_fw)
#   SKIP_OTA_UPLOAD  If set to 1, skip upload_update.sh after CDN push

set -euo pipefail

SW_VERSION=${1:-}

if [ -z "$SW_VERSION" ]; then
  echo "Error: SW_VERSION argument is required"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSET_REPO_DIR="${ASSET_REPO_DIR:-${HOME}/ws/homin-dev/homin-dev_asset}"
ASSET_FW_DIR="${ASSET_FW_DIR:-asset/rusty-hangulclock_fw}"
DEST_DIR="${ASSET_REPO_DIR}/${ASSET_FW_DIR}"

if [ ! -d "${ASSET_REPO_DIR}/.git" ]; then
  echo "Error: ASSET_REPO_DIR is not a git repo: ${ASSET_REPO_DIR}"
  exit 1
fi

mkdir -p "${DEST_DIR}"

echo "Copying firmware binaries for version ${SW_VERSION} to ${DEST_DIR}..."
copied=0
# shellcheck disable=SC2086
for f in "${SCRIPT_DIR}"/*_${SW_VERSION}_*.bin; do
  [ -e "$f" ] || continue
  cp "$f" "${DEST_DIR}/"
  copied=1
done

if [ "$copied" -eq 0 ]; then
  echo "Error: no binaries matching *_${SW_VERSION}_*.bin under ${SCRIPT_DIR}"
  exit 1
fi

echo "Committing and pushing changes to asset repo..."
pushd "${DEST_DIR}" >/dev/null
shopt -s nullglob
staged=(*_"${SW_VERSION}"_*.bin)
if [ "${#staged[@]}" -eq 0 ]; then
  echo "Error: expected bins missing after copy in ${DEST_DIR}"
  exit 1
fi
git add -- "${staged[@]}"
if git diff --cached --quiet; then
  echo "Nothing new to commit for version ${SW_VERSION}"
else
  git commit -m "feat(rusty-hangulclock_fw): add sw ver. ${SW_VERSION}"
  git push
fi
popd >/dev/null

if [ "${SKIP_OTA_UPLOAD:-0}" = "1" ]; then
  echo "SKIP_OTA_UPLOAD=1 — skipping OTA API registration"
  exit 0
fi

for rev in 3 4; do
  file=$(find "${SCRIPT_DIR}" -maxdepth 1 -name "rusty-hangulclock_rev${rev}_${SW_VERSION}_*.bin" | head -n 1)

  if [ -n "$file" ]; then
    echo "Uploading update for Revision ${rev}: ${file}"
    "${SCRIPT_DIR}/upload_update.sh" -v "$SW_VERSION" -r "$rev" -f "$file"
    echo ""
  else
    echo "Warning: No binary found for Revision ${rev} and Version ${SW_VERSION}"
  fi
done
