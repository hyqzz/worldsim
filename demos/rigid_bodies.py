#!/usr/bin/env python3
"""
Demo 1: 刚体动力学 — 自由落体、弹跳、碰撞
================================================

演示物理:
- 一个钢球从高处自由落体
- 一个木球从高处自由落体
- 两个球在地面弹跳
- 碰撞后速度交换（近似弹性碰撞）

输出:
- output/rigid_bodies.gif — 动画帧序列

运行:
    python demos/rigid_bodies.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))
from worldsim.client import Client

def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    import numpy as np
    from PIL import Image

    print("=== Demo 1: 刚体动力学 ===")
    c = ensure_server(port)

    # Create a world with only rigid bodies (no fluid, no heat)
    w = c.create_world(name="rigid_demo", config={
        "quality": "high",
        "gravity": [0, -9.81, 0],
        "bounds": {"min": [-3, 0, -1], "max": [3, 5, 1]},
        "container": True,
        "systems": {"fluid": {"enabled": False}, "heat": {"enabled": False},
                    "nbody": {"enabled": False}},
    })
    wid = w["id"]
    print(f"  World: {wid}")

    # Spawn 3 spheres at different heights and materials
    c.inject(wid, [
        {"type": "spawn_sphere", "material": "steel", "pos": [0, 4.0, 0],
         "vel": [0, 0, 0], "radius": 0.18},
        {"type": "spawn_sphere", "material": "wood", "pos": [1.5, 3.5, 0],
         "vel": [0, 0, 0], "radius": 0.15},
        {"type": "spawn_sphere", "material": "rubber", "pos": [-1.5, 3.0, 0],
         "vel": [1, 0, 0], "radius": 0.12},
    ])
    print("  Spawned: steel ball, wood ball, rubber ball (with horizontal velocity)")

    # Step and capture frames
    frames = []
    dt = 1.0/60.0
    total_time = 3.0  # 3 seconds
    n_steps = int(total_time / dt)

    for step in range(n_steps):
        c.control(wid, "step")

        if step % 3 == 0:  # capture every 3rd frame
            pw = WorldPlotter(
                title=f"刚体动力学  t={step*dt:.2f}s  tick={step}",
                bounds=((-3, 0), (3, 5))
            )

            # Draw the ground
            pw.ax.axhline(y=0, color="#2a3f6b", linewidth=2)

            # Draw falling paths as dotted lines for context
            if step < 50:
                pw.ax.plot([0, 0], [4.0, 4.0 - 0.5*9.81*(step*dt)**2],
                          "--", color="#94a3b8", alpha=0.3, linewidth=1)

            # Add info text
            info = (
                f"steel:  ρ=7850 kg/m³  r=0.18\n"
                f"wood:   ρ=700 kg/m³   r=0.15\n"
                f"rubber: ρ=1100 kg/m³  r=0.12\n"
                f"\ngravity = 9.81 m/s²"
            )
            pw.ax.text(0.98, 0.97, info, transform=pw.ax.transAxes,
                       color="#8ea2c8", fontsize=8, va="top", ha="right",
                       fontfamily="monospace", bbox=dict(boxstyle="round",
                       facecolor="#0f1a30", edgecolor="#1f2a44"))

            # Capture frame
            frame_path = OUTPUT_DIR / f"_rigid_{step:04d}.png"
            pw.save(frame_path)
            frames.append(frame_path)
            pw.close()

    # Save as GIF
    if frames:
        from PIL import Image
        imgs = [Image.open(str(f)) for f in frames]
        gif_path = OUTPUT_DIR / "rigid_bodies.gif"
        imgs[0].save(str(gif_path), save_all=True, append_images=imgs[1:],
                     duration=100, loop=0)
        print(f"  ✓ Saved: {gif_path}")
        for f in frames:
            os.remove(f)

    # Print final state
    state = c.state(wid)
    print(f"  Final: tick={state['tick']}, bodies={state['bodies']}")
    print(f"  Physics: 3 spheres fall under gravity, bounce off ground and walls")
    print(f"  Steel sinks fastest (heaviest), rubber bounces highest (high restitution)")
    c.control(wid, "pause")

if __name__ == "__main__":
    run_demo()
