#!/usr/bin/env python3
"""Demo 4: heat diffusion — render the actual 3D temperature field's centre slice."""
import sys, os
_ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0,_ROOT);sys.path.insert(0,os.path.join(_ROOT,"sdk","python"))


def run_demo(port=18099):
    from demos.viz import ensure_server, WorldPlotter, OUTPUT_DIR
    from PIL import Image
    import numpy as np
    print("=== Demo 4: heat diffusion ===")
    c=ensure_server(port)
    w=c.create_world(name="heat",config={
        "auto_start":False,"quality":"low","gravity":[0,0,0],
        "bounds":{"min":[-2,0,-1],"max":[2,4,1]},"container":True,
        "systems":{"fluid":{"enabled":False},"heat":{"enabled":True,"res":[16,16,16],"alpha":1.43e-5},"nbody":{"enabled":False}},
    })
    wid=w["id"]
    c.inject(wid,[{"type":"heat_source","pos":[-1.4,2.0,0],"radius":.42,"power_watts":5000,"duration_ticks":0}])
    c.control(wid,"flush")
    frames=[]
    for step in range(240):
        obs=c.step_sync(wid,1,include_fields=True)["observation"]
        if step%6==0:
            fields=obs["fields"]; vals=c.decode_f64_array(fields["temperature"])
            nx,ny,nz=int(fields["nx"]),int(fields["ny"]),int(fields["nz"])
            # centre z slice: layout ix + nx*(iy + ny*iz)
            grid=vals.reshape(nz,ny,nx)[nz//2]
            plot=WorldPlotter(f"Heat diffusion  t={obs['time']:.2f}s",((-2,0),(2,4)))
            im=plot.ax.imshow(grid-293.15,origin="lower",extent=(-2,2,0,4),cmap="inferno",vmin=0,vmax=max(1,float((grid-293.15).max())),alpha=.86,aspect="auto")
            plot.fig.colorbar(im,ax=plot.ax,shrink=.65,label="ΔT (K)")
            plot.ax.plot(-1.4,2,"o",color="#ff4444",markersize=9)
            plot.add_text(f"grid={nx}×{ny}×{nz}  α=1.43e-5 m²/s\ncentre z-slice from /step include_fields")
            path=OUTPUT_DIR/f"_heat_{step:04d}.png";plot.save(path);frames.append(path);plot.close()
    if frames:
        imgs=[Image.open(str(p)) for p in frames];out=OUTPUT_DIR/"heat_diffusion.gif"
        imgs[0].save(str(out),save_all=True,append_images=imgs[1:],duration=120,loop=0)
        for p in frames:os.remove(p)
        print(f"  ✓ Saved: {out} ({len(imgs)} frames)")
    c.delete_world(wid)

if __name__=="__main__":
    import argparse
    p=argparse.ArgumentParser();p.add_argument("--port",type=int,default=18099);run_demo(p.parse_args().port)
