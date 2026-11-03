# Runtime image security

[Issue #149](https://github.com/sarattha/relayna-gateway/issues/149) records
57 High/Critical OS-package findings in the published v1.0.1 amd64 image,
covering 12 CVEs. A package record is not proof of a remotely exploitable
Gateway path. The original image remains immutable; rebuilding source does not
repair an already published or deployed digest.

## Packaging boundary

The production Dockerfile builds on Debian 13 and uses the maintained
`gcr.io/distroless/cc-debian13:nonroot` runtime, pinned by multi-platform index
digest. The [upstream image contents](https://github.com/GoogleContainerTools/distroless/blob/main/base/README.md)
provide glibc, OpenSSL, CA certificates, zlib and zstd; the cc variant adds the
GCC runtime. These support the Rust executable, portal RSA certificate parsing,
TLS and the existing glibc allocator setting. Distribution shells, package
managers and unrelated utilities are omitted. Package metadata is retained for
scanning; findings are not cleared by deleting inventory records.

UID/GID 10001, the Relayna account, executable path, ports, configuration,
HTTP/authentication/proxy contracts, embedded UI and migrations remain intact.
Version 1.0.2 packages this remediation without a data migration. Operators
must use an external debug container instead of executing a shell in Gateway.
The body-admission harness mounts its own static sampler tools only in
benchmark pods; see its
[methodology](https://github.com/sarattha/relayna-gateway/blob/main/benches/body-admission/README.md).
The [v1.0.2 release benchmark](benchmarks/v1.0.2.md) must validate this new
runtime before publication. Earlier release benchmarks do not validate it.

## Triage of the reported CVEs

The component descriptions below come from Debian's security tracker. Source
inspection found no production subprocess invocation or use of the named
utility/ACL/disk-probing functions. Gateway processes HTTP, JSON and protocol
streams in Rust; it does not invoke gzip or Perl to decode request bodies.
Absence of a direct Gateway call is a reachability assessment, not a general
claim that an affected package is safe after container compromise.

| CVE | Affected component and v1.0.1 applicability | Candidate disposition |
| --- | --- | --- |
| [CVE-2023-45853](https://security-tracker.debian.org/tracker/CVE-2023-45853) | MiniZip ZIP writer. Debian says Bookworm's zlib binary does not build the vulnerable contrib/minizip code; the reported `zlib1g` record is a source-package association. | Debian 13 zlib is fixed; no MiniZip is added. |
| [CVE-2025-69720](https://security-tracker.debian.org/tracker/CVE-2025-69720) | `infocmp` string analysis; no Gateway invocation. The ncurses source finding spans library/data records as well as the CLI. | ncurses packages and `infocmp` omitted. |
| [CVE-2026-16742](https://security-tracker.debian.org/tracker/CVE-2026-16742) | Local group escalation in systemd-homed. The image's `libsystemd0`/`libudev1` records do not establish that homed is installed or running. Gateway does not manage homed accounts. | systemd/udev packages omitted. |
| [CVE-2026-41992](https://security-tracker.debian.org/tracker/CVE-2026-41992) | Stateful LZW/LZH decompression in GNU gzip; no Gateway invocation. | gzip utility omitted. |
| [CVE-2026-53613](https://security-tracker.debian.org/tracker/CVE-2026-53613) | Restricted fstab mount target race requiring a local authorized user and writable path ancestor. No Gateway mount operation. | util-linux packages omitted. |
| [CVE-2026-53615](https://security-tracker.debian.org/tracker/CVE-2026-53615) | DOS extended-partition arithmetic in libblkid; Gateway does not probe disk images. | libblkid and util-linux packages omitted. |
| [CVE-2026-54369](https://security-tracker.debian.org/tracker/CVE-2026-54369) | Path-based libacl symlink traversal requires a privileged ACL caller; no Gateway ACL call. | libacl omitted. |
| [CVE-2026-76642](https://security-tracker.debian.org/tracker/CVE-2026-76642) | Mount helper/post-mount hooks. Debian now marks Bookworm not affected: vulnerable code was introduced in util-linux 2.39. | util-linux packages omitted. |
| [CVE-2026-78408](https://security-tracker.debian.org/tracker/CVE-2026-78408) | `nsenter --join-cgroup` descriptor leak. Debian now marks Bookworm not affected: introduced in 2.40. | util-linux and `nsenter` omitted. |
| [CVE-2026-78409](https://security-tracker.debian.org/tracker/CVE-2026-78409) | `X-mount.subdir` detached-tree path traversal. Debian now marks Bookworm not affected: introduced in 2.42. | util-linux packages omitted. |
| [CVE-2026-78410](https://security-tracker.debian.org/tracker/CVE-2026-78410) | Restricted bind-mount source race; no Gateway mount operation. | util-linux packages omitted. |
| [CVE-2026-9538](https://security-tracker.debian.org/tracker/CVE-2026-9538) | Perl Archive::Tar unbounded entry allocation. `perl-base` alone does not establish that Archive::Tar is installed; Gateway does not use Perl. | Perl packages omitted. |

This assesses all 12 reported CVEs without a scanner ignore. Upstream deferred
fixes for omitted packages do not require carrying those packages in Gateway.
Kernel/container-host vulnerabilities and optional debug images are separate
artifacts and must be assessed independently.

## Verification and future base refreshes

CI builds the production Dockerfile, runs the runtime-image smoke harness and
blocks on **all** High/Critical image dispositions, including unfixed findings.
The release image gate follows the same rule. The runtime smoke uses a fresh
PostgreSQL database, Redis and bounded synthetic HTTP/SSE/WebSocket/TLS upstream,
with a read-only root, dropped capabilities and the image's UID/GID. It verifies
startup migrations/readiness, mounted portal RSA credentials, assets, admin and
proxy denials, forwarding, custom trust, Usage, monitoring and graceful stop.

The smoke allows 330 seconds for SIGTERM: the unchanged Pingora 0.9 default
grace period is five minutes, followed by runtime shutdown. A 45-second smoke
stop ended with exit 137 after the preceding functional checks passed. The
existing Kubernetes manifest's 45-second termination grace can therefore force
termination before that default finishes; this packaging change does not repair
that separate shutdown configuration mismatch.

For a base digest refresh, rebuild and scan the resulting image, not only the
base. Resolve any new findings or use the documented
[exception process](security-exceptions.md); do not add blanket ignores or
`--ignore-unfixed` to clear the gate. Retain full JSON (including lower
severities), scanner version, DB metadata, image ID/digest, build inputs and
checksums under ignored `target/` or `output/`. Scan each architecture being
published and smoke-test each on a native runner before releasing it.

```sh
docker build --pull -t relayna-gateway:security-candidate .
python3 tests/e2e/runtime-image/run.py --image relayna-gateway:security-candidate
mkdir -p target/runtime-image-security
docker image inspect relayna-gateway:security-candidate \
  --format '{{.Id}} {{.Architecture}}'
trivy --version
trivy image --scanners vuln --list-all-pkgs --format json \
  --output target/runtime-image-security/trivy.json \
  relayna-gateway:security-candidate
make security-image IMAGE=relayna-gateway:security-candidate
```

Vulnerability DB updates can change results for an unchanged image. On
2026-11-03, Trivy 0.76.1 reported 33 High/Critical records across nine CVEs for
the original immutable v1.0.1 amd64 manifest, compared with the issue's older
57 records. That DB change is not a packaging fix; retain both observations.

The rebuilt ARM64 and amd64 candidates both reported **zero High/Critical**
findings with that same database, without an ignore file or unfixed-finding
filter. Each retained **36 Medium and 8 Low** package records across the
14-package runtime inventory. The DB's `UpdatedAt` was
`2026-11-03T13:13:42.304243913Z` (schema version 2). These lower-severity findings
remain disclosed in the full JSON; this is not a claim of a vulnerability-free
image. Local ARM64 execution was native on Apple M1 Pro; amd64 execution used
emulation and does not substitute for native release benchmarks.

These 3 November scans measured the packaging preparation. The committed
v1.0.2 runtime requires its own image validation and native release study,
recorded in the [versioned results](benchmarks/v1.0.2.md).
