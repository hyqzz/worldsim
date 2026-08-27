#!/usr/bin/env python3
"""WorldSim batch training client — step many envs in parallel via HTTP.

Usage::

    from worldsim.training import BatchTrainer
    t = BatchTrainer("http://127.0.0.1:8090")
    ids = t.create(64, config={"quality":"low"})
    for ep in range(100):
        t.inject(ids, [{"type":"spawn_sphere","material":"steel","pos":[0,2.8,0],"radius":0.1}])
        results = t.step(ids, ticks=10)
        for r in results:
            print(f"{r['id']} tick={r['tick']} hash={r['hash'][:8]}")
"""
from __future__ import annotations

import time
from typing import Any, Dict, List, Optional

from .client import Client


class BatchTrainer:
    """High-throughput batch training via the WorldSim batch API.

    Parameters
    ----------
    base_url : str
        WorldSim server URL.
    """

    def __init__(self, base_url: str = "http://127.0.0.1:8090"):
        self.client = Client(base_url)

    def create(
        self,
        count: int,
        config: Optional[Dict[str, Any]] = None,
        name_prefix: str = "batch",
    ) -> List[str]:
        """Create `count` identical worlds with a shared template config.

        Returns
        -------
        list of world ids
        """
        body = {"count": count, "name_prefix": name_prefix}
        if config:
            body["template_config"] = config
        resp = self.client._request("POST", "/v1/batch/create", body)
        return resp.get("ids", [])

    def step(
        self,
        ids: List[str],
        ticks_per_world: int = 1,
        include_observation: bool = True,
        include_fluid: bool = False,
        fluid_stride: int = 1,
    ) -> List[Dict[str, Any]]:
        """Step all worlds in parallel and optionally return observations.

        Each world runs `ticks_per_world` synchronous ticks.  The returned
        observation is post-step data, never a stale realtime snapshot.
        Set ``include_observation=False`` for a pure throughput benchmark.
        """
        resp = self.client._request("POST", "/v1/batch/step", {
            "ids": ids,
            "ticks_per_world": ticks_per_world,
            "include_observation": include_observation,
            "include_fluid": include_fluid,
            "fluid_stride": fluid_stride,
        })
        return resp.get("results", [])

    def inject(self, ids: List[str], events: List[Dict[str, Any]]) -> Dict[str, Any]:
        """Inject the same events into all worlds."""
        return self.client._request("POST", "/v1/batch/inject", {
            "ids": ids,
            "events": events,
        })

    def transition(self, transitions: List[Dict[str, Any]], ticks_per_world: int = 1,
                   include_observation: bool = True, include_fluid: bool = False,
                   fluid_stride: int = 1) -> List[Dict[str, Any]]:
        """Apply per-world events and advance all environments atomically.

        ``transitions`` is ``[{"id": wid, "events": [...]}, ...]``. This is
        the preferred vectorized RL call when each environment has a different
        action.
        """
        response = self.client._request("POST", "/v1/batch/transition", {
            "transitions": transitions,
            "ticks_per_world": ticks_per_world,
            "include_observation": include_observation,
            "include_fluid": include_fluid,
            "fluid_stride": fluid_stride,
        })
        return response.get("results", [])

    def benchmark(self, count: int = 64, ticks: int = 10, repeats: int = 5, config: Optional[Dict] = None) -> Dict[str, Any]:
        """Run a benchmark: create `count` worlds, step `ticks` for `repeats` iterations.

        Returns
        -------
        dict with keys: worlds, ticks_per_world, repeats, total_ticks,
                        wall_time_ms, steps_per_second, avg_tick_ms
        """
        ids = self.create(count, config=config)
        all_ticks = ticks * repeats
        t0 = time.perf_counter()
        for _ in range(repeats):
            self.step(ids, ticks_per_world=ticks, include_observation=False)
        wall_ms = (time.perf_counter() - t0) * 1000
        total_steps = count * all_ticks
        return {
            "worlds": count,
            "ticks_per_world": ticks,
            "repeats": repeats,
            "total_ticks": all_ticks,
            "total_steps": total_steps,
            "wall_time_ms": round(wall_ms, 1),
            "steps_per_second": round(total_steps / (wall_ms / 1000), 0),
            "avg_tick_ms": round(wall_ms / all_ticks, 2),
        }
