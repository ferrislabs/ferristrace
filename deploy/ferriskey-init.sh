#!/bin/sh
# Provisions the realm, confidential client and redirect uri that let the
# self-hosted stack sign somebody in without anybody opening the FerrisKey
# console. Runs once, as the `ferriskey-init` service, before `server`
# starts: compose waits for this script to exit zero
# (depends_on: service_completed_successfully) because `env_file` is read
# when a container is created, and the file this script writes has to
# exist by then.
#
# Idempotent: a realm FerrisKey already holds answers 200 on the GET below
# and is left alone; same for the client, found by its client_id instead of
# created again; the redirect uri is only posted when it is not already in
# the client's list, because FerrisKey does not reject a duplicate one on
# its own. A second `docker compose up` therefore finds everything already
# provisioned, re-reads the same client secret, and rewrites the same env
# file: no error, no duplicate, no new secret.
set -eu

API_URL="${FERRISKEY_API_URL:-http://ferriskey-api:3333}"
ADMIN_USERNAME="${ADMIN_USERNAME:-admin}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-admin}"
REALM="${FERRISKEY_REALM:-ferristrace}"
REALM_DISPLAY_NAME="${FERRISKEY_REALM_DISPLAY_NAME:-FerrisTrace}"
CLIENT_ID="${FERRISKEY_CLIENT_ID:-ferristrace}"
CLIENT_NAME="${FERRISKEY_CLIENT_NAME:-FerrisTrace}"
REDIRECT_URI="${FERRISKEY_REDIRECT_URI:-http://localhost:4318/auth/oidc/callback}"
ISSUER_BASE_URL="${FERRISKEY_ISSUER_BASE_URL:-http://ferriskey:3333}"
OUT_FILE="${OUT_FILE:-/out/ferriskey.env}"
MAX_WAIT_SECONDS="${FERRISKEY_INIT_MAX_WAIT_SECONDS:-60}"

# Writes the body to /tmp/body and the status code to /tmp/status, so one
# curl call tells us both without a second round trip.
http() {
  curl -sS -o /tmp/body -w "%{http_code}" "$@" > /tmp/status
}

json_field() {
  # First "<field>":"<value>" in the body, which is enough here: every
  # field this script reads is either at the top of its object or is the
  # one field a filtered query guarantees is unique. grep -o lists matches
  # left to right; a greedy `sed 's/.*"field"/.../'` would instead find the
  # last one in the line, which is the wrong field once an object nests
  # another "id" inside it, as `redirect_uris` does.
  grep -o "\"$1\":\"[^\"]*\"" "$2" | head -n 1 | sed -n "s/\"$1\":\"\([^\"]*\)\"/\1/p"
}

echo "ferriskey-init: waiting for $API_URL to answer"
waited=0
until http "$API_URL/realms/master/.well-known/openid-configuration"; status=$(cat /tmp/status); [ "$status" = "200" ]; do
  waited=$((waited + 2))
  if [ "$waited" -ge "$MAX_WAIT_SECONDS" ]; then
    echo "ferriskey-init: $API_URL never answered its discovery document (last status: $status)" >&2
    exit 1
  fi
  sleep 2
done

echo "ferriskey-init: fetching an admin token from the master realm"
# The password travels on stdin rather than argv, so it never shows up in
# this container's own process list.
printf '%s' "$ADMIN_PASSWORD" | http -X POST "$API_URL/realms/master/protocol/openid-connect/token" \
  -d grant_type=password -d client_id=admin-cli -d scope=openid \
  --data-urlencode "username=$ADMIN_USERNAME" --data-urlencode password@-
status=$(cat /tmp/status)
if [ "$status" != "200" ]; then
  echo "ferriskey-init: admin login failed with status $status: $(cat /tmp/body)" >&2
  exit 1
fi
TOKEN=$(json_field access_token /tmp/body)
if [ -z "$TOKEN" ]; then
  echo "ferriskey-init: admin login answered 200 with no access_token: $(cat /tmp/body)" >&2
  exit 1
fi
AUTH="authorization: Bearer $TOKEN"

echo "ferriskey-init: checking realm $REALM"
http "$API_URL/realms/$REALM" -H "$AUTH"
status=$(cat /tmp/status)
if [ "$status" = "200" ]; then
  echo "ferriskey-init: realm $REALM already exists, leaving it alone"
else
  echo "ferriskey-init: creating realm $REALM"
  http -X POST "$API_URL/realms" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"name\":\"$REALM\",\"display_name\":\"$REALM_DISPLAY_NAME\"}"
  status=$(cat /tmp/status)
  if [ "$status" != "201" ] && [ "$status" != "409" ]; then
    echo "ferriskey-init: realm creation failed with status $status: $(cat /tmp/body)" >&2
    exit 1
  fi
fi

echo "ferriskey-init: checking client $CLIENT_ID in realm $REALM"
http "$API_URL/realms/$REALM/clients?client_id=$CLIENT_ID" -H "$AUTH"
status=$(cat /tmp/status)
if [ "$status" != "200" ]; then
  echo "ferriskey-init: listing clients failed with status $status: $(cat /tmp/body)" >&2
  exit 1
fi
CLIENT_UUID=""
if grep -qF "\"client_id\":\"$CLIENT_ID\"" /tmp/body; then
  CLIENT_UUID=$(json_field id /tmp/body)
fi
if [ -n "$CLIENT_UUID" ]; then
  echo "ferriskey-init: client $CLIENT_ID already exists as $CLIENT_UUID, leaving it alone"
else
  echo "ferriskey-init: creating client $CLIENT_ID"
  http -X POST "$API_URL/realms/$REALM/clients" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"client_id\":\"$CLIENT_ID\",\"name\":\"$CLIENT_NAME\",\"client_type\":\"confidential\",\"protocol\":\"openid-connect\",\"enabled\":true,\"public_client\":false,\"require_pkce\":true}"
  status=$(cat /tmp/status)
  if [ "$status" != "201" ]; then
    echo "ferriskey-init: client creation failed with status $status: $(cat /tmp/body)" >&2
    exit 1
  fi
  CLIENT_UUID=$(json_field id /tmp/body)
  if [ -z "$CLIENT_UUID" ]; then
    echo "ferriskey-init: client creation answered 201 with no id: $(cat /tmp/body)" >&2
    exit 1
  fi
fi

echo "ferriskey-init: reading the client secret"
http "$API_URL/realms/$REALM/clients/$CLIENT_UUID/client-secret" -H "$AUTH"
status=$(cat /tmp/status)
if [ "$status" != "200" ]; then
  echo "ferriskey-init: reading the client secret failed with status $status: $(cat /tmp/body)" >&2
  exit 1
fi
CLIENT_SECRET=$(json_field client_secret /tmp/body)
if [ -z "$CLIENT_SECRET" ]; then
  echo "ferriskey-init: client-secret answered 200 with no client_secret: $(cat /tmp/body)" >&2
  exit 1
fi

echo "ferriskey-init: checking the redirect uri"
http "$API_URL/realms/$REALM/clients/$CLIENT_UUID" -H "$AUTH"
status=$(cat /tmp/status)
if [ "$status" != "200" ]; then
  echo "ferriskey-init: reading the client failed with status $status: $(cat /tmp/body)" >&2
  exit 1
fi
if grep -qF "\"value\":\"$REDIRECT_URI\"" /tmp/body; then
  echo "ferriskey-init: redirect uri already registered, leaving it alone"
else
  echo "ferriskey-init: registering the redirect uri"
  http -X POST "$API_URL/realms/$REALM/clients/$CLIENT_UUID/redirects" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"value\":\"$REDIRECT_URI\",\"enabled\":true}"
  status=$(cat /tmp/status)
  if [ "$status" != "201" ]; then
    echo "ferriskey-init: redirect uri registration failed with status $status: $(cat /tmp/body)" >&2
    exit 1
  fi
fi

echo "ferriskey-init: writing $OUT_FILE"
cat > "$OUT_FILE.tmp" <<EOF
FERRISTRACE_OIDC__ISSUER=$ISSUER_BASE_URL/realms/$REALM
FERRISTRACE_OIDC__CLIENT_ID=$CLIENT_ID
FERRISTRACE_OIDC__CLIENT_SECRET=$CLIENT_SECRET
FERRISTRACE_OIDC__REDIRECT_URI=$REDIRECT_URI
EOF
# Owner-only before it ever lands at its final name, so the client secret
# is never briefly world-readable on the host bind mount.
chmod 600 "$OUT_FILE.tmp"
mv "$OUT_FILE.tmp" "$OUT_FILE"
echo "ferriskey-init: done"
