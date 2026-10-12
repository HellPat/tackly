#!/usr/bin/env bash
# Deploys the relay to Cloudflare after the tests passed (CI, on main): asks
# Workers Builds to build and deploy main through the deploy hook, waits for
# the build's check run on this commit, then checks that the relay answers.
# Fails when any of that does not happen, so a broken deploy fails the pipeline.
#
# Needs DEPLOY_HOOK_URL, RELAY_URL, GH_TOKEN (checks: read), GITHUB_REPOSITORY
# and GITHUB_SHA.
set -euo pipefail

: "${DEPLOY_HOOK_URL:?DEPLOY_HOOK_URL is not set (secret CLOUDFLARE_DEPLOY_HOOK)}"
: "${RELAY_URL:?RELAY_URL is not set}"

# The hook builds whatever main is now. If main has moved on, the pipeline of
# the newer commit deploys it.
head=$(git ls-remote origin refs/heads/main | cut -f1)
if [ "$head" != "$GITHUB_SHA" ]; then
  echo "main is at $head now, not $GITHUB_SHA; its own pipeline deploys it."
  exit 0
fi

since=$(date -u +%Y-%m-%dT%H:%M:%SZ)
curl -sSf -X POST "$DEPLOY_HOOK_URL" >/dev/null
echo "Deploy hook called; waiting for the Workers Builds check run on $GITHUB_SHA"

deadline=$((SECONDS + 40 * 60))
while :; do
  run=$(gh api "repos/$GITHUB_REPOSITORY/commits/$GITHUB_SHA/check-runs" --jq "
    [.check_runs[] | select(.app.slug == \"cloudflare-workers-and-pages\" and .started_at >= \"$since\")]
    | sort_by(.started_at) | last // empty
    | [.status, (.conclusion // \"-\"), .details_url] | @tsv")
  if [ -n "$run" ]; then
    IFS=$'\t' read -r status conclusion url <<<"$run"
    if [ "$status" = completed ]; then
      if [ "$conclusion" = success ]; then
        echo "Build and deploy succeeded: $url"
        break
      fi
      echo "::error::Cloudflare build $conclusion: $url"
      exit 1
    fi
  fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "::error::No finished Cloudflare build for $GITHUB_SHA after 40 minutes"
    exit 1
  fi
  sleep 20
done

# The container may still be starting (the first start takes minutes) or
# restoring the database from R2.
deadline=$((SECONDS + 10 * 60))
until [ "$(curl -s -o /dev/null -w '%{http_code}' "$RELAY_URL/health")" = 204 ]; do
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "::error::$RELAY_URL/health did not answer 204 within 10 minutes"
    exit 1
  fi
  sleep 15
done
echo "The relay answers at $RELAY_URL"
