#!/usr/bin/env python3
"""
Demo 6: 快照与分叉 — 平行宇宙实验
================================================

演示功能:
- 创建一个世界，让它演化一段时间
- 打一个快照（保存此刻的完整状态）
- 分叉成两个平行世界
- 在其中一个注入额外事件（让它们发散）
- 对比两个世界的最终状态

输出:
- output/snapshot_fork.png — 对比图

运行:
    python demos/snapshot_fork.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))

def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    import numpy as np
    from PIL import Image
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    print("=== Demo 6: 快照与分叉 — 平行宇宙 ===")
    c = ensure_server(port)

    w = c.create_world(name="fork_parent", config={
        "auto_start": False,
        "quality": "low",
        "gravity": [0, -9.81, 0],
        "bounds": {"min": [-2, 0, -1], "max": [2, 4, 1]},
        "container": True,
        "systems": {"fluid": {"enabled": True, "h": 0.1, "rho0": 1000, "c": 12},
                    "heat": {"enabled": False}},
    })
    wid = w["id"]
    print(f"  Parent world: {wid}")

    # Create initial scene
    c.inject(wid, [
        {"type": "spawn_fluid", "center": [0, 0.8, 0], "radius": 0.6},
        {"type": "spawn_sphere", "material": "wood", "pos": [0, 2.5, 0], "radius": 0.15},
    ])
    print("  Spawned: water + wood ball")

    # Step parent for a while
    for _ in range(60):
        c.control(wid, "step")
    parent_state = c.state(wid)
    parent_hash = parent_state["hash"]
    print(f"  Parent after 60 ticks: hash={parent_hash[:12]}")

    # Take a snapshot
    snap = c.snapshot_create(wid)
    snap_id = snap["snapshot_id"]
    print(f"  Snapshot: {snap_id}")

    # Fork
    fork = c.fork(wid, name="forked_world")
    child_id = fork["id"]
    child_state = c.state(child_id)
    child_hash = child_state["hash"]
    print(f"  Child: {child_id}")
    print(f"  Same hash? {parent_hash == child_hash}")  # should be True right after fork

    # Diverge: add a steel ball to child only
    c.inject(child_id, [
        {"type": "spawn_sphere", "material": "steel", "pos": [0.5, 3.0, 0],
         "vel": [0, -2, 0], "radius": 0.1},
    ])
    print("  Child gets an extra steel ball → worlds will diverge")

    # Step both
    for _ in range(60):
        c.control(wid, "step")
        c.control(child_id, "step")

    parent_final = c.state(wid)
    child_final = c.state(child_id)
    print(f"\n  After divergence:")
    print(f"  Parent: tick={parent_final['tick']}, hash={parent_final['hash'][:12]}")
    print(f"  Child:  tick={child_final['tick']}, hash={child_final['hash'][:12]}")
    print(f"  Hashes differ? {parent_final['hash'] != child_final['hash']}")

    # Create visualization
    fig, axes = plt.subplots(1, 2, figsize=(14, 6), facecolor="#0b0f17")
    for ax in axes:
        ax.set_facecolor("#0b0f17")
        ax.set_aspect("equal")
        ax.set_title("Parallel Universes", color="#e6eef8", fontsize=14, fontweight="bold")
        ax.set_xlabel("x", color="#8ea2c8")
        ax.set_ylabel("y", color="#8ea2c8")

    axes[0].set_title(f"Parent: tick={parent_final['tick']}", color="#7aa1ff", fontsize=12)
    axes[1].set_title(f"Child: tick={child_final['tick']}", color="#ff6b6b", fontsize=12)

    # Info boxes
    info_parent = (
        f"Parent: {parent_final['hash'][:12]}\n"
        f"bodies: {parent_final['bodies']}\n"
        f"fluid:  {parent_final['fluid']}\n"
        f"tick:   {parent_final['tick']}\n"
        f"\nNo extra events"
    )
    info_child = (
        f"Child: {child_final['hash'][:12]}\n"
        f"bodies: {child_final['bodies']}\n"
        f"fluid:  {child_final['fluid']}\n"
        f"tick:   {child_final['tick']}\n"
        f"\n+1 steel ball"
    )
    axes[0].text(0.02, 0.97, info_parent, transform=axes[0].transAxes,
                color="#8ea2c8", fontsize=8, va="top", fontfamily="monospace",
                bbox=dict(boxstyle="round", facecolor="#0f1a30", edgecolor="#1f2a44"))
    axes[1].text(0.02, 0.97, info_child, transform=axes[1].transAxes,
                color="#8ea2c8", fontsize=8, va="top", fontfamily="monospace",
                bbox=dict(boxstyle="round", facecolor="#0f1a30", edgecolor="#1f2a44"))

    # Draw bounding boxes
    for ax in axes:
        from matplotlib.patches import Rectangle
        rect = Rectangle((-2, 0), 4, 4, linewidth=1.5, edgecolor="#2a3f6b", facecolor="none")
        ax.add_patch(rect)
        ax.set_xlim(-2.5, 2.5)
        ax.set_ylim(-0.5, 4.5)
        ax.grid(True, alpha=0.1, color="#2a3f6b")

    fig.suptitle("Snapshot → Fork → Diverge  (same seed, different events)",
                color="#8ea2c8", fontsize=11, y=0.02)

    path = OUTPUT_DIR / "snapshot_fork.png"
    fig.savefig(str(path), dpi=120, bbox_inches="tight", facecolor="#0b0f17")
    print(f"\n  ✓ Saved: {path}")
    plt.close(fig)

    # Cleanup
    c.delete_world(wid)
    c.delete_world(child_id)

if __name__ == "__main__":
    import argparse
    parser=argparse.ArgumentParser()
    parser.add_argument("--port",type=int,default=18099)
    run_demo(parser.parse_args().port)
