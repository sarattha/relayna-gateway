#!/usr/bin/env bash
set -Eeuo pipefail

output_dir="target/development-oidc"
days=30

usage() {
  printf '%s\n' \
    'Usage: generate-development-portal-certificate.sh [--output-dir DIR] [--days DAYS]' \
    '' \
    'Creates a short-lived development-only RSA private key and X.509 certificate.' \
    'The command refuses to overwrite existing certificate material.'
}

while (($# > 0)); do
  case "$1" in
    --output-dir)
      [[ -n "${2:-}" ]] || { printf 'Missing value for %s\n' "$1" >&2; exit 2; }
      output_dir="$2"
      shift 2
      ;;
    --days)
      [[ "${2:-}" =~ ^[1-9][0-9]*$ ]] || { printf '%s\n' '--days must be a positive integer' >&2; exit 2; }
      days="$2"
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

command -v openssl >/dev/null 2>&1 || { printf '%s\n' 'Required command not found: openssl' >&2; exit 1; }

private_key="${output_dir}/portal-private-key.pem"
certificate="${output_dir}/portal-certificate.pem"
for path in "${private_key}" "${certificate}"; do
  [[ ! -e "${path}" ]] || { printf 'Refusing to overwrite existing file: %s\n' "${path}" >&2; exit 1; }
done

umask 077
mkdir -p "${output_dir}"
openssl req -x509 -newkey rsa:3072 -sha256 -nodes \
  -keyout "${private_key}" \
  -out "${certificate}" \
  -days "${days}" \
  -subj '/CN=relayna-development-portal'
chmod 600 "${private_key}"
chmod 644 "${certificate}"

printf 'Created development portal certificate material in %s\n' "${output_dir}"
printf 'Private key: %s\n' "${private_key}"
printf 'Public certificate: %s\n' "${certificate}"
printf '%s\n' 'Never commit or register the private key. Register only the public certificate.'
