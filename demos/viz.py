"""Visualization helpers — matplotlib-based plotting for WorldSim demos.

All demos share these utilities:
- `WorldPlotter`: sets up a matplotlib figure with proper scaling
- `plot_particles()`: scatter plot for rigid bodies and fluid
- `plot_heat()`: heatmap for temperature field
- `make_frames()`: step a world and collect frames
- `save_gif()`: save frames as animated GIF
- `save_png()`: save single frame as PNG
"""
import numpy as np
import matplotlib
matplotlib.use("Agg")  # headless backend
import matplotlib.pyplot as plt
from matplotlib.patches import Circle, FancyArrowPatch
from matplotlib.collections import PatchCollection
import os, sys, json, math, time
from pathlib import Path

# WorldSim SDK path
SDK_DIR = Path(__file__).parent.parent / "sdk" / "python"
sys.path.insert(0, str(SDK_DIR))
from worldsim.client import Client

OUTPUT_DIR = Path(__file__).parent / "output"
OUTPUT_DIR.mkdir(exist_ok=True)


def ensure_server(port=18099):
    """Start a worldsim server on the given port if not already running."""
    import subprocess, urllib.request
    try:
        urllib.request.urlopen(f"http://127.0.0.1:{port}/healthz", timeout=2)
        return Client(f"http://127.0.0.1:{port}")
    except Exception:
        pass
    # Start server
    bin_path = Path(__file__).parent.parent / "target" / "release" / "worldsim"
    if not bin_path.exists():
        print(f"  Building worldsim binary...")
        subprocess.run(["cargo", "build", "-p", "worldsim", "--release"],
                       cwd=str(Path(__file__).parent.parent), check=True,
                       capture_output=True)
    proc = subprocess.Popen(
        [str(bin_path), "serve", "--port", str(port)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    time.sleep(1.5)
    print(f"  Server started on port {port} (pid {proc.pid})")
    return Client(f"http://127.0.0.1:{port}")


class WorldPlotter:
    """Manages a matplotlib figure for WorldSim visualization."""

    def __init__(self, title="", bounds=None, figsize=(8, 6)):
        self.fig, self.ax = plt.subplots(1, 1, figsize=figsize)
        self.ax.set_aspect("equal")
        self.ax.set_facecolor("#0b0f17")
        self.fig.patch.set_facecolor("#0b0f17")
        self.ax.set_title(title, color="#e6eef8", fontsize=14, fontweight="bold")
        self.ax.tick_params(colors="#5b6b8a")
        for spine in self.ax.spines.values():
            spine.set_color("#1f2a44")
        if bounds:
            min_x, min_y = bounds[0]
            max_x, max_y = bounds[1]
            self.ax.set_xlim(min_x - 0.2, max_x + 0.2)
            self.ax.set_ylim(min_y - 0.2, max_y + 0.2)
            # Draw bounding box
            from matplotlib.patches import Rectangle
            rect = Rectangle((min_x, min_y), max_x - min_x, max_y - min_y,
                              linewidth=1.5, edgecolor="#2a3f6b", facecolor="none")
            self.ax.add_patch(rect)
        self.frame_count = 0

    def plot_bodies(self, px, py, r, mat_ids, title_text=""):
        """Plot rigid bodies as colored circles."""
        if len(px) == 0:
            return
        MAT_COLORS = {
            0: "#94a3b8", 1: "#c8ccd0", 2: "#d98a5a",
            3: "#9e7042", 4: "#383838", 5: "#bee0e6",
            6: "#cce9fa", 7: "#e6dec8"
        }
        patches = []
        colors = []
        for i in range(len(px)):
            c = Circle((px[i], py[i]), r[i], linewidth=0)
            patches.append(c)
            colors.append(MAT_COLORS.get(int(mat_ids[i]) if i < len(mat_ids) else 0, "#94a3b8"))
        pc = PatchCollection(patches, facecolors=colors, alpha=0.9)
        self.ax.add_collection(pc)
        if title_text:
            self.ax.text(0.02, 0.97, title_text, transform=self.ax.transAxes,
                         color="#8ea2c8", fontsize=10, va="top", fontfamily="monospace")

    def plot_fluid(self, px, py, speed=None, alpha=0.5):
        """Plot fluid particles as scatter."""
        if len(px) == 0:
            return
        if speed is not None and len(speed) > 0:
            colors = np.clip(speed / 3.0, 0, 1)
            self.ax.scatter(px, py, c=colors, cmap="cool", s=8, alpha=alpha, edgecolors="none")
        else:
            self.ax.scatter(px, py, c="#4f7cff", s=8, alpha=alpha, edgecolors="none")

    def plot_heatmap(self, T_grid, nx, ny, bounds, vmin=None, vmax=None):
        """Plot temperature heatmap."""
        min_x, min_y = bounds[0]
        max_x, max_y = bounds[1]
        T = T_grid.reshape(ny, nx)
        extent = [min_x, max_x, min_y, max_y]
        im = self.ax.imshow(T, origin="lower", extent=extent, cmap="inferno",
                            vmin=vmin, vmax=vmax, alpha=0.7, aspect="auto")
        return im

    def add_text(self, text, x=0.02, y=0.97):
        self.ax.text(x, y, text, transform=self.ax.transAxes,
                     color="#8ea2c8", fontsize=10, va="top", fontfamily="monospace")

    def clear(self):
        self.ax.clear()
        self.ax.set_aspect("equal")
        self.ax.set_facecolor("#0b0f17")
        for spine in self.ax.spines.values():
            spine.set_color("#1f2a44")
        self.ax.tick_params(colors="#5b6b8a")

    def save(self, path):
        self.fig.savefig(path, dpi=120, bbox_inches="tight",
                         facecolor=self.fig.get_facecolor())
        self.frame_count += 1

    def close(self):
        plt.close(self.fig)


def make_frames(client, world_id, n_steps, dt_per_frame, title, plot_fn, **kwargs):
    """Step a world and collect matplotlib figure frames.

    plot_fn(frame_data, plotter) should draw on the plotter.
    Returns list of matplotlib figure snapshots.
    """
    frames = []
    for step in range(n_steps):
        client.control(world_id, "step")
        frame_data = plot_fn(client, world_id, step, **kwargs)
        pw = WorldPlotter(title=f"{title}  t={frame_data.get('time', 0):.2f}s",
                          bounds=frame_data.get("bounds", [(-2,0),(2,4)]))
        pw.plot_heatmap(frame_data["T"], frame_data["nx"], frame_data["ny"],
                        frame_data["bounds"]) if "T" in frame_data else None
        pw.plot_fluid(frame_data["fx"], frame_data["fy"],
                      frame_data.get("speed")) if "fx" in frame_data else None
        pw.plot_bodies(frame_data["bx"], frame_data["by"],
                       frame_data.get("br", np.array([0.1]*len(frame_data["bx"]))),
                       frame_data.get("bm", np.zeros(len(frame_data["bx"])))) if "bx" in frame_data else None
        pw.add_text(frame_data.get("label", ""))
        tmp_path = OUTPUT_DIR / f"_tmp_frame_{step:04d}.png"
        pw.save(tmp_path)
        frames.append(str(tmp_path))
        pw.close()
    return frames


def save_gif(frame_paths, output_path, duration=150):
    """Save a list of PNG paths as an animated GIF."""
    try:
        from PIL import Image
        images = [Image.open(p) for p in frame_paths]
        images[0].save(str(output_path), save_all=True, append_images=images[1:],
                       duration=duration, loop=0)
        print(f"  ✓ Saved GIF: {output_path} ({len(images)} frames)")
    except ImportError:
        # Fallback: just keep the last frame
        if frame_paths:
            import shutil
            shutil.copy(frame_paths[-1], str(output_path))
            print(f"  ✓ Saved PNG (install Pillow for GIF): {output_path}")
    # Cleanup temp frames
    for p in frame_paths:
        try:
            os.remove(p)
        except OSError:
            pass


def save_png(fig_or_path, output_path):
    """Save a single PNG."""
    if isinstance(fig_or_path, str):
        import shutil
        shutil.copy(fig_or_path, str(output_path))
    else:
        fig_or_path.savefig(str(output_path), dpi=120, bbox_inches="tight")
    print(f"  ✓ Saved: {output_path}")


def extract_state(client, world_id, bounds):
    """Extract current state for plotting."""
    import urllib.request
    base = client.base_url
    wid = world_id

    # Get state
    state = json.loads(urllib.request.urlopen(f"{base}/v1/worlds/{wid}/state").read())

    # Parse fluid particles from state
    fluid_n = state.get("fluid", 0)
    bodies = state.get("bodies", 0)

    # We'll use probe + direct state for plotting
    # For body positions, we need to probe or use state hash
    # In v0.1, state doesn't expose per-body arrays via JSON
    # We'll use a workaround: create a second world and snapshot it

    return {
        "tick": state.get("tick", 0),
        "time": state.get("time", 0),
        "fluid_n": fluid_n,
        "bodies": bodies,
        "bounds": bounds,
    }
