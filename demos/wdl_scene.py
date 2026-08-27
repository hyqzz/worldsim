#!/usr/bin/env python3
"""Demo 9: WDL — declarative world description and live creation."""
import sys
import os
import json

_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, _ROOT)
sys.path.insert(0, os.path.join(_ROOT, "sdk", "python"))


def run_demo(port=18099):
    from demos.viz import ensure_server, OUTPUT_DIR
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    print("=== Demo 9: WDL — declarative scene description ===")
    client = ensure_server(port)

    wdl_document = {
        "world": {
            "seed": 42,
            "auto_start": False,
            "quality": "low",
            "bounds": {"min": [-2, 0, -2], "max": [2, 4, 2]},
            "gravity": [0, -9.81, 0],
            "container": True,
        },
        "entities": [
            {"pool": {"material": "water", "shape": "fluid", "at": [0, 0.8, 0], "radius": 0.45}},
            {"wood_ball": {"material": "wood", "shape": "sphere[r=0.15]", "at": [0.5, 2.5, 0]}},
            {"steel_ball": {"material": "steel", "shape": "sphere[r=0.12]", "at": [-0.5, 2.5, 0]}},
        ],
        "laws": ["gravity", "navier-stokes", "heat-transfer"],
    }
    wdl_text = json.dumps(wdl_document, indent=2)
    parsed = client.parse_wdl(wdl_text)
    print(f"  Parsed seed={parsed['config']['seed']} events={len(parsed['events'])}")

    created = client.create_world_from_wdl(wdl_text)
    wid = created["id"]
    # WDL explicitly creates a manual world; step it synchronously.
    client.control(wid, "flush")
    result = client.step_sync(wid, ticks=20, include_fluid=False)
    state = result["state"]
    print(f"  Created {wid}: tick={state['tick']} bodies={state['bodies']} fluid={state['fluid']}")

    fig, (source_ax, result_ax) = plt.subplots(1, 2, figsize=(16, 7), facecolor="#0b0f17")
    for ax in (source_ax, result_ax):
        ax.set_facecolor("#0f1a30")
        ax.axis("off")
        for spine in ax.spines.values():
            spine.set_color("#1f2a44")
    source_ax.set_title("WDL source", color="#e6eef8", fontsize=12)
    source_ax.text(0.03, 0.98, wdl_text, transform=source_ax.transAxes,
                   color="#c8d6f0", fontsize=7, va="top", family="monospace",
                   bbox=dict(boxstyle="round", facecolor="#0a1428", edgecolor="#1f2a44"))
    result_ax.set_title("Parsed → executable world", color="#e6eef8", fontsize=12)
    result_text = (
        f"config.seed       {parsed['config']['seed']}\n"
        f"config.auto_start {parsed['config']['auto_start']}\n"
        f"config.quality    {parsed['config']['quality']}\n"
        f"generated events  {len(parsed['events'])}\n\n"
        f"world id          {wid}\n"
        f"tick              {state['tick']}\n"
        f"rigid bodies      {state['bodies']}\n"
        f"fluid particles   {state['fluid']}\n\n"
        "One document → one reproducible world"
    )
    result_ax.text(0.08, 0.92, result_text, transform=result_ax.transAxes,
                   color="#c8d6f0", fontsize=11, va="top", family="monospace",
                   bbox=dict(boxstyle="round", facecolor="#0a1428", edgecolor="#1f2a44"))
    fig.suptitle("WorldSim WDL — declare once, run through any client", color="#8ea2c8", fontsize=14)
    path = OUTPUT_DIR / "wdl_scene.png"
    fig.savefig(str(path), dpi=120, bbox_inches="tight", facecolor="#0b0f17")
    plt.close(fig)
    print(f"  ✓ Saved: {path}")
    client.delete_world(wid)


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--port", type=int, default=18099)
    run_demo(parser.parse_args().port)
