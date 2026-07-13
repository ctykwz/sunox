# Releasing Sunox

Releases are allowed only from commits reachable from `main`. The `v*` tag
ruleset restricts release-tag creation, update, and deletion to repository
administrators, and the `release` environment requires an explicit approval.

## Signing policy

Sunox publishes unsigned platform archives. This keeps releases free of paid
Apple and Windows signing certificates while preserving the static Windows CRT,
SHA256 checksums, CycloneDX SBOM, and GitHub provenance attestations. macOS and
Windows users may see platform security warnings and should verify the checksum
before running the binary.

Never add Clerk values, tokens, or other credentials to the repository.

## Release procedure

1. Merge the version and changelog change through a pull request into `main`.
2. Wait for all required `main` checks to pass.
3. Create `v<version>` at that `main` commit and push the tag as an
   administrator.
4. Review and approve the pending `release` environment deployment.
5. Verify the workflow produced platform archives, `SHA256SUMS`,
   `sunox.cdx.json`, and GitHub artifact attestations before approving
   downstream use.

Linux GNU artifacts are linked against a glibc 2.28 baseline. Windows uses the
static Visual C++ runtime. The platform archives are unsigned and do not include
macOS installer packages.
