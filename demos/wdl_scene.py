#!/usr/bin/env python3
"""
Demo 9: WDL — 声明式场景描述
================================================

演示功能:
- 用 JSON (WDL 格式) 描述一个完整世界
- 解析 WDL → 自动创建世界和实体
- 展示 WDL 语法

输出:
- output/wdl_scene.png — WDL 语法展示 + 解析结果

运行:
    python demos/wdl_scene.py
"""
import sys, os, json, time
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "sdk", "python"))

def run_demo(port=18099):
    from demos.viz import ensure_server, OUTPUT_DIR

    print("=== Demo 9: WDL 声明式场景描述 ===")
    c = ensure_server(port)
    base = f"http://127.0.0.1:{port}"

    # Define a scene in WDL format
    wdl_doc = {
        "world": {
            "seed": 42,
            "quality": "medium",
            "bounds": {"min": [-2, 0, -2], "max": [2, 4, 2]},
            "gravity": [0, -9.81, 0],
            "container": True
        },
        "entities": [
            {"pool": {"material": "water", "shape": "fluid", "at": [0, 0.8, 0], "radius": 0.6}},
            {"wood_ball": {"material": "wood", "shape": "sphere[r=0.15]", "at": [0.5, 2.5, 0]}},
            {"steel_ball": {"material": "steel", "shape": "sphere[r=0.12]", "at": [-0.5, 2.5, 0]}},
        ],
        "laws": ["gravity", "navier-stokes", "heat-transfer"]
    }
    print(f"\n  WDL document:")
    print(json.dumps(wdl_doc, indent=2)[:500] + "...")

    # Parse via API
    result = json.loads(os.popen(
        f'curl -s -X POST {base}/v1/wdl/parse '
        f'-H "Content-Type: application/json" '
        f"-d '{json.dumps({\"wdl\": json.dumps(wdl_doc)})}'"
    ).read())

    print(f"\n  Parsed result:")
    print(f"    Config seed:  {result['config']['seed']}")
    print(f"    Config quality: {result['config']['quality']}")
    print(f"    Events generated: {len(result['events'])}")
    for ev in result["events"]:
        print(f"      - {ev.get('type', '?')}: {ev}")

    # Create world from WDL
    create_result = json.loads(os.popen(
        f'curl -s -X POST {base}/v1/worlds/from-wdl '
        f'-H "Content-Type: application/json" '
        f"-d '{json.dumps({\"wdl\": json.dumps(wdl_doc)})}'"
    ).read())
    wid = create_result["id"]
    print(f"\n  Created world from WDL: {wid}")

    # Step a few times to show it's running
    for _ in range(30):
        c.control(wid, "step")
    state = c.state(wid)
    print(f"  After 30 ticks: bodies={state['bodies']}, fluid={state['fluid']}")

    # Create visualization
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, (ax1, ax2) = plt.subplots(1, 2, figsize=(16, 7), facecolor="#0b0f17")

    for ax in [ax1, ax2]:
        ax.set_facecolor("#0f1a30")
        for spine in ax.spines.values():
            spine.set_color("#1f2a44")

    # Left: WDL source code
    ax1.set_title("WDL Source", color="#e6eef8", fontsize=12)
    wdl_text = json.dumps(wdl_doc, indent=2)
    ax1.text(0.05, 0.95, wdl_text, transform=ax1.transAxes,
            color="#c8d6f0", fontsize=7.5, va="top", fontfamily="monospace",
            bbox=dict(boxstyle="round", facecolor="#0a1428", edgecolor="#1f2a44"))

    # Right: parsed result
    ax2.set_title("Parsed → World Created", color="#e6eef8", fontsize=12)
    result_text = (
        f"seed:     {result['config']['seed']}\n"
        f"quality:  {result['config']['quality']}\n"
        f"bounds:   {result['config']['bounds']}\n"
        f"gravity:  {result['config']['gravity']}\n"
        f"container: {result['config'].get('container', True)}\n"
        f"\nEvents auto-generated:\n"
    )
    for ev in result["events"]:
        t = ev.get("type", "?")
        pos = ev.get("pos", ev.get("center", []))
        result_text += f"  • {t}: pos={pos}\n"

    result_text += f"\nWorld ID: {wid}\n"
    result_text += f"Tick: {state['tick']}  Bodies: {state['bodies']}  Fluid: {state['fluid']}\n"
    result_text += f"\nOne document → full world"

    ax2.text(0.05, 0.95, result_text, transform=ax2.transAxes,
            color="#c8d6f0", fontsize=9, va="top", fontfamily="monospace",
            bbox=dict(boxstyle="round", facecolor="#0a1428", edgecolor="#1f2a44"))

    fig.suptitle("WDL — World Description Language",
                color="#8ea2c8", fontsize=14, y=0.02)

    path = OUTPUT_DIR / "wdl_scene.png"
    fig.savefig(str(path), dpi=120, bbox_inches="tight", facecolor="#0b0f17")
    print(f"\n  ✓ Saved: {path}")
    plt.close(fig)

    c.delete_world(wid)

if __name__ == "__main__":
    run_demo()
