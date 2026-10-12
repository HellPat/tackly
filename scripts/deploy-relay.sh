#!/usr/bin/env bash
# Deploys the relay to Cloudflare after the tests passed (CI, on main): asks
# Workers Builds to build and deploy main through the deploy hook, then waits
# until the relay answers /health from this commit. Fails when that does not
# happen, so a broken deploy fails the pipeline.
#
# Not caught: wrangler makes the Worker live before it builds the image, so if
# only the image fails, the previous container answers for the new commit.
#
# Needs DEPLOY_HOOK_URL, RELAY_URL and GITHUB_SHA.
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

curl -sSf -X POST "$DEPLOY_HOOK_URL" >/dev/null
echo "Deploy hook called; waiting for $RELAY_URL to answer from $GITHUB_SHA"

# A build takes about three minutes; the container then starts and restores
# the database from R2.
deadline=$((SECONDS + 10 * 60))
while :; do
  answer=$(curl -s -o /dev/null -D - -H 'cache-control: no-cache' "$RELAY_URL/health" || true)
  status=$(awk 'NR == 1 { print $2 }' <<<"$answer")
  commit=$(awk 'tolower($1) == "x-tackly-commit:" { print $2 }' <<<"$answer" | tr -d '\r')
  if [ "$commit" = "$GITHUB_SHA" ] && [ "$status" = 204 ]; then
    echo "The relay answers from $GITHUB_SHA"
    exit 0
  fi
  if [ "$SECONDS" -ge "$deadline" ]; then
    echo "::error::After 10 minutes $RELAY_URL/health answers ${status:-nothing} from ${commit:-unknown}, not 204 from $GITHUB_SHA. Workers Builds log: https://dash.cloudflare.com/?to=/:account/workers/services/view/tackly/production/builds"
    exit 1
  fi
  sleep 15
done
