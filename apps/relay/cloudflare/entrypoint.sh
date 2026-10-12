#!/bin/sh
# Starts the relay on a fresh disk: restores the database from R2 (if there is a
# backup), then runs the relay under Litestream, which replicates every change.
# Refuses to start without the R2 settings: a relay that keeps nothing would
# look healthy and lose every family's events on the next restart.
set -eu

: "${R2_ACCOUNT_ID:?R2_ACCOUNT_ID is not set}"
: "${R2_BUCKET:?R2_BUCKET is not set}"
: "${R2_ACCESS_KEY_ID:?R2_ACCESS_KEY_ID is not set}"
: "${R2_SECRET_ACCESS_KEY:?R2_SECRET_ACCESS_KEY is not set}"

litestream restore -if-db-not-exists -if-replica-exists /data/tackly-relay.db
exec litestream replicate -exec tackly-relay
