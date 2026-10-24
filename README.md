# Translation Amigo

A native, pure-Rust desktop translation client. It signs in with your DeepL
account over OAuth and translates through DeepL's hosted MCP service — no API
key to paste. The UI is `egui` on `wgpu`; there is no WebView, no JavaScript, and
no local web server.

![Translation Amigo](assets/icon.png)

## What works

- **Sign in with DeepL** using OAuth 2.1 + PKCE in your system browser
- Translation through the MCP `translate-text` tool
- Two multi-line panes: source on the left, translation on the right
- Five language pairs:
  - Portuguese → English
  - English → Portuguese (Portugal)
  - English → Portuguese (Brazil)
  - English → Spanish
  - Spanish → English
- Swapping direction, with PT→EN swapping to EN→PT-PT (the reverse is ambiguous)
- Opt-in auto-translate with an 850 ms debounce
- Copy, clear, character count, stale-result indicator, error banners
- `Cmd+Enter` to translate, `Cmd+,` for account settings, `Esc` to close
- Access and refresh tokens stored in the macOS Keychain
- Responsive layout: content is capped and centered on wide windows, and the two
  panes stack when the window is narrow
- Light/dark theme following the system

## Architecture

```text
apps/translation-amigo   egui + eframe UI on wgpu (thin, no business logic)
        │
        │  Command / Event channels (std::sync::mpsc)
        ▼
crates/deepl-engine      dedicated worker thread: OAuth lifecycle, request
        │                validation, MCP calls, token refresh
        ▼
crates/deepl-core        UI-agnostic: MCP client, OAuth flow, language pairs,
                         errors, Keychain access
```

Design rules:

1. **Engine first.** Everything the UI does is a `Command`; everything it learns
   is an `Event`. Network I/O and the browser sign-in never run on the UI thread.
2. **The UI is disposable.** `deepl-core` has no `egui` dependency, so replacing
   the UI only means rewriting `apps/`.
3. **Stale work is discarded.** Every translation carries a request id; responses
   that no longer match the current request are ignored.
4. **The backend re-validates.** The five supported pairs and the per-request
   character limit are enforced in Rust, not just hidden in the UI.

### Sign-in flow

1. Fetch the OAuth server metadata from `mcp.deepl.com`.
2. Open a loopback listener on `127.0.0.1:53682` (falling back to ports
   53683–53686) and register a native client via dynamic client registration.
3. Open the authorization page in your browser with PKCE `S256`, a random state,
   `resource=https://mcp.deepl.com/v1/mcp`, and the `offline_access` scope.
4. Receive the authorization code on the loopback callback, exchange it for
   tokens, and store the session in the Keychain.
5. Initialize the MCP session, read `tools/list`, and adapt the
   `translate-text` arguments to the schema the server advertises.

Tokens are refreshed before expiry. Signing out revokes the token and removes the
session from the Keychain; the client registration is kept for next time.

The REST API client in `crates/deepl-core/src/api.rs` is retained for a possible
future API-key backend, but it is **not wired into the app**.

## Run it

Requirements: a recent stable Rust toolchain (built with Rust 1.100), and Python 3
only for regenerating icons.

```sh
cargo run -p translation-amigo
```

On first launch the account dialog opens. Choose **Sign in with DeepL**, complete
the flow in your browser, and return to the app.

## Build a macOS app

```sh
./scripts/bundle-macos.sh
open "dist/Translation Amigo.app"
```

The script builds the release binary, generates `AppIcon.icns` in pure Python,
writes `Info.plist`, and ad-hoc signs the bundle. The result is a ~9 MB app in
`dist/`.

## Limits

- DeepL's MCP Free tier accepts **5,000 characters per translation request**. The
  app enforces that limit and reports it clearly.
- Seat-based plans allow up to 128,000 characters, but the app does not yet
  detect the plan, so the conservative limit applies.
- The MCP service does not expose a `/usage` endpoint, so there is no quota meter.
- Only `translate-text` is used. Rephrase, correct, document translation, and
  glossary/style tools are not wired up yet.

## Logging and diagnostics

- Logs are written to `~/Library/Logs/Translation Amigo/translation-amigo.log` (rotated
  once at 1 MB to `translation-amigo.log.old`).
- Default logging records the sign-in lifecycle, session load, translation
  summaries, and errors — including JSON-RPC codes, error details, HTTP status,
  and DeepL's `X-Trace-ID`.
- **Verbose logging** in the account dialog additionally records MCP requests,
  protocol/session details, the available tools, and the `translate-text`
  schema.
- Tokens, authorization codes, OAuth state/PKCE values, source text, and
  translations are never written to the log.
- **Copy diagnostics** copies the app version, OS, sign-in state, log path, and
  the last 500 log lines to the clipboard.

## Tests

```sh
cargo test --workspace
```

The core tests cover language-code mapping, the supported-pair allowlist, swap
behavior, PKCE generation, OAuth callback parsing, MCP JSON-RPC id matching,
hosted and legacy tool schemas, and translation-result parsing.

## Project layout

```text
crates/deepl-core/
  src/mcp/auth.rs     OAuth discovery, PKCE, loopback callback, tokens
  src/mcp/client.rs   MCP initialize, tools/list, tools/call, SSE parsing
  src/mcp/model.rs    session and status types
  src/api.rs          REST client (shelved backend)
  src/pairs.rs        languages, supported pairs, swap rules
  src/error.rs        ApiError + ErrorKind
  src/keychain.rs     OS credential store
  src/types.rs        REST key status (shelved backend)

crates/deepl-engine/
  src/lib.rs          Command/Event engine on its own thread

apps/translation-amigo/
  src/main.rs         eframe entry point
  src/app.rs          UI, state, shortcuts, account modal, diagnostics
  src/logging.rs      file logger + in-memory ring buffer
  src/theme.rs        light/dark palette and egui style

scripts/
  make_icon.py        procedural 1024px source icon
  make_icons.py       .icns + .iconset generation
  bundle-macos.sh     .app bundle
```

## Roadmap

- Chunk long text on paragraph boundaries for the 5,000-character MCP limit
- Detect the account plan and allow larger requests on seat-based plans
- Optionally expose `rephrase-text` and `correct-text`
- Glossary and style-rule tools for seat-based plans
- Revisit the REST API-key backend if a key-based mode is wanted again
