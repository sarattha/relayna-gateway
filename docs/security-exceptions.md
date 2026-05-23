# Security Exceptions

Relayna Gateway CI treats dependency advisories, denied licenses, committed
secrets, high or critical filesystem/image vulnerabilities, and Semgrep
security findings as blocking by default.

## Exception Requirements

Allowed exceptions must be narrow, temporary, and documented before an ignore is
added to `.trivyignore`, `deny.toml`, `.gitleaks.toml`, `.semgrep.yml`, or a
workflow-level allowlist.

Each exception must include:

| Field | Requirement |
| --- | --- |
| Finding | Scanner and finding ID or rule name. |
| Owner | Person or team responsible for removing the exception. |
| Reason | Why the finding is not exploitable or cannot be fixed immediately. |
| Tracking link | Issue, PR, advisory, or vendor link. |
| Expiration | Date when the exception must be removed or reapproved. |

## Active Exceptions

| Finding | Owner | Reason | Tracking link | Expiration |
| --- | --- | --- | --- | --- |
| `RUSTSEC-2023-0071` | Relayna Gateway maintainers | Transitive `rsa` dependency enters through `sqlx-mysql` under SQLx macros; Gateway configures PostgreSQL only and does not expose MySQL RSA authentication. | https://rustsec.org/advisories/RUSTSEC-2023-0071 | 2026-08-31 |
| `RUSTSEC-2024-0437` | Relayna Gateway maintainers | Transitive `protobuf` dependency enters through `prometheus` under `pingora-core`; Gateway exports its own bounded Prometheus text metrics and does not parse untrusted protobuf metrics payloads. | https://rustsec.org/advisories/RUSTSEC-2024-0437 | 2026-08-31 |
| `RUSTSEC-2024-0388` | Relayna Gateway maintainers | Transitive `derivative` dependency enters through `pingora-core`; no direct Gateway code depends on it. Track upstream Pingora replacement or upgrade. | https://rustsec.org/advisories/RUSTSEC-2024-0388 | 2026-08-31 |
| `RUSTSEC-2025-0069` | Relayna Gateway maintainers | Transitive `daemonize` dependency enters through `pingora-core`; Gateway runs as a foreground container process and does not use daemonization behavior directly. Track upstream Pingora replacement or upgrade. | https://rustsec.org/advisories/RUSTSEC-2025-0069 | 2026-08-31 |
| Historical fake token fingerprints in `.gitleaksignore` | Relayna Gateway maintainers | Existing test fixtures and example tokens predate strict secret scanning. New Gitleaks findings must be fixed or documented separately. | `.gitleaksignore` | 2026-08-31 |

## Local Tooling

Install the local security tools when you need to reproduce CI:

```bash
cargo install cargo-audit --locked
cargo install cargo-deny --locked
cargo install cargo-machete --locked
cargo install cargo-nextest --locked
```

Install Trivy, Gitleaks, and Semgrep with your package manager or their
upstream installers. Then run:

```bash
make security
```

For image scanning, build an image and pass its tag explicitly:

```bash
docker build -t relayna-gateway:local .
make security-image IMAGE=relayna-gateway:local
```
