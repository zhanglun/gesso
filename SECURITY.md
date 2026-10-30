# Security Policy

## Supported versions

| Version | Supported |
|---|---|
| `main` (0.1.x dev) | ✅ |
| tagged releases | as they exist |

## Reporting a vulnerability

Please use [GitHub private vulnerability reporting](https://github.com/zhanglun/gesso/security/advisories/new) for this repository.
If that's unavailable, open a normal issue asking for a contact channel — **do not include exploit details in the public issue**.

We aim to respond within 7 days. There is no bounty program; this is a spare-time project.

## Threat model

Gesso renders **untrusted content by design** — imported HTML/shader files are arbitrary code. The containment strategy:

1. **Webview sandbox** — local HTML and Wallpaper Engine web content runs in a WKWebView/WebView2 child view of a window we own. The iframe receives `allow-scripts allow-same-origin`, but no top-level navigation, popups, forms, or Rust IPC. The manager UI and wallpaper windows have disjoint capability scopes; wallpaper windows receive typed push events only.
2. **Read-only resource protocol** — local assets are served through `gesso://host`, `gesso://library/<entry-id>/…`, or `gesso://steam/<entry-id>/…`. Entry IDs are unguessable, paths are resolved only inside the selected entry directory, traversal is rejected, and the handler supplies MIME, CSP, and Range responses. Video playback uses a loopback HTTP media server rather than the custom scheme's media path.
3. **Remote pages** — users may explicitly import an `https://` URL. Gesso does not download it into the library or provide Rust commands to it; the page remains subject to the sandbox and CSP. Network requests made by the page are controlled by the browser/webview and the generated CSP, not by a Gesso downloader.
4. **Wallpaper Engine assets** — users must explicitly select an existing `project.json` or entry directory. Gesso does not scan Steam, download workshop content, or redistribute it (see [docs/WALLPAPER-ENGINE.md](docs/WALLPAPER-ENGINE.md)).

Out of scope: attacks requiring local code execution, compromised build infrastructure, or malicious macOS/Windows system components.
