#!/usr/bin/env bash
set -euo pipefail

usage() {
    cat <<'EOF'
Archive GitHub repository traffic and metadata into timestamped JSON snapshots.

Usage:
  scripts/archive-github-traffic.sh [owner/repo]

Environment:
  GITHUB_REPO   Override the repository slug instead of inferring from git remote.
  OUTPUT_DIR    Override the snapshot root (default: metrics/github-traffic).

Notes:
  - Requires GitHub CLI (`gh`) authenticated with access to the target repo.
  - GitHub traffic endpoints expose only the last 14 days, so run this on a schedule.
EOF
}

require_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "ERROR: missing required command: $1" >&2
        exit 1
    fi
}

infer_repo_from_git() {
    local remote_url

    remote_url=$(git remote get-url origin 2>/dev/null || true)
    if [[ -z "${remote_url}" ]]; then
        echo "ERROR: could not infer repo from git remote 'origin'" >&2
        exit 1
    fi

    remote_url="${remote_url%.git}"
    remote_url="${remote_url#git@github.com:}"
    remote_url="${remote_url#https://github.com/}"
    remote_url="${remote_url#ssh://git@github.com/}"

    if [[ "${remote_url}" != */* ]]; then
        echo "ERROR: unsupported GitHub remote URL: ${remote_url}" >&2
        exit 1
    fi

    printf '%s\n' "${remote_url}"
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
    usage
    exit 0
fi

require_cmd gh
require_cmd git
require_cmd date
require_cmd mkdir

repo="${1:-${GITHUB_REPO:-}}"
if [[ -z "${repo}" ]]; then
    repo="$(infer_repo_from_git)"
fi

output_root="${OUTPUT_DIR:-metrics/github-traffic}"
timestamp="$(date -u +%Y-%m-%dT%H-%M-%SZ)"
snapshot_dir="${output_root}/${timestamp}"

mkdir -p "${snapshot_dir}"

gh auth status >/dev/null

echo "Archiving GitHub traffic for ${repo}"
echo "Snapshot directory: ${snapshot_dir}"

gh api "repos/${repo}" > "${snapshot_dir}/repo.json"
gh api "repos/${repo}/traffic/views" > "${snapshot_dir}/views.json"
gh api "repos/${repo}/traffic/clones" > "${snapshot_dir}/clones.json"
gh api "repos/${repo}/traffic/popular/referrers" > "${snapshot_dir}/referrers.json"
gh api "repos/${repo}/traffic/popular/paths" > "${snapshot_dir}/paths.json"

cat > "${snapshot_dir}/README.txt" <<EOF
repo=${repo}
captured_at_utc=${timestamp}
source=GitHub Traffic API
files=repo.json views.json clones.json referrers.json paths.json
EOF

echo "Saved:"
echo "  ${snapshot_dir}/repo.json"
echo "  ${snapshot_dir}/views.json"
echo "  ${snapshot_dir}/clones.json"
echo "  ${snapshot_dir}/referrers.json"
echo "  ${snapshot_dir}/paths.json"
