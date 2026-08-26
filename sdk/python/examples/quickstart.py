"""WorldSim quickstart — create a world, spawn things, probe, and read one streaming frame."""
import time
from worldsim import Client

c = Client("http://127.0.0.1:8090")

# 1. Create a world
w = c.create_world(name="quickstart", config={"quality": "low"})
wid = w["id"]
print(f"world {wid}  config={w['world']['config']['quality']}")

# 2. Inject a water blob + two balls (wood floats, steel sinks — buoyancy is real)
c.inject(wid, [
    {"type": "spawn_fluid",  "center": [0, 1.0, 0], "radius": 0.7},
    {"type": "spawn_sphere", "material": "wood",  "pos": [0.4, 2.6, 0], "radius": 0.17},
    {"type": "spawn_sphere", "material": "steel", "pos": [-0.4, 2.6, 0], "radius": 0.11},
])
print("injected: water + wood + steel")
time.sleep(1.0)

# 3. Probe the world
for y in [0.3, 1.0, 2.0]:
    print("probe", y, c.probe(wid, 0, y, 0, quantity="density"))
    print("probe", y, c.probe(wid, 0, y, 0, quantity="temperature"))

# 4. Read one live frame via SSE
print("\nreading one streaming frame…")
for frame in c.watch(wid, max_frames=1):
    print(f"tick {frame['tick']}  bodies {frame['bodies']['n']}  fluid {frame['fluid']['n']}  quality {frame['stats']['quality']}")
    break

print("\nOpen the viewer at http://127.0.0.1:8090/viewer/  — the same world is visible there.")
print(f"World id for the viewer dropdown: {wid}")
