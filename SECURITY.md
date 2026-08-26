# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| 0.1.x | ✅ |

## Reporting a vulnerability

Email the maintainers via a GitHub Security Advisory ([New Advisory](https://github.com/hyqzz/worldsim/security/advisories/new)) or open a **private** issue and request a security contact.

Please include a minimal reproduction and, if applicable, the snapshot or event log that triggers the issue.  Do not post exploit details in public issues.

We aim to acknowledge reports within 3 business days and to ship a fix or mitigation within 30 days.

## Scope

- The HTTP/WebSocket server (`ws-server`, `worldsim` binary): request parsing, WebSocket handshake, snapshot handling.
- Snapshot deserialization: malformed Base64 or out-of-range array lengths must not trigger panics or OOM — they are validated and return `400`.

The physics solvers are not a security boundary, but panics or resource exhaustion triggered by crafted API payloads are treated as security issues.

## Token hygiene

Personal access tokens must never be committed.  CI uses `GITHUB_TOKEN` with minimal scopes.  If you accidentally pushed a token, revoke it immediately and notify the maintainers so we can rotate any exposed secret from logs.
