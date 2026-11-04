#!/usr/bin/env bash
# The GitHub side of the PR previews, for preview-teardown.yml:
#
#   .github/scripts/preview-deployments.sh open-previews
#     Prints the open PRs into 2027 that should have a preview as a JSON list,
#     e.g. [12,14]: from a branch of this repository, not a draft or labelled
#     `preview`. Also dependabot's: the list only keeps previews, and one of
#     its PRs has a preview only if a person triggered one.
#
#   .github/scripts/preview-deployments.sh deactivate --pr <n>
#   .github/scripts/preview-deployments.sh deactivate --keep '[<n>,...]'
#     Marks the deployments of the environment preview-pr-<n>, or of every
#     preview-pr-<n> but those of the list, inactive, so the PR no longer
#     links the preview. Deployments that are inactive already stay as they
#     are, so a nightly run only changes what a teardown missed. --keep also
#     keeps the PRs that open-previews lists by now: one that got a preview
#     while the playbook ran keeps its link.
#
# Needs gh with GH_TOKEN (pull-requests: read, deployments: write), jq and
# GITHUB_REPOSITORY; GITHUB_SERVER_URL and GITHUB_RUN_ID link the run as the
# status' log.
set -euo pipefail

BASE=2027

usage() {
  cat >&2 <<USAGE
Usage: $0 open-previews
       $0 deactivate --pr <n>
       $0 deactivate --keep '[<n>,...]'
USAGE
  exit 2
}

open_previews() {
  gh pr list --repo "$GITHUB_REPOSITORY" --base "$BASE" --state open --limit 1000 \
    --json number,isDraft,isCrossRepository,labels \
    | jq -c '[.[] | select((.isCrossRepository | not)
                            and ((.isDraft | not) or any(.labels[]; .name == "preview")))
              | .number] | sort'
}

# One line per deployment: <id> <PR> <state>, for the environments preview-pr-<n>.
preview_deployments() {
  # shellcheck disable=SC2016 # GraphQL variables, not shell ones.
  gh api graphql --paginate -f owner="${GITHUB_REPOSITORY%%/*}" -f name="${GITHUB_REPOSITORY#*/}" -f query='
    query($owner: String!, $name: String!, $endCursor: String) {
      repository(owner: $owner, name: $name) {
        deployments(first: 100, after: $endCursor) {
          nodes { databaseId environment state }
          pageInfo { hasNextPage endCursor }
        }
      }
    }' \
    | jq -r '.data.repository.deployments.nodes[]
             | (.environment | capture("^preview-pr-(?<pr>[1-9][0-9]*)$")?) as $env
             | select($env) | "\(.databaseId) \($env.pr) \(.state)"'
}

# Marks the deployments of the PRs that are (listed) or aren't (unlisted) in
# the list, e.g. `deactivate listed " 12 "` or `deactivate unlisted " 12 14 "`.
deactivate() {
  local which=$1 list=$2 deployments id pr state failed=0
  deployments=$(preview_deployments)
  while read -r id pr state; do
    [ -n "$id" ] || continue
    case $state in INACTIVE|DESTROYED) continue ;; esac
    if [[ $list == *" $pr "* ]]; then [ "$which" = listed ] || continue
    else [ "$which" = unlisted ] || continue; fi
    echo "Marking deployment $id of preview-pr-$pr ($state) inactive."
    gh api --method POST "repos/$GITHUB_REPOSITORY/deployments/$id/statuses" -f state=inactive \
      -f "log_url=${GITHUB_SERVER_URL:-https://github.com}/$GITHUB_REPOSITORY/actions/runs/${GITHUB_RUN_ID:-}" \
      --silent || failed=1
  done <<< "$deployments"
  return "$failed"
}

case "${1:-} ${2:-}" in
  "open-previews ")
    [ "$#" = 1 ] || usage
    open_previews ;;
  "deactivate --pr")
    [ "$#" = 3 ] && [[ $3 =~ ^[1-9][0-9]*$ ]] || usage
    deactivate listed " $3 " ;;
  "deactivate --keep")
    [ "$#" = 3 ] && jq -en --argjson keep "$3" \
      '$keep | type == "array" and all(.[]; type == "number" and . >= 1 and . == floor)' \
      > /dev/null 2>&1 || usage
    now=$(open_previews)
    deactivate unlisted " $(jq -rn --argjson keep "$3" --argjson now "$now" '$keep + $now | unique | join(" ")') " ;;
  *)
    usage ;;
esac
