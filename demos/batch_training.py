#!/usr/bin/env python3
"""Demo 7: batch training — many manually stepped environments."""
import sys, os, time
_ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0,_ROOT);sys.path.insert(0,os.path.join(_ROOT,"sdk","python"))


def run_demo(port=18099):
    from worldsim.client import Client
    from worldsim.training import BatchTrainer
    from demos.viz import ensure_server, OUTPUT_DIR
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    print("=== Demo 7: batch training ===")
    # ensure_server returns a client and keeps the same server for this process.
    client=ensure_server(port)
    trainer=BatchTrainer(f"http://127.0.0.1:{port}")
    ids=trainer.create(32,config={
        "quality":"low","dt":0.0166667,
        "systems":{"fluid":{"enabled":False},"heat":{"enabled":False}},
    },name_prefix="train")
    print(f"  Created {len(ids)} manual worlds")
    trainer.inject(ids,[{"type":"spawn_sphere","material":"steel","pos":[0,2,0],"radius":.1}])
    timings=[]; step_results=[]
    for rep in range(5):
        started=time.perf_counter()
        result=trainer.step(ids,ticks_per_world=20,include_observation=False)
        timings.append((time.perf_counter()-started)*1000)
        step_results=result
        print(f"  run {rep+1}: {timings[-1]:.1f} ms, tick={result[0]['tick']}")
    best=min(timings); avg=sum(timings)/len(timings)
    total=len(ids)*20; throughput=total/(best/1000)
    print(f"  best={best:.1f}ms avg={avg:.1f}ms throughput={throughput:.0f} world-steps/s")

    fig,(left,right)=plt.subplots(1,2,figsize=(13,5),facecolor="#0b0f17")
    for ax in (left,right):
        ax.set_facecolor("#0f1a30")
        for spine in ax.spines.values(): spine.set_color("#1f2a44")
        ax.tick_params(colors="#8ea2c8")
    left.bar(range(1,len(timings)+1),timings,color="#4f7cff",alpha=.85)
    left.axhline(avg,color="#ff6b6b",linestyle="--",label=f"avg {avg:.1f}ms")
    left.set_title("32 environments × 20 synchronous ticks",color="#e6eef8")
    left.set_xlabel("batch run",color="#8ea2c8");left.set_ylabel("HTTP batch call (ms)",color="#8ea2c8")
    left.legend(facecolor="#0f1a30",edgecolor="#1f2a44",labelcolor="#8ea2c8")
    right.axis("off")
    right.text(.08,.9,
        "BATCH CONTRACT\n\n"
        f"worlds              {len(ids)}\n"
        "mode                auto_start=false\n"
        "ticks per call      20\n"
        f"total transitions    {total}\n\n"
        f"best wall time       {best:.1f} ms\n"
        f"throughput           {throughput:.0f} steps/s\n\n"
        "Each result contains:\n"
        "  tick · time · hash · observation\n\n"
        "No realtime scheduler race;\n"
        "step is atomic per world.",
        transform=right.transAxes,color="#c8d6f0",fontsize=10,va="top",family="monospace",
        bbox=dict(boxstyle="round",facecolor="#0a1428",edgecolor="#1f2a44"))
    fig.suptitle("WorldSim — batch training data plane",color="#8ea2c8",fontsize=14)
    path=OUTPUT_DIR/"batch_training.png";fig.savefig(path,dpi=120,bbox_inches="tight",facecolor="#0b0f17");plt.close(fig)
    print(f"  ✓ Saved: {path}")
    for wid in ids: client.delete_world(wid)


if __name__=="__main__":
    import argparse
    p=argparse.ArgumentParser();p.add_argument("--port",type=int,default=18099);run_demo(p.parse_args().port)
