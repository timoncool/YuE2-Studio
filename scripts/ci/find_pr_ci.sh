#!/usr/bin/env bash
# The PR workflow tests the merge result on both platforms. If it cannot be
# identified for the current source and base, the ordinary push tests run.
set -eu
repo=$GITHUB_REPOSITORY
endpoint="repos/$repo/actions/workflows/ci.yml/runs?event=pull_request&head_sha=$GITHUB_SHA&per_page=10"
runs=$(gh api "$endpoint") || exit 0
while read -r run number base; do
  [ -n "$run" ] || continue
  pull=$(gh api "repos/$repo/pulls/$number") || continue
  if jq -e --arg repo "$repo" --arg branch "$GITHUB_REF_NAME" --arg sha "$GITHUB_SHA" --arg base "$base" '
       .state == "open" and .mergeable != false and .head.repo.full_name == $repo
       and .head.ref == $branch and .head.sha == $sha and .base.sha == $base
     ' <<<"$pull" >/dev/null; then
    echo "run=$run" >>"$GITHUB_OUTPUT"
    echo "Feature push covered by PR #$number at https://github.com/$repo/actions/runs/$run" >>"$GITHUB_STEP_SUMMARY"
    exit 0
  fi
done < <(jq -r --arg sha "$GITHUB_SHA" '
  .workflow_runs[] | select(.head_sha == $sha and (.status != "completed" or .conclusion == "success"))
  | .id as $run | .pull_requests[] | select(.head.sha == $sha)
  | [$run,.number,.base.sha] | @tsv
' <<<"$runs")
