"""Orbit energy check (Python-side analytic) — mirrors crates/ws-engine/tests/validation.rs.

This example needs a world with N-body enabled (not yet exposed via the HTTP config
in v0.1, so it illustrates the *intended* API and currently prints a note).

For a real orbital validation, use the Rust validation suite:
    cargo test -p ws-engine circular_orbit_energy_is_conserved -- --nocapture
"""
print("Orbit demo — N-body HTTP config is on the roadmap (v0.2).")
print("Run:  cargo test -p ws-engine circular_orbit_energy_is_conserved -- --nocapture")
print("That test integrates two 1e10 kg bodies 2 m apart for 5 periods and checks")
print("energy drift < 5e-4 and radius drift < 5%. See the test for the full analytic setup.")
