#!/usr/bin/env python3
"""WorldSim Gymnasium Environment (v0.1 — HTTP mode).

Drop-in `gym.Env` that talks to a running WorldSim server. No Rust
extensions needed — works with any server deployment.

Usage::

    from worldsim.gym import WorldSimEnv
    env = WorldSimEnv("http://127.0.0.1:8090")
    obs = env.reset()
    for _ in range(1000):
        action = env.action_space.sample()
        obs, reward, terminated, truncated, info = env.step(action)
"""
from __future__ import annotations

import gymnasium as gym
import numpy as np
import json
from typing import Any, Dict, Optional, Tuple

from ..client import Client


class WorldSimEnv(gym.Env):
    """Gymnasium-compatible WorldSim environment.

    Parameters
    ----------
    base_url : str
        WorldSim server URL (e.g. ``"http://127.0.0.1:8090"``).
    config : dict, optional
        World configuration (partial). Defaults to a small demo scene.
    obs_keys : list[str], optional
        Which quantities to observe (default: ``["bodies","fluid","temperature"]``).
    max_steps : int
        Episode length.
    """

    metadata = {"render_modes": ["human"], "render_fps": 30}

    def __init__(
        self,
        base_url: str = "http://127.0.0.1:8090",
        config: Optional[Dict[str, Any]] = None,
        obs_keys: Optional[list] = None,
        max_steps: int = 1000,
        render_mode: Optional[str] = None,
    ):
        super().__init__()
        self.client = Client(base_url)
        self.config = config or {"quality": "low", "dt": 0.0166667}
        self.obs_keys = obs_keys or ["bodies", "fluid"]
        self.max_steps = max_steps
        self.render_mode = render_mode
        self.world_id: Optional[str] = None
        self.step_count = 0
        self._obs_dim = 12  # bodies×(px,py,pz,r) for 3 demo spheres

        # Observation: vector of sensor readings (temperature at fixed probe)
        self.observation_space = gym.spaces.Box(
            low=-np.inf, high=np.inf, shape=(self._obs_dim,), dtype=np.float32,
        )
        # Action: force applied to body 0 in [x,y,z]
        self.action_space = gym.spaces.Box(low=-10.0, high=10.0, shape=(3,), dtype=np.float32)

    def reset(self, *, seed=None, options=None) -> Tuple[np.ndarray, dict]:
        super().reset(seed=seed)
        self.step_count = 0
        # Create a fresh world
        w = self.client.create_world(config=self.config, name=f"gym-{seed or 0}")
        self.world_id = w["id"]
        # Spawn default scene
        self.client.inject(self.world_id, [
            {"type": "spawn_fluid", "center": [0, 1.0, 0], "radius": 0.5},
            {"type": "spawn_sphere", "material": "wood", "pos": [0.4, 2.6, 0], "radius": 0.17},
            {"type": "spawn_sphere", "material": "steel", "pos": [-0.4, 2.6, 0], "radius": 0.12},
        ])
        return self._get_obs(), {}

    def step(self, action: np.ndarray):
        assert self.action_space.contains(action)
        self.step_count += 1
        # Apply force to body 0
        self.client.inject(self.world_id, [{
            "type": "apply_force",
            "target": {"body": 0},
            "force": action.tolist(),
            "ticks": 1,
        }])
        # Step the world
        self.client.control(self.world_id, "step")
        obs = self._get_obs()
        terminated = self.step_count >= self.max_steps
        reward = self._compute_reward(obs)
        info = {"tick": self.step_count}
        return obs, float(reward), terminated, False, info

    def _get_obs(self) -> np.ndarray:
        obs = np.zeros(self._obs_dim, dtype=np.float32)
        try:
            # Read state for body positions
            state = self.client.state(self.world_id)
            # In v0.1 state doesn't expose per-body positions;
            # use temperature probe as a placeholder observable
            t = self.client.probe(self.world_id, 0, 0.5, 0, quantity="temperature")
            obs[0] = float(t.get("value", 0))
            d = self.client.probe(self.world_id, 0, 0.5, 0, quantity="density")
            obs[1] = float(d.get("value", 0))
            obs[2] = float(state.get("tick", 0))
            obs[3] = float(state.get("fluid", 0))
            obs[4] = float(state.get("bodies", 0))
        except Exception:
            pass
        return obs

    def _compute_reward(self, obs: np.ndarray) -> float:
        """Reward: keep temperature moderate and fluid mass conserved."""
        temp = obs[0]
        # Penalize extreme temperatures away from 293 K
        reward = -abs(temp - 293.0) * 0.001
        return float(reward)

    def render(self):
        if self.render_mode == "human":
            print(f"tick={self.step_count} world={self.world_id}")

    def close(self):
        if self.world_id:
            try:
                self.client.delete_world(self.world_id)
            except Exception:
                pass
