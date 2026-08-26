"""Buoyancy demo: wood floats, steel sinks — the same code the validation suite checks."""
import time
from worldsim import Client

c = Client("http://127.0.0.1:8090")
w = c.create_world(name="floating", config={"quality": "low"})
wid = w["id"]
print(f"world {wid}")

# A calm water column
c.inject(wid, [{"type": "spawn_fluid", "center": [0, 0.9, 0], "radius": 0.9}])
time.sleep(0.8)

# Drop two balls from the same height
c.inject(wid, [
    {"type": "spawn_sphere", "material": "wood",  "pos": [0.45, 2.8, 0], "radius": 0.19},
    {"type": "spawn_sphere", "material": "steel", "pos": [-0.45, 2.8, 0], "radius": 0.13},
])
print("dropped wood (should float) + steel (should sink)")

# Watch their y positions via state polling (coarse, but dependency-free)
for _ in range(14):
    s = c.state(wid)
    print(f"tick {s['tick']:4}  bodies {s['bodies']}  fluid {s['fluid']}")
    time.sleep(0.5)

# Final probe: density at two heights shows stratification after settling
print("\ndensity near bottom:", c.probe(wid, 0, 0.2, 0, quantity="density"))
print("density near top:   ", c.probe(wid, 0, 2.0, 0, quantity="density"))
print("\nIf the platform is correct, wood hovers near the surface while steel rests on the floor.")
