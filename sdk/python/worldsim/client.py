"""WorldSim Python client — stdlib only (urllib + http.client + json)."""
from __future__ import annotations

import http.client
import json
import time
import urllib.parse
import urllib.request
import urllib.error
from typing import Any, Dict, Iterator, List, Optional


class WorldSimError(RuntimeError):
    def __init__(self, status: int, code: str, message: str, body: Any = None):
        super().__init__(f"[{status} {code}] {message}")
        self.status = status
        self.code = code
        self.message = message
        self.body = body


def _raise_for_payload(status: int, payload: Any):
    if isinstance(payload, dict) and "error" in payload:
        err = payload["error"]
        raise WorldSimError(status, err.get("code", "error"), err.get("message", ""), payload)
    raise WorldSimError(status, "http_error", f"HTTP {status}", payload)


class Client:
    """Thin, stdlib-only WorldSim client.

    Parameters
    ----------
    base_url:
        e.g. ``"http://127.0.0.1:8090"``.
    timeout:
        per-request socket timeout in seconds.
    """

    def __init__(self, base_url: str = "http://127.0.0.1:8090", timeout: float = 12.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    # ---- low-level ----

    def _request(self, method: str, path: str, body: Optional[Dict[str, Any]] = None) -> Any:
        url = f"{self.base_url}{path}"
        data = None
        headers = {"Accept": "application/json"}
        if body is not None:
            data = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                raw = resp.read()
                if not raw:
                    return None
                try:
                    return json.loads(raw.decode("utf-8"))
                except Exception:
                    return raw
        except urllib.error.HTTPError as e:
            raw = e.read()
            try:
                payload = json.loads(raw.decode("utf-8")) if raw else {}
            except Exception:
                payload = {"raw": raw.decode("utf-8", errors="replace")}
            _raise_for_payload(e.code, payload)
        except urllib.error.URLError as e:
            raise WorldSimError(0, "connection_failed", str(e))

    def _get(self, path: str, params: Optional[Dict[str, Any]] = None) -> Any:
        if params:
            qs = urllib.parse.urlencode({k: v for k, v in params.items() if v is not None})
            path = f"{path}?{qs}" if qs else path
        return self._request("GET", path)

    # ---- worlds ----

    def create_world(self, config: Optional[Dict[str, Any]] = None, name: Optional[str] = None) -> Dict[str, Any]:
        body: Dict[str, Any] = {}
        if config is not None:
            body["config"] = config
        if name is not None:
            body["name"] = name
        return self._request("POST", "/v1/worlds", body)

    def list_worlds(self) -> List[Dict[str, Any]]:
        data = self._get("/v1/worlds")
        return data.get("worlds", []) if isinstance(data, dict) else []

    def get_world(self, wid: str) -> Dict[str, Any]:
        return self._request("GET", f"/v1/worlds/{urllib.parse.quote(wid)}")

    def delete_world(self, wid: str) -> Dict[str, Any]:
        return self._request("DELETE", f"/v1/worlds/{urllib.parse.quote(wid)}")

    def state(self, wid: str) -> Dict[str, Any]:
        return self._request("GET", f"/v1/worlds/{urllib.parse.quote(wid)}/state")

    # ---- control / precision ----

    def control(self, wid: str, action: str, rate: Optional[float] = None) -> Dict[str, Any]:
        body: Dict[str, Any] = {"action": action}
        if rate is not None:
            body["rate"] = rate
        return self._request("POST", f"/v1/worlds/{urllib.parse.quote(wid)}/control", body)

    def pause(self, wid: str) -> Dict[str, Any]:
        return self.control(wid, "pause")

    def resume(self, wid: str) -> Dict[str, Any]:
        return self.control(wid, "resume")

    def step(self, wid: str) -> Dict[str, Any]:
        return self.control(wid, "step")

    def set_precision(self, wid: str, quality: Optional[str] = None, budget_ms: Optional[float] = None) -> Dict[str, Any]:
        body: Dict[str, Any] = {}
        if quality is not None:
            body["quality"] = quality
        if budget_ms is not None:
            body["budget_ms"] = budget_ms
        return self._request("PUT", f"/v1/worlds/{urllib.parse.quote(wid)}/precision", body)

    # ---- events / probes ----

    def inject(self, wid: str, events: List[Dict[str, Any]]) -> Dict[str, Any]:
        return self._request("POST", f"/v1/worlds/{urllib.parse.quote(wid)}/events", {"events": events})

    def probe(self, wid: str, x: float, y: float, z: float, quantity: str = "temperature", r: Optional[float] = None) -> Dict[str, Any]:
        params: Dict[str, Any] = {"x": x, "y": y, "z": z, "quantity": quantity}
        if r is not None:
            params["r"] = r
        return self._get(f"/v1/worlds/{urllib.parse.quote(wid)}/probe", params)

    # ---- snapshots / fork ----

    def snapshot_create(self, wid: str) -> Dict[str, Any]:
        return self._request("POST", f"/v1/worlds/{urllib.parse.quote(wid)}/snapshots")

    def snapshot_list(self, wid: str) -> List[Dict[str, Any]]:
        data = self._request("GET", f"/v1/worlds/{urllib.parse.quote(wid)}/snapshots")
        return data.get("snapshots", []) if isinstance(data, dict) else []

    def snapshot_get(self, wid: str, sid: str) -> Dict[str, Any]:
        return self._request("GET", f"/v1/worlds/{urllib.parse.quote(wid)}/snapshots/{urllib.parse.quote(sid)}")

    def restore(self, wid: str, snapshot_id: Optional[str] = None, snapshot: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        body: Dict[str, Any] = {}
        if snapshot_id is not None:
            body["snapshot_id"] = snapshot_id
        if snapshot is not None:
            body["snapshot"] = snapshot
        return self._request("POST", f"/v1/worlds/{urllib.parse.quote(wid)}/restore", body)

    def fork(self, wid: str, name: Optional[str] = None) -> Dict[str, Any]:
        body: Dict[str, Any] = {}
        if name is not None:
            body["name"] = name
        return self._request("POST", f"/v1/worlds/{urllib.parse.quote(wid)}/fork", body)

    # ---- streaming (SSE, stdlib) ----

    def watch(self, wid: str, max_frames: Optional[int] = None) -> Iterator[Dict[str, Any]]:
        """Yield live frames via SSE.

        Each frame is the same JSON object the WebSocket viewer receives:
        ``{\"t\":\"frame\",\"tick\":...,\"bodies\":{...},\"fluid\":{...},\"stats\":{...}}``.

        Parameters
        ----------
        max_frames:
            if given, stop after this many frames (useful in examples / tests).
        """
        parsed = urllib.parse.urlparse(self.base_url)
        host = parsed.hostname or "127.0.0.1"
        port = parsed.port or (443 if parsed.scheme == "https" else 80)
        path = f"/v1/worlds/{urllib.parse.quote(wid)}/stream?format=sse"
        conn = http.client.HTTPConnection(host, port, timeout=self.timeout)
        conn.request("GET", path, headers={"Accept": "text/event-stream"})
        resp = conn.getresponse()
        if resp.status != 200:
            body = resp.read().decode("utf-8", errors="replace")
            try:
                payload = json.loads(body) if body else {}
            except Exception:
                payload = {}
            _raise_for_payload(resp.status, payload)
        count = 0
        buf = ""
        try:
            while True:
                chunk = resp.read(8192)
                if not chunk:
                    break
                buf += chunk.decode("utf-8", errors="replace")
                while "\n\n" in buf:
                    raw, buf = buf.split("\n\n", 1)
                    # SSE: lines starting with data:
                    data_lines = [ln[5:].lstrip() for ln in raw.splitlines() if ln.startswith("data:")]
                    if not data_lines:
                        continue
                    data = "\n".join(data_lines).strip()
                    if not data:
                        continue
                    try:
                        frame = json.loads(data)
                    except Exception:
                        continue
                    yield frame
                    count += 1
                    if max_frames is not None and count >= max_frames:
                        return
        finally:
            try:
                conn.close()
            except Exception:
                pass

    # ---- convenience event builders ----

    @staticmethod
    def ev_spawn_sphere(material: str, pos, vel=None, radius: float = 0.15):
        return {"type": "spawn_sphere", "material": material, "pos": list(pos), "vel": list(vel or [0, 0, 0]), "radius": radius}

    @staticmethod
    def ev_spawn_fluid(center, radius: float):
        return {"type": "spawn_fluid", "center": list(center), "radius": radius}

    @staticmethod
    def ev_heat_source(pos, radius: float, power_watts: float, duration_ticks: int = 0):
        return {"type": "heat_source", "pos": list(pos), "radius": radius, "power_watts": power_watts, "duration_ticks": duration_ticks}
