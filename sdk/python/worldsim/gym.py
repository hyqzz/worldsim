"""Gymnasium adapter for synchronous WorldSim training.

This module is optional: install ``worldsim[training]`` to get NumPy and
Gymnasium. Unlike the realtime viewer API, this adapter uses the atomic
``POST /v1/worlds/:id/step`` endpoint, so every ``step(action)`` returns the
state *after* that action and exactly one simulation tick.
"""
from __future__ import annotations

import copy
from typing import Any, Callable, Dict, List, Optional, Tuple

import gymnasium as gym
import numpy as np

from .client import Client


class WorldSimEnv(gym.Env):
    """A synchronous Gymnasium environment backed by a WorldSim world.

    Parameters
    ----------
    base_url:
        WorldSim server URL.
    config:
        Partial WorldConfig. The adapter forces ``auto_start=false`` so the
        realtime scheduler cannot race the RL loop.
    initial_events:
        Events injected during reset. If omitted, a small water/ball scene is
        created. Pass ``[]`` for a completely empty world.
    body_slots:
        Fixed number of rigid bodies represented in the observation. Extra
        bodies are ignored; missing slots are zero-padded. A fixed shape is
        required by Gymnasium.
    max_steps:
        Episode horizon.
    reward_fn:
        Optional ``Callable[[np.ndarray, np.ndarray], float]`` receiving
        ``(observation, action)``. Default reward is 0; physical simulation
        cannot choose a task-specific reward for the caller.
    """

    metadata = {"render_modes": ["human"], "render_fps": 60}

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8090",
        config: Optional[Dict[str, Any]] = None,
        initial_events: Optional[List[Dict[str, Any]]] = None,
        body_slots: int = 2,
        max_steps: int = 1000,
        render_mode: Optional[str] = None,
        reward_fn: Optional[Callable[[np.ndarray, np.ndarray], float]] = None,
    ):
        super().__init__()
        if body_slots < 1:
            raise ValueError("body_slots must be positive")
        self.client = Client(base_url)
        self.config = copy.deepcopy(config or {"quality": "low", "dt": 0.0166667})
        self.initial_events = initial_events
        self.body_slots = body_slots
        self.max_steps = max_steps
        self.render_mode = render_mode
        self.reward_fn = reward_fn
        self.world_id: Optional[str] = None
        self.step_count = 0
        # Each body contributes position + velocity; final scalar is mean T.
        self._obs_dim = body_slots * 6 + 1
        self.observation_space = gym.spaces.Box(
            low=-np.inf, high=np.inf, shape=(self._obs_dim,), dtype=np.float32
        )
        # Action is a force in newtons applied to body slot 0.
        self.action_space = gym.spaces.Box(
            low=np.full(3, -10.0, dtype=np.float32),
            high=np.full(3, 10.0, dtype=np.float32),
            dtype=np.float32,
        )

    def _default_events(self) -> List[Dict[str, Any]]:
        return [
            {"type": "spawn_fluid", "center": [0, 1.0, 0], "radius": 0.5},
            {"type": "spawn_sphere", "material": "wood", "pos": [0.4, 2.6, 0], "radius": 0.17},
            {"type": "spawn_sphere", "material": "steel", "pos": [-0.4, 2.6, 0], "radius": 0.12},
        ]

    def reset(self, *, seed: Optional[int] = None, options: Optional[Dict[str, Any]] = None) -> Tuple[np.ndarray, Dict[str, Any]]:
        super().reset(seed=seed)
        if self.world_id is not None:
            try:
                self.client.delete_world(self.world_id)
            except Exception:
                pass
        cfg = copy.deepcopy(self.config)
        cfg["auto_start"] = False
        if seed is not None:
            # Keep all 64 bits without passing a lossy JSON float.
            cfg["seed_hex"] = f"{int(seed) & ((1 << 64) - 1):016x}"
        response = self.client.create_world(config=cfg, name=f"gym-{seed if seed is not None else 0}")
        self.world_id = response["id"]
        events = self._default_events() if self.initial_events is None else copy.deepcopy(self.initial_events)
        if events:
            self.client.inject(self.world_id, events)
            self.client.control(self.world_id, "flush")
        self.step_count = 0
        obs = self._observation_from_payload(
            self.client.observe(self.world_id).get("observation", {})
        )
        return obs, {"world_id": self.world_id, "tick": 0}

    def step(self, action: np.ndarray):
        if self.world_id is None:
            raise RuntimeError("call reset() before step()")
        action = np.asarray(action, dtype=np.float32)
        if not self.action_space.contains(action):
            raise ValueError(f"action must be a 3-vector in [-10,10], got {action!r}")
        self.step_count += 1
        result = self.client.step_sync(self.world_id, ticks=1, include_fluid=False, events=[{
            "type": "apply_force",
            "target": {"body": 0},
            "force": action.astype(float).tolist(),
            "ticks": 1,
        }])
        obs = self._observation_from_payload(result.get("observation", {}))
        terminated = self.step_count >= self.max_steps
        truncated = False
        reward = float(self.reward_fn(obs, action) if self.reward_fn else 0.0)
        info = {
            "world_id": self.world_id,
            "tick": result.get("state", {}).get("tick", self.step_count),
            "hash": result.get("state", {}).get("hash"),
        }
        return obs, reward, terminated, truncated, info

    @staticmethod
    def _decode_f64(value: Any) -> np.ndarray:
        if not value:
            return np.empty(0, dtype=np.float64)
        import base64
        return np.frombuffer(base64.b64decode(value), dtype="<f8").copy()

    def _observation_from_payload(self, payload: Dict[str, Any]) -> np.ndarray:
        obs = np.zeros(self._obs_dim, dtype=np.float32)
        bodies = payload.get("bodies") or {}
        n = min(int(bodies.get("n", 0)), self.body_slots)
        px = self._decode_f64(bodies.get("px")); py = self._decode_f64(bodies.get("py")); pz = self._decode_f64(bodies.get("pz"))
        vx = self._decode_f64(bodies.get("vx")); vy = self._decode_f64(bodies.get("vy")); vz = self._decode_f64(bodies.get("vz"))
        for i in range(n):
            base = i * 6
            arrays = (px, py, pz, vx, vy, vz)
            for j, arr in enumerate(arrays):
                if i < len(arr):
                    obs[base + j] = float(arr[i])
        obs[-1] = float(payload.get("temperature_mean", 293.15))
        return obs

    def render(self):
        if self.render_mode == "human":
            print(f"WorldSim tick={self.step_count} world={self.world_id}")

    def close(self):
        if self.world_id:
            try:
                self.client.delete_world(self.world_id)
            except Exception:
                pass
            self.world_id = None
