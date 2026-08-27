# WorldSim 3D Viewer

WorldSim includes a real-time **WebGL2 3D** viewer embedded in the server binary. It is not a screenshot generator or a 2D canvas: the client receives x/y/z columns, builds a perspective view-projection matrix, performs depth-tested point-sprite rendering, and lets the user orbit a camera around the world.

## Start

```bash
cargo run -p worldsim -- demo --port 8090
# Open http://127.0.0.1:8090/viewer/
```

The same assets are available from a release binary; no Node, npm, bundler, or separate static-file server is needed.

## Controls

| Control | Action |
|---|---|
| Drag left mouse | Orbit camera (yaw/pitch) |
| Mouse wheel | Zoom |
| Pause/Resume | Pause or resume realtime world scheduler |
| Speed | Set world time scale: 1/4×, 1/2×, 1×, 2×, 4× |
| Quality | Lock Low/Medium/High QoS for simulation and stream density |
| Spawn | Add steel/wood/rubber sphere, water blob, or heat source |
| Click ground | Place a selected rigid sphere at the clicked 3D ray/ground intersection |
| Reset view | Fit the camera to the world AABB |

## What is rendered

The WebSocket frame contains three independent coordinate sets:

```json
{
  "bodies": {"px":"…", "py":"…", "pz":"…", "r":"…", "mat":"…"},
  "fluid":  {"px":"…", "py":"…", "pz":"…", "speed":"…", "stride":2},
  "nbody":  {"px":"…", "py":"…", "pz":"…", "r":"…"}
}
```

- **Rigid bodies**: perspective-sized circular impostors, colored by material.
- **WCSPH fluid**: blue/cyan/white speed heatmap, QoS downsampled for bandwidth.
- **N-body masses**: gold point masses with visual radius derived from mass.
- **World bounds**: 3D wireframe AABB and floor grid.
- **HUD**: tick, physical time, measured simulation time, bodies, fluid, N-body count, QoS state.

The viewer uses persistent GPU buffer objects. It does not allocate and delete WebGL buffers on every frame.

## Protocol

The viewer is a normal read-only subscriber:

1. `GET /v1/worlds` lists worlds.
2. `GET /v1/worlds/:id` obtains the AABB and world metadata.
3. `GET /v1/worlds/:id/stream` upgrades to WebSocket.
4. The server sends a JSON text frame roughly every 30 ms.
5. Base64 little-endian `f32` columns are decoded in the browser.
6. The render loop runs independently with `requestAnimationFrame`.

Rendering never blocks the simulation. If a client disappears, the server prunes its channel on the next broadcast.

## Browser requirements

- WebGL2-capable browser (modern Chrome, Edge, Firefox, or Safari with WebGL2 enabled).
- JavaScript and WebSocket enabled.
- For a headless/server-only deployment, do not open the viewer; the kernel and REST/SSE APIs have no graphics dependency.

If WebGL2 is unavailable, the page reports the limitation. The simulation itself continues to run and remains accessible through REST, SSE, and the Python SDK.

## Offline 3D GIF

For a reproducible artifact that does not require a browser:

```bash
pip install -r demos/requirements.txt
python demos/three_d_scene.py --port 8090
```

This reads the public `/step` observation and plots actual x/y/z columns using a Matplotlib `Axes3D`; the resulting `demos/output/three_d_scene.gif` is a scientific/offline companion to the online WebGL2 view.
