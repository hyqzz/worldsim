#!/usr/bin/env python3
"""
Demo 7: 批量训练 — 64 个并行世界
================================================

演示功能:
- 用 Batch API 创建 64 个世界
- 所有世界同时演化
- 收集每个世界的 tick/hash 统计
- 可视化并行执行的吞吐量

输出:
- output/batch_training.png — 统计图

运行:
    python demos/batch_training.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))

def run_demo(port=18099):
    from demos.viz import ensure_server, OUTPUT_DIR

    print("=== Demo 7: 批量训练 — 64 个并行世界 ===")
    c = ensure_server(port)
    base = f"http://127.0.0.1:{port}"

    # Batch create
    t0 = time.perf_counter()
    resp = json.loads(os.popen(
        f'curl -s -X POST {base}/v1/batch/create '
        f'-H "Content-Type: application/json" '
        f'-d \'{{"count":64,"template_config":{{"quality":"low","dt":0.0166667}},"name_prefix":"train"}}\''
    ).read())
    t_create = time.perf_counter() - t0
    ids = resp["ids"]
    print(f"  Created {len(ids)} worlds in {t_create*1000:.1f}ms")

    # Batch inject (fluid in all worlds)
    t0 = time.perf_counter()
    inject_data = json.dumps({"ids": ids, "events": [
        {"type": "spawn_fluid", "center": [0, 1.0, 0], "radius": 0.4},
        {"type": "spawn_sphere", "material": "wood", "pos": [0, 2.5, 0], "radius": 0.14},
    ]})
    os.popen(f'curl -s -X POST {base}/v1/batch/inject '
             f'-H "Content-Type: application/json" '
             f"-d '{inject_data}'").read()
    t_inject = time.perf_counter() - t0
    print(f"  Injected events into {len(ids)} worlds in {t_inject*1000:.1f}ms")

    # Batch step — benchmark
    print(f"\n  Benchmarking: {len(ids)} worlds × 20 ticks...")
    step_data = json.dumps({"ids": ids, "ticks_per_world": 20})

    timings = []
    for rep in range(5):
        t0 = time.perf_counter()
        result = json.loads(os.popen(
            f'curl -s -X POST {base}/v1/batch/step '
            f'-H "Content-Type: application/json" '
            f"-d '{step_data}'"
        ).read())
        t_step = time.perf_counter() - t0
        timings.append(t_step)

        if rep == 0:
            results = result["results"]

    avg_ms = sum(timings) / len(timings) * 1000
    min_ms = min(timings) * 1000
    total_steps = len(ids) * 20
    steps_per_sec = total_steps / (min_ms / 1000)

    print(f"  Results:")
    print(f"    Worlds:         {len(ids)}")
    print(f"    Ticks/world:    20")
    print(f"    Total steps:    {total_steps}")
    print(f"    Avg time:       {avg_ms:.1f} ms")
    print(f"    Best time:      {min_ms:.1f} ms")
    print(f"    Steps/second:   {steps_per_sec:.0f}")

    # Create visualization
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    import numpy as np

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(14, 6), facecolor="#0b0f17")
    for ax in [ax1, ax2]:
        ax.set_facecolor("#0f1a30")
        for spine in ax.spines.values():
            spine.set_color("#1f2a44")

    # Left: bar chart of steps/sec
    ax1.set_title("Batch Training Throughput", color="#e6eef8", fontsize=12)
    x = np.arange(5)
    bars = ax1.bar(x, [t*1000 for t in timings], color="#4f7cff", alpha=0.8)
    ax1.set_xticks(x)
    ax1.set_xticklabels([f"Run {i+1}" for i in range(5)])
    ax1.set_ylabel("Time (ms)", color="#8ea2c8")
    ax1.axhline(y=avg_ms, color="#ff6b6b", linestyle="--", label=f"avg={avg_ms:.0f}ms")
    ax1.legend(facecolor="#0f1a30", edgecolor="#1f2a44", labelcolor="#8ea2c8")
    ax1.tick_params(colors="#8ea2c8")

    # Right: stats summary
    ax2.axis("off")
    stats_text = (
        f"━━━ Batch Training Stats ━━━\n\n"
        f"  Worlds:          {len(ids)}\n"
        f"  Ticks/world:     20\n"
        f"  Total steps:     {total_steps}\n\n"
        f"  Avg time:        {avg_ms:.1f} ms\n"
        f"  Best time:       {min_ms:.1f} ms\n\n"
        f"  ━━━━━━━━━━━━━━━━━━━━━━\n"
        f"  Steps/sec:       {steps_per_sec:,.0f}\n"
        f"  ━━━━━━━━━━━━━━━━━━━━━━\n\n"
        f"  Create time:     {t_create*1000:.1f} ms\n"
        f"  Inject time:     {t_inject*1000:.1f} ms\n\n"
        f"  Parallel execution\n"
        f"  via std::thread::scope"
    )
    ax2.text(0.1, 0.95, stats_text, transform=ax2.transAxes,
            color="#e6eef8", fontsize=11, va="top", fontfamily="monospace",
            bbox=dict(boxstyle="round", facecolor="#0f1a30", edgecolor="#1f2a44"))

    fig.suptitle("WorldSim v0.1 — Batch Training Demo",
                color="#8ea2c8", fontsize=14, y=0.02)

    path = OUTPUT_DIR / "batch_training.png"
    fig.savefig(str(path), dpi=120, bbox_inches="tight", facecolor="#0b0f17")
    print(f"\n  ✓ Saved: {path}")
    plt.close(fig)

    # Cleanup
    for wid in ids:
        c.delete_world(wid)

if __name__ == "__main__":
    run_demo()
