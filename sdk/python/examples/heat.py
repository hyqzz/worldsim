"""Heat diffusion demo: a heat source warms the water, conduction spreads."""
import time
from worldsim import Client

c = Client("http://127.0.0.1:8090")
w = c.create_world(name="heat", config={"quality": "low"})
wid = w["id"]
print(f"world {wid}")

# Water + persistent heat source near the left wall
c.inject(wid, [
    {"type": "spawn_fluid",  "center": [0, 0.9, 0], "radius": 0.75},
    {"type": "heat_source",  "pos": [-0.95, 0.4, 0.1], "radius": 0.32, "power_watts": 7000, "duration_ticks": 0},
])
print("water + heat source active")

def sample_t(pos):
    return c.probe(wid, *pos, quantity="temperature")["value"]

probe_pos = [0, 0.5, 0]
for i in range(12):
    t = sample_t(probe_pos)
    print(f"tick {c.state(wid)['tick']:4}  T at {probe_pos} = {t:.2f} K")
    time.sleep(0.6)

print("\nExpected: temperature rises monotonically while the source is active and spreads by diffusion.")
