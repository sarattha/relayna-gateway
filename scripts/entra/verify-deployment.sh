#!/usr/bin/env bash
set -Eeuo pipefail

namespace="default"
control_ingress_namespace="ingress-nginx-internal"
certificate_file=""

usage() {
  printf '%s\n' \
    'Usage: verify-deployment.sh [--namespace NAME] [--control-ingress-namespace NAME] [--certificate-file FILE]' \
    '' \
    'Read-only verification of the deployed Relayna Entra ConfigMap, Secrets,' \
    'certificate pair, Deployment, private Ingress, NetworkPolicy, and readiness.'
}

while (($# > 0)); do
  case "$1" in
    --namespace)
      [[ -n "${2:-}" ]] || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
      namespace="$2"
      shift 2
      ;;
    --control-ingress-namespace)
      [[ -n "${2:-}" ]] || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
      control_ingress_namespace="$2"
      shift 2
      ;;
    --certificate-file)
      [[ -n "${2:-}" ]] || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
      certificate_file="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

for command_name in kubectl jq openssl; do
  command -v "${command_name}" >/dev/null 2>&1 || { printf 'Required command not found: %s\n' "${command_name}" >&2; exit 1; }
done
if [[ -n "${certificate_file}" ]]; then
  [[ -r "${certificate_file}" ]] || { printf 'Certificate is not readable: %s\n' "${certificate_file}" >&2; exit 1; }
  openssl x509 -in "${certificate_file}" -noout >/dev/null 2>&1 || { printf 'Certificate is not valid X.509 PEM: %s\n' "${certificate_file}" >&2; exit 1; }
fi

config_json="$(kubectl --namespace "${namespace}" get configmap relayna-gateway-config --output json)"
deployment_json="$(kubectl --namespace "${namespace}" get deployment relayna-gateway --output json)"
ingress_json="$(kubectl --namespace "${namespace}" get ingress relayna-gateway-control --output json)"
policy_json="$(kubectl --namespace "${namespace}" get networkpolicy relayna-gateway --output json)"
ingress_namespace_json="$(kubectl get namespace "${control_ingress_namespace}" --output json)"

failures=0
check() {
  local label="$1"
  shift
  if "$@" >/dev/null; then
    printf 'PASS: %s\n' "${label}"
  else
    printf 'FAIL: %s\n' "${label}" >&2
    failures=$((failures + 1))
  fi
}

check_config_value() {
  local name="$1"
  local expected="$2"
  check "ConfigMap ${name}=${expected}" jq -e --arg name "${name}" --arg expected "${expected}" \
    '.data[$name] == $expected' <<<"${config_json}"
}

check_config_nonempty() {
  local name="$1"
  check "ConfigMap ${name} is configured" jq -e --arg name "${name}" \
    '.data[$name] | type == "string" and length > 0' <<<"${config_json}"
}

check_config_value PORTAL_OIDC_ENABLED true
check_config_nonempty PORTAL_OIDC_TENANT_ID
check_config_nonempty PORTAL_OIDC_CLIENT_ID
check_config_value PORTAL_OIDC_PRIVATE_KEY_PATH /var/run/secrets/relayna-portal-oidc/portal-private-key.pem
check_config_value PORTAL_OIDC_CERTIFICATE_PATH /var/run/secrets/relayna-portal-oidc/portal-certificate.pem
check_config_nonempty PORTAL_OIDC_ISSUER
check_config_nonempty PORTAL_OIDC_DISCOVERY_URL
check "portal redirect uses exact callback route" jq -e \
  '.data.PORTAL_OIDC_REDIRECT_URI | test("^https://[^?#]+/admin-ui/auth/callback$")' <<<"${config_json}"
check "portal logout redirect uses exact admin route" jq -e \
  '.data.PORTAL_OIDC_POST_LOGOUT_REDIRECT_URI | test("^https://[^?#]+/admin-ui$")' <<<"${config_json}"
check_config_value PORTAL_SESSION_COOKIE_SECURE true
check_config_value OWNER_ENTRA_AUTH_ENABLED true
check_config_nonempty OWNER_ENTRA_TENANT_ID
check_config_nonempty OWNER_ENTRA_AUDIENCE
check_config_nonempty OWNER_ENTRA_ISSUER
check_config_nonempty OWNER_ENTRA_OIDC_DISCOVERY_URL

if jq -e '.data.PORTAL_ADMIN_EMAILS | type == "string" and length > 0' <<<"${config_json}" >/dev/null; then
  check_config_nonempty PORTAL_ADMIN_OBJECT_IDS
  printf '%s\n' 'INFO: immutable email/object-ID pairs are configured for first-administrator bootstrap; remove both settings after persisted Admin roles are verified.'
elif jq -e '.data.PORTAL_ADMIN_OBJECT_IDS | type == "string" and length > 0' <<<"${config_json}" >/dev/null; then
  check "PORTAL_ADMIN_OBJECT_IDS is empty when PORTAL_ADMIN_EMAILS is empty" false
else
  printf '%s\n' 'INFO: PORTAL_ADMIN_EMAILS and PORTAL_ADMIN_OBJECT_IDS are empty, as expected after first-administrator bootstrap.'
fi

check "certificate Secret is mounted read-only" jq -e \
  'any(.spec.template.spec.containers[];
    .name == "gateway" and any(.volumeMounts[]?;
      .name == "portal-oidc-certificate"
      and .mountPath == "/var/run/secrets/relayna-portal-oidc"
      and .readOnly == true))' <<<"${deployment_json}"
check "certificate Secret volume uses the expected Secret" jq -e \
  'any(.spec.template.spec.volumes[]?;
    .name == "portal-oidc-certificate"
    and .secret.secretName == "relayna-gateway-portal-oidc")' <<<"${deployment_json}"

check_ingress_path() {
  local path="$1"
  check "control Ingress routes ${path}" jq -e --arg path "${path}" \
    'any(.spec.rules[].http.paths[];
      .path == $path
      and .pathType == "Prefix"
      and .backend.service.name == "relayna-gateway-control"
      and .backend.service.port.name == "control")' <<<"${ingress_json}"
}
check_ingress_path /admin-ui
check_ingress_path /owner/v1
check "internal ingress namespace carries control-plane access label" jq -e \
  '.metadata.labels["relayna.io/control-plane-access"] == "true"' <<<"${ingress_namespace_json}"
check "NetworkPolicy admits labeled control-plane namespaces on 8081" jq -e \
  'any(.spec.ingress[];
    any(.ports[]?; .port == 8081)
    and any(.from[]?;
      .namespaceSelector.matchLabels["relayna.io/control-plane-access"] == "true"))' <<<"${policy_json}"

certificate_data="$(kubectl --namespace "${namespace}" get secret relayna-gateway-portal-oidc --output go-template='{{index .data "portal-certificate.pem"}}')"
check "portal public certificate exists" test -n "${certificate_data}"

private_key_public_hash="$(kubectl --namespace "${namespace}" get secret relayna-gateway-portal-oidc --output go-template='{{index .data "portal-private-key.pem"}}' | openssl base64 -d -A | openssl pkey -pubout -outform DER 2>/dev/null | openssl dgst -sha256 -r | awk '{print $1}')" || private_key_public_hash=""
certificate_public_hash="$(printf '%s' "${certificate_data}" | openssl base64 -d -A | openssl x509 -pubkey -noout 2>/dev/null | openssl pkey -pubin -outform DER 2>/dev/null | openssl dgst -sha256 -r | awk '{print $1}')" || certificate_public_hash=""
check "portal private key is valid RSA PEM" test -n "${private_key_public_hash}"
check "private key and certificate public keys are identical" test "${private_key_public_hash}" = "${certificate_public_hash}"
check "certificate remains valid for at least seven days" bash -c \
  'printf %s "$1" | openssl base64 -d -A | openssl x509 -noout -checkend 604800 >/dev/null 2>&1' _ "${certificate_data}"

if [[ -n "${certificate_file}" ]]; then
  expected_fingerprint="$(openssl x509 -in "${certificate_file}" -outform DER | openssl dgst -sha256 -r | awk '{print $1}')"
  deployed_fingerprint="$(printf '%s' "${certificate_data}" | openssl base64 -d -A | openssl x509 -outform DER | openssl dgst -sha256 -r | awk '{print $1}')"
  check "deployed certificate matches --certificate-file" test "${deployed_fingerprint}" = "${expected_fingerprint}"
fi

check "Gateway Deployment is fully available" jq -e \
  '(.spec.replicas // 1) > 0 and (.status.availableReplicas // 0) >= (.spec.replicas // 1)' <<<"${deployment_json}"

if ((failures > 0)); then
  printf '%s\n' "${failures} Relayna Entra deployment check(s) failed." >&2
  exit 1
fi

printf '%s\n' 'All Relayna Entra deployment checks passed.'
