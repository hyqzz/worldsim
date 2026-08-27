#!/usr/bin/env python3
"""
Demo 8: QoS 精度控制 — Low/Medium/High 对比
================================================

演示功能:
- 同一个水箱场景在三种精度等级下的表现
- 每种等级的流体粒子数、子步数、流传输步幅

输出:
- output/qos_precision.png — 对比图

运行:
    python demos/qos_precision.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))

def run_demo(port=18099):
    from demos.viz import ensure_server, OUTPUT_DIR

    print("=== Demo 8: QoS 精度控制 ===")
    c = ensure_server(port)

    levels = [
        ("low",    "Low",    "#ff6b6b"),
        ("medium", "Medium", "#ffa500"),
        ("high",   "High",   "#4f7cff"),
    ]

    results = []

    for quality, label, color in levels:
        print(f"\n  Testing {label} quality...")
        w = c.create_world(name=f"qos_{quality}", config={
            "auto_start": False,
            "quality": quality,
            "gravity": [0, -9.81, 0],
            "bounds": {"min": [-2, 0, -1], "max": [2, 4, 1]},
            "container": True,
            "systems": {"fluid": {"enabled": True, "h": 0.08},
                        "heat": {"enabled": False}},
        })
        wid = w["id"]

        # Inject fluid
        c.inject(wid, [
            {"type": "spawn_fluid", "center": [0, 0.8, 0], "radius": 0.5},
        ])

        # Step for 3 seconds and measure
        t0 = time.perf_counter()
        for _ in range(180):  # 3 seconds at 60Hz
            c.control(wid, "step")
        t_total = time.perf_counter() - t0

        state = c.state(wid)
        results.append({
            "quality": quality,
            "label": label,
            "color": color,
            "tick": state["tick"],
            "fluid": state["fluid"],
            "time_ms": t_total * 1000,
        })
        print(f"    {label}: tick={state['tick']}, fluid={state['fluid']}, time={t_total*1000:.0f}ms")
        c.delete_world(wid)

    # Create comparison visualization
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    fig, axes = plt.subplots(1, 3, figsize=(15, 5), facecolor="#0b0f17")

    titles = ["Execution Time", "Fluid Particles", "Tick Rate"]
    keys = ["time_ms", "fluid", None]
    colors = [r["color"] for r in results]
    labels = [r["label"] for r in results]

    for ax in axes:
        ax.set_facecolor("#0f1a30")
        for spine in ax.spines.values():
            spine.set_color("#1f2a44")
        ax.tick_params(colors="#8ea2c8")

    # Time comparison
    ax = axes[0]
    vals = [r["time_ms"] for r in results]
    bars = ax.bar(labels, vals, color=colors, alpha=0.8)
    ax.set_title("3s Simulation Wall Time", color="#e6eef8", fontsize=11)
    ax.set_ylabel("Time (ms)", color="#8ea2c8")
    for bar, v in zip(bars, vals):
        ax.text(bar.get_x() + bar.get_width()/2, v + 10, f"{v:.0f}ms",
               ha="center", color="#e6eef8", fontsize=9)

    # Fluid particles
    ax = axes[1]
    vals = [r["fluid"] for r in results]
    bars = ax.bar(labels, vals, color=colors, alpha=0.8)
    ax.set_title("Fluid Particles (capped by QoS)", color="#e6eef8", fontsize=11)
    ax.set_ylabel("Particles", color="#8ea2c8")
    for bar, v in zip(bars, vals):
        ax.text(bar.get_x() + bar.get_width()/2, v + 50, str(v),
               ha="center", color="#e6eef8", fontsize=9)

    # Tick rate
    ax = axes[2]
    vals = [1000 * r["tick"] / max(r["time_ms"], 1) for r in results]
    bars = ax.bar(labels, vals, color=colors, alpha=0.8)
    ax.set_title("Tick Rate", color="#e6eef8", fontsize=11)
    ax.set_ylabel("Ticks/second", color="#8ea2c8")
    for bar, v in zip(bars, vals):
        ax.text(bar.get_x() + bar.get_width()/2, v + 1, f"{v:.0f}",
               ha="center", color="#e6eef8", fontsize=9)

    # Legend text
    legend_text = (
        "QoS levels:\n"
        "  Low:    4k particles, 4 substeps, stride=4\n"
        "  Medium: 16k particles, 10 substeps, stride=2\n"
        "  High:   48k particles, 24 substeps, stride=1\n"
        "\nAuto controller: PID-free hysteresis\n"
        "60 ticks over → down, 600 under → up"
    )
    fig.text(0.98, 0.97, legend_text, color="#8ea2c8", fontsize=8,
            va="top", ha="right", fontfamily="monospace",
            bbox=dict(boxstyle="round", facecolor="#0f1a30", edgecolor="#1f2a44"))

    fig.suptitle("QoS Precision Control — Same Scene, Different Accuracy",
                color="#8ea2c8", fontsize=13, y=0.02)

    path = OUTPUT_DIR / "qos_precision.png"
    fig.savefig(str(path), dpi=120, bbox_inches="tight", facecolor="#0b0f17")
    print(f"\n  ✓ Saved: {path}")
    plt.close(fig)

if __name__ == "__main__":
    import argparse
    parser=argparse.ArgumentParser()
    parser.add_argument("--port",type=int,default=18099)
    run_demo(parser.parse_args().port)
