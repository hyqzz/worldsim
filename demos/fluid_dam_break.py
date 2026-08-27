#!/usr/bin/env python3
"""Demo 2: WCSPH dam-break — plot actual server particle positions."""
import sys, os
_ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0,_ROOT); sys.path.insert(0,os.path.join(_ROOT,"sdk","python"))


def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    from PIL import Image
    import numpy as np
    print("=== Demo 2: WCSPH dam-break ===")
    c=ensure_server(port)
    w=c.create_world(name="dam_break",config={
        "auto_start":False,"quality":"low","gravity":[0,-9.81,0],
        "bounds":{"min":[-3,0,-1],"max":[3,4,1]},"container":True,
        "systems":{"fluid":{"enabled":True,"h":0.10,"rho0":1000,"c":12},
                    "heat":{"enabled":False},"nbody":{"enabled":False}},
    })
    wid=w["id"]
    c.inject(wid,[{"type":"spawn_fluid","center":[-1.5,0.8,0],"radius":0.62}])
    c.control(wid,"flush")
    frames=[]
    for step in range(240):
        obs=c.step_sync(wid,1,include_fluid=True,fluid_stride=1)["observation"]
        fl=obs["fluid"]
        px=c.decode_f64_array(fl["px"]); py=c.decode_f64_array(fl["py"])
        speed=c.decode_f64_array(fl["vx"])
        # vx is a column, use it as a cheap colour signal; all coordinates are actual f64 state.
        if step%4==0:
            plot=WorldPlotter(f"WCSPH dam break  t={obs['time']:.2f}s",((-3,0),(3,4)))
            plot.ax.scatter(px,py,c=np.abs(speed) if len(speed) else "#4f7cff",cmap="cool",s=11,alpha=.72,edgecolors="none")
            plot.ax.axhline(0,color="#2a3f6b",linewidth=2)
            plot.add_text(f"particles={len(px)}  h=0.10m\nTait EOS γ=7 · Monaghan viscosity\npositions from /step observation")
            path=OUTPUT_DIR/f"_dam_{step:04d}.png";plot.save(path);frames.append(path);plot.close()
    if frames:
        imgs=[Image.open(str(p)) for p in frames];out=OUTPUT_DIR/"dam_break.gif"
        imgs[0].save(str(out),save_all=True,append_images=imgs[1:],duration=100,loop=0)
        for p in frames:os.remove(p)
        print(f"  ✓ Saved: {out} ({len(imgs)} frames)")
    print("  Result: actual WCSPH particle positions and velocities")
    c.delete_world(wid)

if __name__=="__main__":
    import argparse
    p=argparse.ArgumentParser();p.add_argument("--port",type=int,default=18099);run_demo(p.parse_args().port)
