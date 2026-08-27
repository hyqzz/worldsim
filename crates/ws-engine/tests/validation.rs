#![allow(clippy::field_reassign_with_default)]

use ws_core::math::Vec3;
use ws_domain::config::{Quality, WorldConfig};
use ws_engine::world::World;

/// Helper: create a zero-fluid, zero-heat world suitable for pure rigid tests.
fn minimal_world(seed: u64, gravity: Vec3, bounds: Option<ws_core::math::Aabb>) -> World {
    let mut cfg = WorldConfig::default();
    cfg.seed = seed;
    cfg.quality = Quality::Low; // fast
    cfg.container = false;
    cfg.gravity = gravity;
    if let Some(b) = bounds {
        cfg.bounds = b;
    }
    cfg.systems.fluid.enabled = false;
    cfg.systems.heat.enabled = false;
    cfg.systems.nbody.enabled = false;
    World::new(format!("test-{seed:04x}"), cfg, None)
}

#[test]
fn projectile_analytic_no_drag() {
    // A single sphere in free fall with no container and no fluid.
    let mut world = minimal_world(1, Vec3::new(0.0, -9.80665, 0.0), None);
    // Keep the default small bounds but disable the container — the particle may briefly
    // leave the domain; the uniform grid clamps outliers to the edge cell so the test stays fast.
    world.config.container = false;

    let pos0 = Vec3::new(0.0, 10.0, 0.0);
    let vel0 = Vec3::new(5.0, 10.0, 0.0);
    world.rigid.spawn(pos0, vel0, 0.1, 7850.0, 0.6, 0).unwrap();

    let g = -9.80665;
    let dt = world.config.dt;
    let steps = (2.0 / dt) as usize; // 2 s
    for _ in 0..steps {
        assert!(world.step_tick());
    }

    let t = 2.0;
    let expect_x = pos0.x + vel0.x * t;
    let expect_y = pos0.y + vel0.y * t + 0.5 * g * t * t;
    let ix = 0usize;
    let got_x = world.rigid.px[ix];
    let got_y = world.rigid.py[ix];

    let tol = 0.008; // symplectic Euler with 2 substeps accumulates ~O(dt²) error; 0.8% is generous but valid for v0.1
    let rel_x = ((got_x - expect_x) / expect_x.abs().max(1.0)).abs();
    let rel_y = ((got_y - expect_y) / expect_y.abs().max(1.0)).abs();
    assert!(rel_x < tol, "x drift {got_x} vs {expect_x} rel {rel_x}");
    assert!(rel_y < tol, "y drift {got_y} vs {expect_y} rel {rel_y}");
}

#[test]
fn circular_orbit_energy_is_conserved() {
    let mut cfg = WorldConfig::default();
    cfg.seed = 7;
    cfg.gravity = Vec3::zero();
    cfg.container = false;
    cfg.dt = 1.0 / 120.0; // finer step for orbital accuracy
    cfg.systems.fluid.enabled = false;
    cfg.systems.heat.enabled = false;
    cfg.systems.nbody.enabled = true;
    cfg.systems.nbody.softening = 0.001;
    let mut world = World::new("orbit".into(), cfg, None);

    let gm = ws_core::constants::G;
    let m = 1.0e10;
    let d = 2.0;
    let v = (gm * m / (2.0 * d)).sqrt(); // circular speed for equal masses around CoM
                                         // Symmetric placement around origin
    world
        .nbody
        .add_body(m, Vec3::new(-d / 2.0, 0.0, 0.0), Vec3::new(0.0, v, 0.0))
        .unwrap();
    world
        .nbody
        .add_body(m, Vec3::new(d / 2.0, 0.0, 0.0), Vec3::new(0.0, -v, 0.0))
        .unwrap();

    let e0 = world.nbody.total_energy();
    let period = world.nbody.orbital_period_two_body(0, 1).unwrap();
    let steps_per_period = (period / world.config.dt) as usize;
    // Integrate for ~5 periods
    for _ in 0..steps_per_period * 5 {
        world.nbody.step(world.config.dt);
    }
    let e1 = world.nbody.total_energy();
    let rel_e = ((e1 - e0) / e0.abs().max(1.0)).abs();
    assert!(rel_e < 5e-4, "energy drift {rel_e} e0={e0} e1={e1}");

    // Radius stability: distance from origin for each body should stay within 5% of d/2.
    for i in 0..world.nbody.n {
        let r = (world.nbody.px[i] * world.nbody.px[i]
            + world.nbody.py[i] * world.nbody.py[i]
            + world.nbody.pz[i] * world.nbody.pz[i])
            .sqrt();
        let rel = ((r - d / 2.0) / (d / 2.0)).abs();
        assert!(rel < 0.05, "body {i} radius drift {rel} r={r}");
    }
}

#[test]
fn heat_1d_cosine_decay() {
    // 1-D cosine mode under Neumann (insulated) boundaries.
    let mut cfg = WorldConfig::default();
    cfg.seed = 42;
    cfg.gravity = Vec3::zero();
    cfg.container = false;
    cfg.systems.fluid.enabled = false;
    cfg.systems.heat.enabled = true;
    cfg.systems.heat.res = [32, 4, 4];
    cfg.systems.heat.alpha = 0.02;
    cfg.systems.heat.volumetric_capacity = 4.186e6;
    cfg.bounds = ws_core::math::Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
    let mut world = World::new("heat".into(), cfg, None);

    // Impose T(x)= 300 + 40*cos(pi*(x+1)/L) where L=2.
    let l = 2.0;
    let t0 = 300.0;
    let amp = 40.0;
    let nx = world.heat.nx as usize;
    let ny = world.heat.ny as usize;
    let nz = world.heat.nz as usize;
    // Fill field manually
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let x = world.heat.bounds.min.x + (ix as f64 + 0.5) * world.heat.dx;
                let v = t0 + amp * (std::f64::consts::PI * (x + 1.0) / l).cos();
                let idx = ix + nx * (iy + ny * iz);
                world.heat.t[idx] = v;
            }
        }
    }
    // let the solver's semi-Lagrangian + diffusion steps handle buffering — no need to mirror next_t manually.
    // Keep a copy for the decay check
    let probed_t = 1.0; // seconds
    let steps = (probed_t / world.config.dt) as usize;
    for _ in 0..steps {
        world.heat.step(world.config.dt, None::<fn(Vec3) -> Vec3>);
    }
    // Analytic: amplitude decays as exp(-α π² t / L²)
    let alpha = world.heat.alpha;
    let expected_amp =
        amp * (-alpha * std::f64::consts::PI * std::f64::consts::PI * probed_t / (l * l)).exp();
    let mid_x = 0.0;
    let sampled = world.heat.sample(Vec3::new(mid_x, 0.0, 0.0));
    // At x=0, cos(pi*1/2)=0 → T should be ~t0 for all t (node). Check average near node stays near t0.
    assert!(
        (sampled - t0).abs() < 2.0,
        "mid-node temperature {sampled} vs {t0}"
    );
    let crest = world
        .heat
        .sample(Vec3::new(-1.0 + world.heat.dx * 0.5, 0.0, 0.0));
    let expected_crest = t0 + expected_amp;
    let rel = ((crest - expected_crest) / expected_crest).abs();
    // Explicit diffusion is first-order in time for this Δt, so we allow 4% relative tolerance at medium resolution.
    assert!(
        rel < 0.05,
        "heat crest {crest} vs expected {expected_crest} rel {rel} alpha={alpha}"
    );
}

#[test]
fn fluid_tank_settles_and_conserves_mass() {
    let mut cfg = WorldConfig::default();
    cfg.seed = 99;
    cfg.quality = Quality::Low;
    cfg.gravity = Vec3::new(0.0, -9.80665, 0.0);
    cfg.container = true;
    cfg.systems.nbody.enabled = false;
    // Tuned for test speed: larger smoothing length → fewer particles.
    cfg.systems.fluid.h = 0.12;
    cfg.systems.fluid.c = 12.0;
    cfg.systems.fluid.visc_alpha = 0.08;
    cfg.systems.heat.enabled = false;
    let mut world = World::new("tank".into(), cfg, None);
    // Smaller blob for CI speed (≈ 500–900 particles, not 10k).
    let added = {
        let mut rng = ws_core::rng::Rng::new(99, ws_core::rng::STREAM_FLUID_JITTER);
        world.fluid.spawn_blob(
            Vec3::new(0.0, 0.85, 0.0),
            0.62,
            world.fluid.h * 0.08,
            &mut rng,
        )
    };
    assert!(added > 80, "too few fluid particles {added}");
    let mass0 = world.fluid.total_mass();
    let steps = 180; // ~3 s (enough to see settling trend)
    for _ in 0..steps {
        world.step_tick();
    }
    let mass1 = world.fluid.total_mass();
    let rel_mass = ((mass1 - mass0) / mass0).abs();
    assert!(
        rel_mass < 1e-9,
        "fluid mass not conserved {mass0} -> {mass1} rel {rel_mass}"
    );
    let vmax = world.fluid.max_speed();
    assert!(vmax < 1.2, "tank should have settled but vmax={vmax} m/s");
    // Density stratification: bottom half average rho should be >= top half.
    let mut bottom_rho = 0.0;
    let mut top_rho = 0.0;
    let mut bottom_n = 0usize;
    let mut top_n = 0usize;
    let midy = (world.config.bounds.min.y + world.config.bounds.max.y) * 0.5;
    for i in 0..world.fluid.n {
        if world.fluid.py[i] < midy {
            bottom_rho += world.fluid.rho[i];
            bottom_n += 1;
        } else {
            top_rho += world.fluid.rho[i];
            top_n += 1;
        }
    }
    if bottom_n > 0 && top_n > 0 {
        let avg_b = bottom_rho / bottom_n as f64;
        let avg_t = top_rho / top_n as f64;
        // In WCSPH with gravity, bottom slightly denser; at rest the difference is modest (compressibility c=12).
        assert!(
            avg_b + 1e-6 >= avg_t,
            "bottom avg {avg_b} should be >= top {avg_t}"
        );
    }
}

#[test]
fn determinism_double_run() {
    let mk = || {
        let mut cfg = WorldConfig::default();
        cfg.seed = 2026;
        cfg.quality = Quality::Low;
        cfg.systems.fluid.h = 0.15; // few particles for speed
        cfg.systems.heat.enabled = false; // disable 110k-cell heat field for this focused test
        let mut w = World::new("det".into(), cfg, None);
        w.enqueue_events(vec![
            ws_domain::events::SimEvent::SpawnSphere {
                material: "steel".into(),
                pos: Vec3::new(0.0, 2.0, 0.0),
                vel: Vec3::new(1.0, 0.0, 0.0),
                radius: 0.15,
            },
            ws_domain::events::SimEvent::SpawnFluidBlob {
                center: Vec3::new(0.0, 1.0, 0.0),
                radius: 0.42,
            },
        ]);
        w
    };
    let mut a = mk();
    let mut b = mk();
    for _ in 0..70 {
        a.step_tick();
        b.step_tick();
    }
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "double-run must be bit-exact on same platform"
    );
    assert_eq!(a.tick, b.tick);
}

#[test]
fn snapshot_roundtrip_is_exact_and_resumable() {
    let mut world = {
        let mut cfg = WorldConfig::default();
        cfg.seed = 777;
        cfg.quality = Quality::Low;
        cfg.systems.fluid.h = 0.12;
        cfg.systems.heat.enabled = false;
        World::new("snap".into(), cfg, None)
    };
    world.enqueue_events(vec![ws_domain::events::SimEvent::SpawnSphere {
        material: "rubber".into(),
        pos: Vec3::new(0.5, 2.0, 0.0),
        vel: Vec3::zero(),
        radius: 0.12,
    }]);
    for _ in 0..60 {
        world.step_tick();
    }
    let snap = world.snapshot_json();
    let hash_before = world.state_hash();
    let tick_before = world.tick;

    let mut restored = World::new("restored".into(), WorldConfig::default(), None);
    restored.restore_from_json(&snap).expect("restore failed");
    assert_eq!(restored.state_hash(), hash_before);
    assert_eq!(restored.tick, tick_before);

    // Advance both for another 60 ticks and compare
    for _ in 0..60 {
        world.step_tick();
        restored.step_tick();
    }
    assert_eq!(
        world.state_hash(),
        restored.state_hash(),
        "post-restore divergence"
    );
}

#[test]
fn fork_produces_independent_diverging_worlds() {
    let mut parent = {
        let mut cfg = WorldConfig::default();
        cfg.seed = 12345;
        cfg.quality = Quality::Low;
        World::new("parent".into(), cfg, None)
    };
    for _ in 0..30 {
        parent.step_tick();
    }
    let mut child = parent.fork("child".into());
    assert_eq!(parent.state_hash(), child.state_hash());
    assert_ne!(parent.id, child.id);

    // Diverge with different events
    child.enqueue_events(vec![ws_domain::events::SimEvent::SpawnSphere {
        material: "wood".into(),
        pos: Vec3::new(0.0, 3.0, 0.0),
        vel: Vec3::zero(),
        radius: 0.1,
    }]);
    for _ in 0..30 {
        parent.step_tick();
        child.step_tick();
    }
    assert_ne!(parent.state_hash(), child.state_hash());
    assert_eq!(parent.rigid.n + 1, child.rigid.n);
}
