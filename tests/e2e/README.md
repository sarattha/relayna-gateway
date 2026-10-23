# Gateway E2E harnesses

These reusable Docker Compose harnesses exercise Gateway against PostgreSQL,
Redis, mock identity/provider services, and real LiteLLM where required:

- `entra-front-door-real-env/`: Entra validation, Apigee trusted headers,
  credential stripping, and usage attribution.
- `front-door-penetration/`: rejection and credential-isolation checks across
  direct, Entra, and Apigee front doors.
- `litellm-real-passthrough/`: managed/direct routes and LiteLLM passthrough.

From the repository root, run one harness at a time:

```bash
bash tests/e2e/entra-front-door-real-env/run.sh
bash tests/e2e/front-door-penetration/run.sh
bash tests/e2e/litellm-real-passthrough/run.sh
```

Docker Compose must be available. Each script builds the current Gateway and
starts its local stack. JSON results and Markdown reports are written under
ignored `target/e2e/<harness>/`; generated reports and screenshots do not belong
in the source tree. Credentials in these fixtures are local test values.

Stop a stack after use, for example:

```bash
docker compose -f tests/e2e/entra-front-door-real-env/docker-compose.yml down -v
```

The shared Entra application/role contract is also checked without Docker:

```bash
node --test scripts/entra/entra-integration.test.mjs
```
