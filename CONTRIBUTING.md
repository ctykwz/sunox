# Contributing

Use Rust 1.88 or newer and keep `Cargo.lock` authoritative. New capabilities should follow the
dependency direction in [ARCHITECTURE.md](ARCHITECTURE.md) and add the narrowest useful test at
each affected boundary.

Before committing, run:

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo llvm-cov --locked --all-targets --fail-under-lines 72
cargo audit
cargo package --locked --allow-dirty
```

The coverage command requires `cargo-llvm-cov` 0.9.0 and the
`llvm-tools-preview` rustup component. CI installs both explicitly.

Do not use production credentials or make real Suno writes in automated tests. Public-binary HTTP
tests must use `SUNOX_TEST_API_BASE_URL`, which debug builds restrict to a plain-HTTP loopback
origin. Do not add a release-build endpoint override.

For a protocol change, test the exact method/path/body and the response or error shape. For a
mutation, additionally test no redirect replay, ambiguity classification, and resource readback or
checkpoint guidance. Update [the capability matrix](CAPABILITY_TEST_MATRIX.md) when adding or
removing a user-visible capability.
