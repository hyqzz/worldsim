"""WorldSim Python SDK — stdlib only."""

from .client import Client  # noqa: F401

__all__ = ["Client"]

try:
    from importlib.metadata import version as _v
    __version__ = _v("worldsim")
except Exception:
    __version__ = "0.1.0"
