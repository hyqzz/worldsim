#!/usr/bin/env python3
"""Demo 3: Buoyancy — wood floats, steel sinks."""
import sys, os, json, time, math
_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
_SDK  = os.path.join(_ROOT, "sdk", "python")
sys.path.insert(0, _ROOT); sys.path.insert(0, _SDK)

def run_demo(port=18099):
    from viz import ensure_server, WorldPlotter, OUTPUT_DIR
    from PIL import Image
    import numpy as np

    print("=== Demo 3: Buoyancy ===")
    c = ensure_server(port)

    w = c.create_world(name="buoyancy", config={
        "quality": "low", "gravity": [0, -9.81, 0],
        "bounds": {"min": [-2, 0, -1], "max": [2, 4, 1]},
        "container": True,
        "systems": {"fluid": {"enabled": True, "h": 0.09, "rho0": 1000, "c": 12},
                    "heat": {"enabled": False}, "nbody": {"enabled": False}},
    })
    wid = w["id"]
    c.inject(wid, [
        {"type": "spawn_fluid", "center": [0, 0.85, 0], "radius": 0.75},
        {"type": "spawn_sphere", "material": "wood", "pos": [-0.8, 2.8, 0], "radius": 0.15},
        {"type": "spawn_sphere", "material": "steel", "pos": [0.0, 2.8, 0], "radius": 0.12},
        {"type": "spawn_sphere", "material": "copper", "pos": [0.8, 2.8, 0], "radius": 0.10},
    ])
    print("  Spawned: water + wood(700) + steel(7850) + copper(8960)")

    frames = []
    for step in range(300):
        c.control(wid, "step")
        if step % 6 == 0:
            pw = WorldPlotter(title=f"Buoyancy  t={step/60:.2f}s",
                              bounds=((-2, 0), (2, 4)))
            pw.ax.axhline(y=0, color="#2a3f6b", linewidth=2)
            pw.ax.text(0.02, 0.97,
                "wood(700)→floats\nsteel(7850)→sinks\ncopper(8960)→sinks",
                transform=pw.ax.transAxes, color="#8ea2c8", fontsize=9,
                va="top", fontfamily="monospace",
                bbox=dict(boxstyle="round", facecolor="#0f1a30", edgecolor="#1f2a44"))
            fp = OUTPUT_DIR / f"_buoy_{step:04d}.png"
            pw.save(fp); frames.append(fp); pw.close()

    if frames:
        imgs = [Image.open(str(f)) for f in frames]
        imgs[0].save(str(OUTPUT_DIR / "buoyancy.gif"),
                     save_all=True, append_images=imgs[1:],
                     duration=100, loop=0)
        print(f"  ✓ Saved: {OUTPUT_DIR}/buoyancy.gif")
        for f in frames: os.remove(f)
    c.control(wid, "pause")

if __name__ == "__main__":
    import argparse
    p = argparse.ArgumentParser()
    p.add_argument("--port", type=int, default=18099)
    run_demo(p.parse_args().port)
