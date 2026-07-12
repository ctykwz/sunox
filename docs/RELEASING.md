# Releasing Sunox

Releases are allowed only from commits reachable from `main`. The `v*` tag
ruleset restricts release-tag creation, update, and deletion to repository
administrators, and the `release` environment requires an explicit approval.

## Required environment secrets

The release workflow intentionally fails instead of publishing unsigned macOS
or Windows binaries when any signing input is missing.

- `APPLE_CERTIFICATE_P12`: base64-encoded PKCS#12 containing the Developer ID
  Application and Developer ID Installer identities.
- `APPLE_CERTIFICATE_PASSWORD`
- `APPLE_SIGNING_IDENTITY`: full Developer ID Application identity name.
- `APPLE_INSTALLER_IDENTITY`: full Developer ID Installer identity name.
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`: notarization credentials.
- `WINDOWS_CERTIFICATE_PFX`: base64-encoded Authenticode certificate.
- `WINDOWS_CERTIFICATE_PASSWORD`

Store these as secrets on the protected `release` environment. Never add
certificate bytes, passwords, Clerk values, or tokens to the repository.

## Release procedure

1. Merge the version and changelog change through a pull request into `main`.
2. Wait for all required `main` checks to pass.
3. Create `v<version>` at that `main` commit and push the tag as an
   administrator.
4. Review and approve the pending `release` environment deployment.
5. Verify the workflow produced signed archives, stapled macOS installer
   packages, `SHA256SUMS`, `sunox.cdx.json`, and GitHub artifact attestations
   before approving downstream use.

Linux GNU artifacts are linked against a glibc 2.28 baseline. Windows uses the
static Visual C++ runtime. The macOS tarballs contain signed/notarized binaries;
the `.pkg` files additionally carry a stapled notarization ticket.
