#!/usr/bin/env python3
"""Demo 11: true 3D offline visualization from WorldSim state columns.

The online viewer is WebGL2. This companion demo proves that the same public
state can also be consumed by a scientific Python 3D renderer: no hidden
2-D projection is used while collecting data; x/y/z columns are plotted in a
real Axes3D scene.
"""
import sys, os
_ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0,_ROOT);sys.path.insert(0,os.path.join(_ROOT,"sdk","python"))


def run_demo(port=18099):
    from demos.viz import ensure_server, OUTPUT_DIR
    from worldsim.client import Client
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from mpl_toolkits.mplot3d import Axes3D  # noqa: F401: registers projection
    from PIL import Image
    import numpy as np
    print("=== Demo 11: true 3D scene ===")
    c=ensure_server(port)
    w=c.create_world(name="three_d",config={
        "auto_start":False,"quality":"low","gravity":[0,-9.81,0],
        "bounds":{"min":[-2,0,-2],"max":[2,4,2]},"container":True,
        "systems":{"fluid":{"enabled":True,"h":.12,"rho0":1000,"c":12},"heat":{"enabled":False}},
    })
    wid=w["id"]
    c.inject(wid,[
        {"type":"spawn_fluid","center":[0,0.9,0],"radius":.55},
        {"type":"spawn_sphere","material":"wood","pos":[-.45,2.7,.2],"radius":.16},
        {"type":"spawn_sphere","material":"steel","pos":[.45,2.9,-.2],"radius":.13},
        {"type":"spawn_sphere","material":"rubber","pos":[0,3.2,0],"radius":.11},
    ])
    c.control(wid,"flush")
    frames=[]
    for step in range(150):
        obs=c.step_sync(wid,1,include_fluid=True,fluid_stride=1)["observation"]
        if step%3: continue
        fig=plt.figure(figsize=(8,7),facecolor="#0b0f17")
        ax=fig.add_subplot(111,projection="3d",facecolor="#0b0f17")
        b=obs["bodies"];bx=c.decode_f64_array(b["px"]);by=c.decode_f64_array(b["py"]);bz=c.decode_f64_array(b["pz"])
        f=obs["fluid"];fx=c.decode_f64_array(f["px"]);fy=c.decode_f64_array(f["py"]);fz=c.decode_f64_array(f["pz"])
        if len(fx): ax.scatter(fx,fy,fz,s=3,c="#4f9dff",alpha=.24,depthshade=False)
        palette=["#94a3b8","#9e7042","#383838"]
        for i in range(len(bx)):
            ax.scatter([bx[i]],[by[i]],[bz[i]],s=90,c=palette[i%len(palette)],edgecolors="white",linewidths=.5,depthshade=True)
        ax.set_xlim(-2,2);ax.set_ylim(0,4);ax.set_zlim(-2,2)
        ax.set_xlabel("x (m)",color="#8ea2c8");ax.set_ylabel("y (m)",color="#8ea2c8");ax.set_zlabel("z (m)",color="#8ea2c8")
        ax.tick_params(colors="#8ea2c8");ax.set_title(f"WorldSim true 3D state  t={obs['time']:.2f}s",color="#e6eef8")
        ax.text2D(.02,.95,"WebGL2 viewer uses the same x/y/z columns\nWCSPH particles + rigid bodies",transform=ax.transAxes,color="#8ea2c8",fontsize=9)
        path=OUTPUT_DIR/f"_3d_{step:04d}.png";fig.savefig(path,dpi=100,facecolor="#0b0f17");frames.append(path);plt.close(fig)
    if frames:
        imgs=[Image.open(str(p)) for p in frames];out=OUTPUT_DIR/"three_d_scene.gif"
        imgs[0].save(str(out),save_all=True,append_images=imgs[1:],duration=100,loop=0)
        for p in frames:os.remove(p)
        print(f"  ✓ Saved: {out} ({len(imgs)} frames)")
    c.delete_world(wid)

if __name__=="__main__":
    import argparse
    p=argparse.ArgumentParser();p.add_argument("--port",type=int,default=18099);run_demo(p.parse_args().port)
