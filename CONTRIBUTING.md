# Contributing to WorldSim

Thank you for wanting to help build a reliable physics infrastructure.

## Quick start

```bash
git clone https://github.com/hyqzz/worldsim
cd worldsim
export PATH="$HOME/.cargo/bin:$PATH"

# Every crate must stay warning-free
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Python SDK (no deps)
python -m compileall sdk/python/worldsim
```

## What to contribute

- **Physics correctness first.**  Any new solver or coupling change must ship with an analytic validation test under `crates/ws-engine/tests/validation.rs` (or a new integration test file).  Convergence tests trump eyeballing.
- **Contract-first.**  API/JSON/wire-format changes require an update to `docs/api.md` and `proto`-ish change notes in the PR description.  Snapshot format bumps `SNAPSHOT_VERSION` and must include a migration test.
- **Zero-dependency discipline.**  The runtime crates (`ws-core`, `ws-domain`, `ws-solvers`, `ws-engine`, `ws-server`) must stay at zero third-party dependencies unless the change is discussed in an issue and justifies the supply-chain cost.  Dev dependencies are fine.

## PR checklist

- [ ] `cargo fmt` clean
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test --workspace` green
- [ ] Added/updated docs for user-facing changes
- [ ] If touching numerics: new or updated validation test with analytic reference
