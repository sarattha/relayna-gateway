#!/usr/bin/env bash
set -Eeuo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repository_root="$(cd -- "${script_dir}/../.." && pwd)"
compose_file="${script_dir}/docker-compose.yml"
certificate_dir="${repository_root}/target/development-oidc"
rebuild=false

usage() {
  printf '%s\n' \
    'Usage: deploy/local/run.sh [--rebuild]' \
    '' \
    'Starts the isolated project-owner inspection stack and seeds mock data.' \
    'Use --rebuild to force fresh local Gateway, OIDC, and mock-upstream images.'
}

while (($# > 0)); do
  case "$1" in
    --rebuild)
      rebuild=true
      shift
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

if [[ ! -f "${certificate_dir}/portal-private-key.pem" || ! -f "${certificate_dir}/portal-certificate.pem" ]]; then
  "${repository_root}/scripts/entra/generate-development-portal-certificate.sh" \
    --output-dir "${certificate_dir}" \
    --days 30
fi

if [[ "${rebuild}" == true ]]; then
  docker compose -f "${compose_file}" build --no-cache
  docker compose -f "${compose_file}" up -d --force-recreate
else
  docker compose -f "${compose_file}" up -d --build --force-recreate
fi
docker compose -f "${compose_file}" wait seed

for _ in {1..60}; do
  if curl --fail --silent --show-error http://127.0.0.1:18381/admin-ui/readyz >/dev/null; then
    printf '%s\n' \
      'Relayna local inspection stack is ready.' \
      'Admin UI: http://127.0.0.1:18381/admin-ui' \
      'Choose “Analytics Project Owner” in the development sign-in screen.' \
      'Gateway proxy: http://127.0.0.1:18380' \
      'Mock upstream: http://127.0.0.1:18382'
    exit 0
  fi
  sleep 1
done

printf '%s\n' 'Gateway did not become ready within 60 seconds.' >&2
docker compose -f "${compose_file}" ps >&2
docker compose -f "${compose_file}" logs --tail=100 gateway seed >&2
exit 1
