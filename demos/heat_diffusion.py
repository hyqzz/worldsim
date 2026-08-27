#!/usr/bin/env python3
"""
Demo 4: 热扩散 — 从热点到全域的温度均衡
================================================

演示物理:
- 一个高温热源放置在左侧
- 热量通过扩散方程 ∂T/∂t = α∇²T 向四周传播
- 温度场的三线性插值可视化

输出:
- output/heat_diffusion.gif — 动画帧序列

运行:
    python demos/heat_diffusion.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))

def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    import numpy as np
    from PIL import Image

    print("=== Demo 4: 热扩散 ===")
    c = ensure_server(port)

    w = c.create_world(name="heat", config={
        "quality": "low",
        "gravity": [0, -9.81, 0],
        "bounds": {"min": [-2, 0, -1], "max": [2, 4, 1]},
        "container": True,
        "systems": {"fluid": {"enabled": False}, "heat": {"enabled": True, "res": [16, 16, 16]},
                    "nbody": {"enabled": False}},
    })
    wid = w["id"]
    print(f"  World: {wid}")

    # Place a heat source
    c.inject(wid, [
        {"type": "heat_source", "pos": [-1.5, 2.0, 0], "radius": 0.5,
         "power_watts": 5000, "duration_ticks": 0},
    ])
    print("  Spawned: heat source at (-1.5, 2.0) — 5000W persistent")

    # Step and capture frames showing temperature field
    frames = []
    dt = 1.0/60.0
    total_time = 6.0  # 6 seconds for diffusion to spread
    n_steps = int(total_time / dt)

    # Store previous temps for heatmap
    prev_temps = None

    for step in range(n_steps):
        c.control(wid, "step")

        if step % 6 == 0:  # every 6th frame for heatmap
            # Probe temperature at a grid of points
            nx, ny = 20, 16
            T_grid = np.zeros((ny, nx))
            x_range = np.linspace(-1.8, 1.8, nx)
            y_range = np.linspace(0.2, 3.8, ny)

            for j, y in enumerate(y_range):
                for i, x in enumerate(x_range):
                    try:
                        t = c.probe(wid, float(x), float(y), 0.0, quantity="temperature")
                        T_grid[j, i] = t.get("value", 293.15)
                    except Exception:
                        T_grid[j, i] = 293.15

            pw = WorldPlotter(
                title=f"热扩散  t={step*dt:.2f}s  α=1.43×10⁻⁷ m²/s",
                bounds=((-2, 0), (2, 4))
            )
            pw.ax.axhline(y=0, color="#2a3f6b", linewidth=2)

            # Draw heatmap
            extent = [-2, 2, 0, 4]
            T_display = T_grid - 293.15  # Show temperature above ambient
            T_max = max(T_display.max(), 1)
            im = pw.ax.imshow(T_display, origin="lower", extent=extent,
                             cmap="inferno", vmin=0, vmax=T_max * 0.8,
                             alpha=0.8, aspect="auto")

            # Add heatmap legend
            cbar = pw.fig.colorbar(im, ax=pw.ax, shrink=0.6, label="ΔT (K above 293K)")

            # Mark heat source
            pw.ax.plot(-1.5, 2.0, "o", color="#ff4444", markersize=10, alpha=0.8)
            pw.ax.text(-1.5, 2.2, "5000W", color="#ff4444", fontsize=8, ha="center")

            # Info
            T_hot = T_grid[8, 2]  # Near the heat source
            T_far = T_grid[8, 18]  # Far from heat source
            info = (
                f"T near source: {T_hot:.1f}K\n"
                f"T far away:    {T_far:.1f}K\n"
                f"ΔT ratio:      {(T_hot-293)/(T_far-293+0.01):.1f}x\n"
                f"Analytic: T ∝ exp(-απ²t/L²)"
            )
            pw.ax.text(0.98, 0.97, info, transform=pw.ax.transAxes,
                      color="#8ea2c8", fontsize=8, va="top", ha="right",
                      fontfamily="monospace",
                      bbox=dict(boxstyle="round", facecolor="#0f1a30",
                               edgecolor="#1f2a44"))

            frame_path = OUTPUT_DIR / f"_heat_{step:04d}.png"
            pw.save(frame_path)
            frames.append(frame_path)
            pw.close()

    if frames:
        imgs = [Image.open(str(f)) for f in frames]
        gif_path = OUTPUT_DIR / "heat_diffusion.gif"
        imgs[0].save(str(gif_path), save_all=True, append_images=imgs[1:],
                     duration=200, loop=0)
        print(f"  ✓ Saved: {gif_path}")
        for f in frames:
            os.remove(f)

    print(f"  Physics: Explicit diffusion + Neumann boundary conditions")
    print(f"  Heat source (5000W) drives temperature gradient across the domain")
    print(f"  Verification: analytic cosine decay < 5% (see validation tests)")
    c.control(wid, "pause")

if __name__ == "__main__":
    run_demo()
