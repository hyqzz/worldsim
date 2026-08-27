#!/usr/bin/env python3
"""WorldSim Demo Runner — run all or specific demos.

Usage:
    python demos/run_all.py              # All demos
    python demos/run_all.py --demo 3     # Specific demo (buoyancy)
    python demos/run_all.py --list       # List demos
"""
import sys, os, subprocess, time, importlib.util

# Project paths
DEMO_DIR  = os.path.dirname(os.path.abspath(__file__))
ROOT_DIR  = os.path.dirname(DEMO_DIR)
SDK_DIR   = os.path.join(ROOT_DIR, "sdk", "python")
BIN_PATH  = os.path.join(ROOT_DIR, "target", "release", "worldsim.exe")
OUTPUT_DIR = os.path.join(DEMO_DIR, "output")

sys.path.insert(0, ROOT_DIR)
sys.path.insert(0, SDK_DIR)

DEMOS = {
    1:  ("rigid_bodies",   "刚体 — 自由落体弹跳碰撞"),
    2:  ("fluid_dam_break","SPH流体 — 水坝溃坝"),
    3:  ("buoyancy_demo",  "耦合 — 木浮钢沉"),
    4:  ("heat_diffusion", "热扩散 — 热点扩散"),
    5:  ("nbody_orbits",   "N体 — 轨道动力学"),
    6:  ("snapshot_fork",  "快照/分叉 — 平行宇宙"),
    7:  ("batch_training", "批量训练 — 64并行世界"),
    8:  ("qos_precision",  "QoS — Low/Med/High"),
    9:  ("wdl_scene",      "WDL — 声明式场景"),
    10: ("full_demo",      "全场景 — 水箱+球+热"),
}

def build():
    if os.path.exists(BIN_PATH):
        return
    print("Building worldsim...")
    subprocess.run(["cargo", "build", "-p", "worldsim", "--release"],
                   cwd=ROOT_DIR, check=True, capture_output=True)

def list_demos():
    print("Demos:")
    for num, (name, desc) in sorted(DEMOS.items()):
        print(f"  {num:2d}. {name:20s} — {desc}")

def run_demo(num, port=18099):
    name, desc = DEMOS[num]
    print(f"\n{'='*60}")
    print(f"  Demo {num}: {desc}")
    print(f"{'='*60}")

    fpath = os.path.join(DEMO_DIR, f"{name}.py")
    result = subprocess.run(
        [sys.executable, fpath, "--port", str(port)],
        capture_output=True, text=True, timeout=120,
        env={**os.environ, "PYTHONPATH": f"{ROOT_DIR}:{SDK_DIR}"}
    )
    print(result.stdout)
    if result.returncode != 0:
        print(f"  ✗ Error:\n{result.stderr[:500]}")
    return result.returncode == 0

def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--demo", type=int)
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--port", type=int, default=18099)
    args = parser.parse_args()

    if args.list:
        list_demos()
        return

    build()
    os.makedirs(OUTPUT_DIR, exist_ok=True)

    demos = [args.demo] if args.demo else list(range(1, len(DEMOS)+1))
    t0 = time.perf_counter()
    for num in demos:
        run_demo(num, args.port)
    elapsed = time.perf_counter() - t0
    print(f"\n{'='*60}")
    print(f"  Done in {elapsed:.0f}s — outputs in demos/output/")
    print(f"{'='*60}")

if __name__ == "__main__":
    main()
