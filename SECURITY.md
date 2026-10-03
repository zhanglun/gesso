# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| `main` (0.1.x dev) | ✅ |
| tagged releases | as they exist |

## Reporting a vulnerability

Please use [GitHub private vulnerability reporting](../../security/advisories/new) for this repository.
If that's unavailable, open a normal issue asking for a contact channel — **do not include exploit details in the public issue**.

We aim to respond within 7 days. There is no bounty program; this is a spare-time project.

## Threat model

Gesso renders **untrusted content by design** — imported HTML/shader files are arbitrary code. The containment strategy:

1. **Webview sandbox** — wallpaper content runs in a WKWebView/WebView2 child view of a window we own. It has **no IPC into the Rust process** (the manager UI and wallpaper windows have disjoint capability scopes; wallpaper windows receive push events only).
2. **Filesystem** — wallpaper content is served from its own self-contained entry directory (`library/<random-id>/`). The intended end-state is a read-only custom scheme (`gesso://`) with path-traversal protection, CSP header injection and a navigation allow-list.
   - **Current known gap (being tracked):** entries are currently loaded via `file://` from the library directory while the custom-scheme handler is non-functional on our current webview stack ([docs/ENGINEERING-NOTES.md](docs/ENGINEERING-NOTES.md) pitfall list). Treat this as a hardening item, not a broken promise: content still cannot call into the app.
3. **Network** — wallpaper pages are expected to be local; remote-content wallpapers are not supported at this stage.
4. **Wallpaper Engine assets** — only ever read from the local workshop directory of a Steam installation the user already has. No downloader, no redistribution (see [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)).

Out of scope: attacks requiring local code execution, compromised build infrastructure, or malicious macOS/Windows system components.
