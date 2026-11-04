#!/bin/sh
# Provisions the realm, the public web client, its redirect uri and its web
# origin that let the self-hosted stack sign somebody in without anybody
# opening the FerrisKey console. The client is public (code flow with PKCE,
# no secret): the web signs in with it and the server only verifies the
# tokens it issues, so no client secret exists for the web client. Runs once, as the `ferriskey-init` service, before `server`
# starts: compose waits for this script to exit zero
# (depends_on: service_completed_successfully) because `env_file` is read
# when a container is created, and the file this script writes has to
# exist by then.
#
# Idempotent: a realm FerrisKey already holds answers 200 on the GET below
# and is left alone; same for the client, found by its client_id instead of
# created again; the redirect uri and the web origin are only posted when
# they are not already in the client's lists, because FerrisKey does not
# reject a duplicate redirect uri on its own and refuses a duplicate origin.
# A second `docker compose up` therefore finds everything already
# provisioned and rewrites the same env file: no error, no duplicate.
# A client that already exists is never modified, so one created confidential
# by an earlier version stays confidential: see deploy/README.md.
set -eu

API_URL="${FERRISKEY_API_URL:-http://ferriskey-api:3333}"
ADMIN_USERNAME="${ADMIN_USERNAME:-admin}"
ADMIN_PASSWORD="${ADMIN_PASSWORD:-admin}"
REALM="${FERRISKEY_REALM:-ferristrace}"
REALM_DISPLAY_NAME="${FERRISKEY_REALM_DISPLAY_NAME:-FerrisTrace}"
CLIENT_ID="${FERRISKEY_CLIENT_ID:-ferristrace}"
CLIENT_NAME="${FERRISKEY_CLIENT_NAME:-FerrisTrace}"
APP_ORIGIN="${FERRISKEY_APP_ORIGIN:-http://localhost:4318}"
EXTRA_APP_ORIGINS="${FERRISKEY_EXTRA_APP_ORIGINS:-}"
ISSUER_BASE_URL="${FERRISKEY_ISSUER_BASE_URL:-http://ferriskey:3333}"
OUT_FILE="${OUT_FILE:-/out/ferriskey.env}"
MAX_WAIT_SECONDS="${FERRISKEY_INIT_MAX_WAIT_SECONDS:-60}"
MCP_GATEWAY_CLIENT_ID="${FERRISKEY_MCP_GATEWAY_CLIENT_ID:-ferristrace-mcp-gateway}"
MCP_PEP_CLIENT_ID="${FERRISKEY_MCP_PEP_CLIENT_ID:-ferristrace-mcp-pep}"
MCP_PEP_ROLE="${FERRISKEY_MCP_PEP_ROLE:-ferristrace-mcp-pep}"
AGENT_AUDIENCE="${FERRISKEY_AGENT_AUDIENCE:-ferristrace-agents}"
MCP_PUBLIC_URL="${FERRISKEY_MCP_PUBLIC_URL:-http://localhost:8080}"
MCP_ORGS="${FERRISKEY_MCP_ORGS:-}"
MCP_OUT_FILE="${MCP_OUT_FILE:-/out/mcp-gateway.env}"
READER="${READER:-65532:65532}"
MCP_POLICY_DIR="${MCP_POLICY_DIR:-/mcp/policies}"
MCP_SEED_POLICY="${MCP_SEED_POLICY:-seed-members-read.cedar}"
MCP_SEED_POLICY_LEGACY="${MCP_SEED_POLICY_LEGACY:-seed-members-read-by-name.cedar}"
MCP_SCHEMA_FILE="${MCP_SCHEMA_FILE:-/mcp/ferristrace.cedarschema}"
MCP_SCHEMA_NAMESPACE="FerrisTrace"
MCP_SEED_NAME="${MCP_SEED_NAME:-members-read-tools}"
MCP_MANAGED_BY="ferristrace-init"

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

client_uuid_in_body() {
  grep -o "\"id\":\"[^\"]*\",\"enabled\":[a-z]*,\"client_id\":\"$1\"" /tmp/body | head -n 1 | sed -n 's/^"id":"\([^"]*\)".*/\1/p'
}

expect() {
  what="$1"
  shift
  status=$(cat /tmp/status)
  for accepted in "$@"; do
    if [ "$status" = "$accepted" ]; then
      return 0
    fi
  done
  echo "ferriskey-init: $what failed with status $status: $(cat /tmp/body)" >&2
  exit 1
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
CLIENT_UUID=$(client_uuid_in_body "$CLIENT_ID")
if [ -n "$CLIENT_UUID" ]; then
  echo "ferriskey-init: client $CLIENT_ID already exists as $CLIENT_UUID, leaving it alone"
else
  echo "ferriskey-init: creating client $CLIENT_ID"
  http -X POST "$API_URL/realms/$REALM/clients" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"client_id\":\"$CLIENT_ID\",\"name\":\"$CLIENT_NAME\",\"client_type\":\"public\",\"protocol\":\"openid-connect\",\"enabled\":true,\"public_client\":true,\"require_pkce\":true}"
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

ensure_app_origin() {
  origin=${1%/}
  redirect_uri="$origin/"
  http "$API_URL/realms/$REALM/clients/$CLIENT_UUID" -H "$AUTH"
  expect "reading client $CLIENT_ID" 200
  if grep -qF "\"value\":\"$redirect_uri\"" /tmp/body; then
    echo "ferriskey-init: redirect uri $redirect_uri already registered, leaving it alone"
  else
    echo "ferriskey-init: registering the redirect uri $redirect_uri"
    http -X POST "$API_URL/realms/$REALM/clients/$CLIENT_UUID/redirects" -H "$AUTH" -H 'content-type: application/json' \
      -d "{\"value\":\"$redirect_uri\",\"enabled\":true}"
    expect "registering the redirect uri $redirect_uri" 201
  fi
  http "$API_URL/realms/$REALM/clients/$CLIENT_UUID/web-origins" -H "$AUTH"
  expect "listing the web origins of client $CLIENT_ID" 200
  if grep -qF "\"value\":\"$origin\"" /tmp/body; then
    echo "ferriskey-init: web origin $origin already registered, leaving it alone"
  else
    echo "ferriskey-init: registering the web origin $origin"
    http -X POST "$API_URL/realms/$REALM/clients/$CLIENT_UUID/web-origins" -H "$AUTH" -H 'content-type: application/json' \
      -d "{\"value\":\"$origin\"}"
    expect "registering the web origin $origin" 201
  fi
}

ensure_app_origin "$APP_ORIGIN"
for extra in $EXTRA_APP_ORIGINS; do
  ensure_app_origin "$extra"
done

if [ "${FERRISKEY_MCP:-true}" != "true" ]; then
  echo "ferriskey-init: FERRISKEY_MCP is not true, skipping the MCP gateway"
else
  json_body_of() {
    sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' "$1" | awk 'BEGIN { ORS = "\\n" } { print }'
  }

  ensure_client() {
    http "$API_URL/realms/$REALM/clients?client_id=$1" -H "$AUTH"
    expect "listing clients" 200
    ENSURED_UUID=$(client_uuid_in_body "$1")
    if [ -n "$ENSURED_UUID" ]; then
      echo "ferriskey-init: client $1 already exists as $ENSURED_UUID, leaving it alone"
    else
      echo "ferriskey-init: creating client $1"
      http -X POST "$API_URL/realms/$REALM/clients" -H "$AUTH" -H 'content-type: application/json' \
        -d "{\"client_id\":\"$1\",\"name\":\"$2\",\"client_type\":\"confidential\",\"protocol\":\"openid-connect\",\"enabled\":true,\"public_client\":false$3}"
      expect "creating client $1" 201
      ENSURED_UUID=$(json_field id /tmp/body)
      if [ -z "$ENSURED_UUID" ]; then
        echo "ferriskey-init: creating client $1 answered 201 with no id: $(cat /tmp/body)" >&2
        exit 1
      fi
    fi
  }

  secret_of() {
    http "$API_URL/realms/$REALM/clients/$1/client-secret" -H "$AUTH"
    expect "reading the secret of client $1" 200
    SECRET_VALUE=$(json_field client_secret /tmp/body)
    if [ -z "$SECRET_VALUE" ]; then
      echo "ferriskey-init: the secret of client $1 came back empty: $(cat /tmp/body)" >&2
      exit 1
    fi
  }

  lookup_scope() {
    http "$API_URL/realms/$REALM/client-scopes?limit=100" -H "$AUTH"
    expect "listing client scopes" 200
    SCOPE_ID=$(grep -o "\"id\":\"[^\"]*\",\"realm_id\":\"[^\"]*\",\"name\":\"$1\"" /tmp/body | head -n 1 | sed -n 's/^"id":"\([^"]*\)".*/\1/p')
  }

  ensure_scope() {
    lookup_scope "$1"
    if [ -n "$SCOPE_ID" ]; then
      echo "ferriskey-init: client scope $1 already exists, leaving it alone"
    else
      echo "ferriskey-init: creating client scope $1"
      http -X POST "$API_URL/realms/$REALM/client-scopes" -H "$AUTH" -H 'content-type: application/json' \
        -d "{\"name\":\"$1\",\"protocol\":\"openid-connect\",\"description\":\"$2\",\"is_default\":false,\"dynamic_registration_allowed\":$3}"
      expect "creating client scope $1" 201
      SCOPE_ID=$(json_field id /tmp/body)
    fi
  }

  ensure_mapper() {
    http "$API_URL/realms/$REALM/client-scopes/$1" -H "$AUTH"
    expect "reading client scope $1" 200
    if grep -qF "\"name\":\"$2\",\"mapper_type\"" /tmp/body; then
      echo "ferriskey-init: mapper $2 already exists, leaving it alone"
    else
      echo "ferriskey-init: adding mapper $2"
      http -X POST "$API_URL/realms/$REALM/client-scopes/$1/protocol-mappers" -H "$AUTH" -H 'content-type: application/json' \
        -d "{\"name\":\"$2\",\"mapper_type\":\"$3\",\"config\":$4}"
      expect "adding mapper $2" 201
    fi
  }

  echo "ferriskey-init: turning authorization on for realm $REALM"
  http -X PUT "$API_URL/realms/$REALM/authz/state" -H "$AUTH" -H 'content-type: application/json' -d '{"enabled":true}'
  expect "turning authorization on" 200

  ensure_client "$AGENT_AUDIENCE" "FerrisTrace agents" ""
  ensure_client "$MCP_GATEWAY_CLIENT_ID" "FerrisTrace MCP gateway" ',"service_account_enabled":true,"token_exchange_enabled":true'
  GATEWAY_UUID="$ENSURED_UUID"
  secret_of "$GATEWAY_UUID"
  GATEWAY_SECRET="$SECRET_VALUE"
  ensure_client "$MCP_PEP_CLIENT_ID" "FerrisTrace MCP policy enforcement point" ',"service_account_enabled":true'
  PEP_UUID="$ENSURED_UUID"
  secret_of "$PEP_UUID"
  PEP_SECRET="$SECRET_VALUE"

  echo "ferriskey-init: checking role $MCP_PEP_ROLE"
  http "$API_URL/realms/$REALM/roles?limit=100" -H "$AUTH"
  expect "listing roles" 200
  PEP_ROLE_ID=$(grep -o "\"id\":\"[^\"]*\",\"name\":\"$MCP_PEP_ROLE\"" /tmp/body | head -n 1 | sed -n 's/^"id":"\([^"]*\)".*/\1/p')
  if [ -z "$PEP_ROLE_ID" ]; then
    echo "ferriskey-init: creating role $MCP_PEP_ROLE"
    http -X POST "$API_URL/realms/$REALM/roles" -H "$AUTH" -H 'content-type: application/json' \
      -d "{\"name\":\"$MCP_PEP_ROLE\",\"description\":\"Asks this realm for authorization decisions on behalf of the FerrisTrace MCP gateway\",\"permissions\":[\"evaluate_authorization\"]}"
    expect "creating role $MCP_PEP_ROLE" 201
    PEP_ROLE_ID=$(json_field id /tmp/body)
  fi

  http "$API_URL/realms/$REALM/users?username=service-account-$MCP_PEP_CLIENT_ID" -H "$AUTH"
  expect "finding the service account of $MCP_PEP_CLIENT_ID" 200
  PEP_USER_ID=$(grep -o "\"id\":\"[^\"]*\",\"realm_id\":\"[^\"]*\",\"client_id\":\"[^\"]*\",\"username\":\"service-account-$MCP_PEP_CLIENT_ID\"" /tmp/body | head -n 1 | sed -n 's/^"id":"\([^"]*\)".*/\1/p')
  if [ -z "$PEP_USER_ID" ]; then
    echo "ferriskey-init: client $MCP_PEP_CLIENT_ID has no service account user" >&2
    exit 1
  fi
  http "$API_URL/realms/$REALM/users/$PEP_USER_ID/roles" -H "$AUTH"
  expect "reading the roles of the policy service account" 200
  if grep -qF "\"name\":\"$MCP_PEP_ROLE\"" /tmp/body; then
    echo "ferriskey-init: the policy service account already holds $MCP_PEP_ROLE"
  else
    echo "ferriskey-init: granting $MCP_PEP_ROLE to the policy service account"
    http -X POST "$API_URL/realms/$REALM/users/$PEP_USER_ID/roles/$PEP_ROLE_ID" -H "$AUTH"
    expect "granting $MCP_PEP_ROLE" 200
  fi

  echo "ferriskey-init: allowing the gateway to exchange tokens for $AGENT_AUDIENCE"
  http -X POST "$API_URL/realms/$REALM/clients/$GATEWAY_UUID/token-exchange-policies" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"target_audience\":\"$AGENT_AUDIENCE\",\"allow_delegation\":true,\"allow_impersonation\":false}"
  expect "creating the token exchange policy" 201 409

  for scope in telemetry:read incidents:comment incidents:resolve oncall:acknowledge; do
    ensure_scope "$scope" "FerrisTrace agent scope $scope" true
  done
  ensure_scope groups "The groups of the person, for authorization rules" true
  ensure_scope "$AGENT_AUDIENCE" "Puts $AGENT_AUDIENCE in the audience, for the MCP gateway only" false
  AUDIENCE_SCOPE_ID="$SCOPE_ID"
  ensure_mapper "$AUDIENCE_SCOPE_ID" "$AGENT_AUDIENCE-audience" oidc-audience-mapper \
    "{\"included.client.audience\":\"$AGENT_AUDIENCE\",\"access.token.claim\":\"true\",\"id.token.claim\":\"false\"}"
  lookup_scope groups
  ensure_mapper "$SCOPE_ID" groups oidc-group-membership-mapper \
    '{"claim.name":"groups","full.path":"false","membership":"effective","access.token.claim":"true","id.token.claim":"false"}'
  lookup_scope telemetry:read
  TELEMETRY_SCOPE_ID="$SCOPE_ID"
  http "$API_URL/realms/$REALM/clients/$GATEWAY_UUID/client-scopes" -H "$AUTH"
  expect "reading the client scopes of the gateway" 200
  cp /tmp/body /tmp/gateway-scopes
  for attached in "$AUDIENCE_SCOPE_ID" "$TELEMETRY_SCOPE_ID"; do
    if grep -qF "\"id\":\"$attached\",\"realm_id\"" /tmp/gateway-scopes; then
      echo "ferriskey-init: the gateway already holds client scope $attached"
    else
      http -X PUT "$API_URL/realms/$REALM/clients/$GATEWAY_UUID/default-client-scopes/$attached" -H "$AUTH"
      expect "attaching a client scope to the gateway" 200
    fi
  done

  PUBLIC_BASE="${MCP_PUBLIC_URL%/}"
  WANTED_RESOURCES=""
  for org in $(echo "$MCP_ORGS" | tr ',' ' '); do
    case "$org" in
      *[!0-9a-fA-F-]*)
        echo "ferriskey-init: $org is not an organisation id" >&2
        exit 1
        ;;
    esac
    WANTED_RESOURCES="$WANTED_RESOURCES $PUBLIC_BASE/orgs/$org/mcp"
  done

  echo "ferriskey-init: enabling MCP client registration"
  http "$API_URL/realms/$REALM" -H "$AUTH"
  expect "reading the realm" 200
  KNOWN_RESOURCES=$(grep -o '"allowed_resources":\[[^]]*\]' /tmp/body | head -n 1 | sed -n 's/^"allowed_resources":\[\(.*\)\]$/\1/p')
  ALLOWED_RESOURCES="$KNOWN_RESOURCES"
  for resource in $WANTED_RESOURCES; do
    case ",$ALLOWED_RESOURCES," in
      *",\"$resource\","*) ;;
      *) ALLOWED_RESOURCES="${ALLOWED_RESOURCES:+$ALLOWED_RESOURCES,}\"$resource\"" ;;
    esac
  done
  http -X PUT "$API_URL/realms/$REALM/settings" -H "$AUTH" -H 'content-type: application/json' \
    -d "{\"cimd_enabled\":true,\"dcr_enabled\":true,\"allowed_resources\":[$ALLOWED_RESOURCES]}"
  expect "enabling MCP client registration" 200

  if [ -n "$WANTED_RESOURCES" ]; then
    echo "ferriskey-init: making the gateway the owner of the MCP resources"
    http "$API_URL/realms/$REALM/resource-owners" -H "$AUTH"
    expect "reading the resource owners" 200
    OWNERS=$(grep -o '{"uri":"[^"]*","client_id":"[^"]*"}' /tmp/body | paste -sd, -)
    for resource in $WANTED_RESOURCES; do
      if ! grep -qF "\"uri\":\"$resource\"" /tmp/body; then
        OWNERS="${OWNERS:+$OWNERS,}{\"uri\":\"$resource\",\"client_id\":\"$GATEWAY_UUID\"}"
      fi
    done
    http -X PUT "$API_URL/realms/$REALM/resource-owners" -H "$AUTH" -H 'content-type: application/json' \
      -d "{\"owners\":[$OWNERS]}"
    expect "setting the resource owners" 200
  fi

  http "$API_URL/api-docs/openapi.json"
  expect "reading the api description" 200
  if grep -qF "ContextExtension" /tmp/body; then
    echo "ferriskey-init: this FerrisKey lets a realm extend the agent types"
    http "$API_URL/realms/$REALM/authz/schema" -H "$AUTH"
    expect "reading the realm schema" 200 404
    if [ "$(cat /tmp/status)" = "404" ] || grep -qF "\"namespace\":\"$MCP_SCHEMA_NAMESPACE\"" /tmp/body; then
      echo "ferriskey-init: storing the $MCP_SCHEMA_NAMESPACE schema"
      SCHEMA_SOURCE=$(json_body_of "$MCP_SCHEMA_FILE")
      http -X PUT "$API_URL/realms/$REALM/authz/schema" -H "$AUTH" -H 'content-type: application/json' \
        -d "{\"namespace\":\"$MCP_SCHEMA_NAMESPACE\",\"source\":\"$SCHEMA_SOURCE\"}"
      expect "storing the schema" 200
    else
      echo "ferriskey-init: the realm already has a schema of its own, leaving it alone: add the types of $MCP_SCHEMA_FILE to it" >&2
    fi
    SEED_FILE="$MCP_SEED_POLICY"
  else
    echo "ferriskey-init: this FerrisKey keeps only part of the AuthZEN request and checks a stored schema against its own agent context, so no schema is stored and the seeded policy names its tools (see docs/mcp.md)"
    SEED_FILE="$MCP_SEED_POLICY_LEGACY"
  fi

  echo "ferriskey-init: checking the seeded policy $MCP_SEED_NAME"
  http "$API_URL/realms/$REALM/authz/policies?managed_by=$MCP_MANAGED_BY" -H "$AUTH"
  expect "listing the seeded policies" 200
  if grep -qF "\"name\":\"$MCP_SEED_NAME\"" /tmp/body; then
    echo "ferriskey-init: policy $MCP_SEED_NAME already exists, leaving it alone"
  else
    echo "ferriskey-init: creating policy $MCP_SEED_NAME"
    SEED_SOURCE=$(json_body_of "$MCP_POLICY_DIR/$SEED_FILE")
    http -X POST "$API_URL/realms/$REALM/authz/policies?managed_by=$MCP_MANAGED_BY" -H "$AUTH" -H 'content-type: application/json' \
      -d "{\"name\":\"$MCP_SEED_NAME\",\"description\":\"Members of the realm may call the read-only FerrisTrace MCP tools\",\"enabled\":true,\"body\":{\"type\":\"static\",\"source\":\"$SEED_SOURCE\"}}"
    expect "creating policy $MCP_SEED_NAME" 201
  fi

  echo "ferriskey-init: writing $MCP_OUT_FILE"
  cat > "$MCP_OUT_FILE.tmp" <<EOF
FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_ID=$MCP_PEP_CLIENT_ID
FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_SECRET=$PEP_SECRET
FERRISTRACE_MCP_GATEWAY_EXCHANGE__CLIENT_ID=$MCP_GATEWAY_CLIENT_ID
FERRISTRACE_MCP_GATEWAY_EXCHANGE__CLIENT_SECRET=$GATEWAY_SECRET
EOF
  chmod 600 "$MCP_OUT_FILE.tmp"
  chown "$READER" "$MCP_OUT_FILE.tmp"
  mv "$MCP_OUT_FILE.tmp" "$MCP_OUT_FILE"
fi

echo "ferriskey-init: writing $OUT_FILE"
cat > "$OUT_FILE.tmp" <<EOF
FERRISTRACE_OIDC__ISSUER=$ISSUER_BASE_URL/realms/$REALM
FERRISTRACE_OIDC__CLIENT_ID=$CLIENT_ID
EOF
# The owner is the user the server and the gateway run as, since they read
# it themselves.
chmod 600 "$OUT_FILE.tmp"
chown "$READER" "$OUT_FILE.tmp"
mv "$OUT_FILE.tmp" "$OUT_FILE"
echo "ferriskey-init: done"
