#!/usr/bin/env python3
"""Demo 1: rigid-body dynamics — real positions rendered as an animated GIF."""
import sys
import os

_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, _ROOT)
sys.path.insert(0, os.path.join(_ROOT, "sdk", "python"))


def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    from PIL import Image
    import numpy as np

    print("=== Demo 1: rigid-body dynamics ===")
    client = ensure_server(port)
    created = client.create_world(name="rigid_demo", config={
        "auto_start": False, "quality": "high", "gravity": [0, -9.81, 0],
        "bounds": {"min": [-3, 0, -1], "max": [3, 5, 1]}, "container": True,
        "systems": {"fluid": {"enabled": False}, "heat": {"enabled": False}, "nbody": {"enabled": False}},
    })
    wid = created["id"]
    client.inject(wid, [
        {"type": "spawn_sphere", "material": "steel", "pos": [0, 4.0, 0], "vel": [0, 0, 0], "radius": 0.18},
        {"type": "spawn_sphere", "material": "wood", "pos": [1.5, 3.5, 0], "vel": [0, 0, 0], "radius": 0.15},
        {"type": "spawn_sphere", "material": "rubber", "pos": [-1.5, 3.0, 0], "vel": [1, 0, 0], "radius": 0.12},
    ])
    client.control(wid, "flush")
    frames = []
    for step in range(180):
        payload = client.step_sync(wid, 1)["observation"]
        bodies = payload["bodies"]
        px, py = client.decode_f64_array(bodies["px"]), client.decode_f64_array(bodies["py"])
        radius = client.decode_f64_array(bodies["r"])
        import base64
        mat = np.frombuffer(base64.b64decode(bodies["mat_id"]), dtype=np.uint8)
        if step % 3 == 0:
            plot = WorldPlotter(f"Rigid bodies  t={payload['time']:.2f}s", ((-3, 0), (3, 5)))
            plot.ax.axhline(0, color="#2a3f6b", linewidth=2)
            plot.plot_bodies(px, py, radius, mat)
            plot.add_text("symplectic Euler\nimpulse contact + Coulomb friction\nsleep/wake enabled")
            path = OUTPUT_DIR / f"_rigid_{step:04d}.png"
            plot.save(path); frames.append(path); plot.close()
    if frames:
        images = [Image.open(str(p)) for p in frames]
        out = OUTPUT_DIR / "rigid_bodies.gif"
        images[0].save(str(out), save_all=True, append_images=images[1:], duration=90, loop=0)
        for p in frames: os.remove(p)
        print(f"  ✓ Saved: {out} ({len(images)} frames)")
    print("  Result: real server state, 3D coordinates projected to a documented 2D slice")
    client.delete_world(wid)


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18099)
    run_demo(parser.parse_args().port)
