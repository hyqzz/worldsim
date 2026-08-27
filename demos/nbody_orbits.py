#!/usr/bin/env python3
"""Demo 5: N-body gravity — a real two-body circular orbit.

This demo uses the public ``spawn_nbody`` event and the synchronous ``step``
API, then plots the actual positions returned by ``observation.nbody``.
"""
import sys, os, json, time, math, base64
_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, _ROOT)
sys.path.insert(0, os.path.join(_ROOT, "sdk", "python"))


def decode_f64(value):
    import numpy as np
    if not value:
        return np.empty(0, dtype=np.float64)
    return np.frombuffer(base64.b64decode(value), dtype="<f8").copy()


def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    import numpy as np
    from PIL import Image

    print("=== Demo 5: N-body gravity — two-body orbit ===")
    c = ensure_server(port)
    w = c.create_world(name="nbody_orbit", config={
        "quality": "high",
        "auto_start": False,
        "dt": 1.0 / 120.0,
        "gravity": [0, 0, 0],
        "bounds": {"min": [-4, -4, -1], "max": [4, 4, 1]},
        "container": False,
        "systems": {
            "fluid": {"enabled": False},
            "heat": {"enabled": False},
            "nbody": {"enabled": True, "softening": 0.001},
        },
    })
    wid = w["id"]
    mass = 1.0e10
    separation = 2.0
    g = 6.67430e-11
    speed = math.sqrt(g * mass / (2.0 * separation))
    c.inject(wid, [
        {"type": "spawn_nbody", "mass": mass, "pos": [-1.0, 0, 0], "vel": [0, speed, 0]},
        {"type": "spawn_nbody", "mass": mass, "pos": [1.0, 0, 0], "vel": [0, -speed, 0]},
    ])
    c.control(wid, "flush")
    print(f"  World: {wid}")
    print(f"  Bodies: 2 × {mass:.2e} kg, separation={separation} m, v={speed:.4f} m/s")

    frames = []
    trail = [[], []]
    # 12 seconds, about one orbit for this normalized setup.
    for step in range(1440):
        result = c.step_sync(wid, ticks=1)
        obs = result["observation"]["nbody"]
        px, py = decode_f64(obs["px"]), decode_f64(obs["py"])
        if step % 8 == 0 and len(px) >= 2:
            for i in range(2):
                trail[i].append((float(px[i]), float(py[i])))
            pw = WorldPlotter(title=f"N-body orbit  t={step/120:.2f}s",
                              bounds=((-4, -4), (4, 4)))
            for i, color in enumerate(("#4f7cff", "#ff6b6b")):
                if len(trail[i]) > 1:
                    a = np.asarray(trail[i][-220:])
                    pw.ax.plot(a[:, 0], a[:, 1], color=color, alpha=0.65, linewidth=1.2)
                pw.ax.scatter([px[i]], [py[i]], s=90, c=color, edgecolors="white", linewidths=0.8)
            pw.ax.scatter([0], [0], marker="+", c="#e6eef8", s=55)
            pw.ax.set_xlabel("x (m)", color="#8ea2c8")
            pw.ax.set_ylabel("y (m)", color="#8ea2c8")
            pw.add_text("softened Newtonian force\nleapfrog KDK\nE drift target < 5e-4")
            path = OUTPUT_DIR / f"_orbit_{step:04d}.png"
            pw.save(path); frames.append(path); pw.close()

    if frames:
        imgs = [Image.open(str(f)) for f in frames]
        out = OUTPUT_DIR / "orbits.gif"
        imgs[0].save(str(out), save_all=True, append_images=imgs[1:], duration=70, loop=0)
        print(f"  ✓ Saved: {out} ({len(imgs)} frames)")
        for f in frames:
            os.remove(f)
    print("  Physics: G·m₁m₂/r², Plummer softening, leapfrog KDK")
    c.delete_world(wid)


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18099)
    run_demo(parser.parse_args().port)
