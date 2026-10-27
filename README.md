# Amigo

A native, pure-Rust desktop translation client with two providers: DeepL over
its hosted MCP service, and a local Ollama TranslateGemma model. Enable either
or both; when both are enabled, translations appear side by side for comparison.
The UI is `egui` on `wgpu`; there is no WebView, no JavaScript, and no local web
server.

![Amigo](assets/icon.png)

## What works

- **Provider checkboxes** for DeepL and Ollama, each with its own language rows
- **DeepL** using OAuth 2.1 + PKCE and the MCP `translate-text` tool
- **Ollama** using the local daemon, with TranslateGemma 4B/12B/27B and
  DeepSeek V4.1 Flash cloud as selectable models
- **Register control** for Ollama: Neutral, Casual, Formal, Conversational, Academic
- **Context hint** for Ollama: free-text situation notes such as “flirty
  conversation between friends”, appended to the prompt
- Both providers can run at once, with independently labelled results
- Two multi-line panes: source on the left, translations on the right
- Five language pairs per provider:
  - Portuguese → English
  - English → Portuguese (Portugal)
  - English → Portuguese (Brazil)
  - English → Spanish
  - Spanish → English
- Swapping direction per provider, with PT→EN swapping to EN→PT-PT
- Copy, clear, character count, stale-result indicator, error banners
- `Cmd+Enter` to translate, `Cmd+,` for settings, `Esc` to close
- DeepL tokens stored in a local 0600 session file for the prototype; Ollama needs no credentials
- Responsive layout: content is capped and centered on wide windows, and the two
  panes stack when the window is narrow
- Light/dark theme following the system

## Architecture

```text
apps/amigo               egui + eframe UI on wgpu (thin, no business logic)
        │
        │  Command / Event channels (std::sync::mpsc)
        ▼
crates/deepl-engine      dedicated worker thread: provider jobs, OAuth lifecycle,
        │                request validation, MCP and Ollama calls
        ▼
crates/deepl-core        UI-agnostic: DeepL MCP client, OAuth flow, local Ollama
                         client, language pairs, errors, local session storage
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
   tokens, and store the session in a 0600 local file.
5. Initialize the MCP session, read `tools/list`, and adapt the
   `translate-text` arguments to the schema the server advertises.

Tokens are refreshed before expiry. Signing out revokes the token and removes the
session file; the client registration is kept for next time.

The REST API client in `crates/deepl-core/src/api.rs` is retained for a possible
future API-key backend, but it is **not wired into the app**.

**Credential storage:** this prototype writes the DeepL session and OAuth client
registration to `~/Library/Application Support/Amigo/` with `0600`
permissions. That avoids macOS Keychain invalidation on every ad-hoc rebuild. A
release build should move this back into the Keychain (or another OS credential
store) with a stable signing identity.

### Ollama flow

1. Make sure the Ollama daemon is running (`ollama serve`, or the Ollama app).
2. Install a model, for example `ollama pull translategemma:4b`. Cloud models
   already available in your Ollama account, such as `deepseek-v4.1-flash:cloud`,
   appear directly in the model list.
3. The app checks `http://127.0.0.1:11434/api/tags` and enables the installed
   selectable models in Settings.
4. Translation uses `POST /api/chat` with `temperature: 0`, the chosen register,
   and an optional context hint. No API key or cloud account is managed by the app.

Model prompts contain language names and codes plus generic register/context
instructions. They contain no language-specific dialect directives, so the same
prompt structure works for the other languages TranslateGemma supports.

Cloud models (`:cloud`) are routed through Ollama Cloud. The UI notes when the
selected model leaves the machine; text and context hints are sent to that
service. Local models stay local.

## Run it

Requirements: a recent stable Rust toolchain (built with Rust 1.100), Python 3
only for regenerating icons, and Ollama for the local provider.

```sh
cargo run -p amigo
```

On first launch, enable the providers you want. DeepL opens a browser sign-in;
Ollama only needs the local daemon and an installed TranslateGemma model.

```sh
ollama serve
ollama pull translategemma:4b
```

## Build a macOS app

```sh
./scripts/bundle-macos.sh
open "dist/Amigo.app"
```

The script builds the release binary, generates `AppIcon.icns` in pure Python,
writes `Info.plist`, and ad-hoc signs the bundle. The result is a ~9 MB app in
`dist/`.

## Limits

- Both providers are capped at **5,000 characters per translation request** in
  this build.
- DeepL seat-based plans allow up to 128,000 characters, but the app does not yet
  detect the plan.
- The MCP service does not expose a `/usage` endpoint, so there is no quota meter.
- Only text translation is used. Rephrase, correct, document translation, and
  glossary/style tools are not wired up yet.
- Ollama requests are sequential when both providers are enabled; the second
  provider waits for the first result.
- The register and model are persisted; the context hint is intentionally
  session-only and is never written to logs.

## Logging and diagnostics

- Logs are written to `~/Library/Logs/Amigo/amigo.log` (rotated once at 1 MB
  to `amigo.log.old`).
- Default logging records the sign-in lifecycle, session load, translation
  summaries, and errors — including JSON-RPC codes, error details, HTTP status,
  and DeepL's `X-Trace-ID`.
- **Verbose logging** in Settings additionally records MCP and Ollama requests,
  protocol/session details, available models/tools, and the `translate-text`
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
hosted and legacy tool schemas, generic translation prompt construction,
register/context handling, and Ollama model discovery.

## Project layout

```text
crates/deepl-core/
  src/mcp/auth.rs     OAuth discovery, PKCE, loopback callback, tokens
  src/mcp/client.rs   MCP initialize, tools/list, tools/call, SSE parsing
  src/mcp/model.rs    session and status types
  src/ollama.rs       local Ollama client, TranslateGemma prompt, model discovery
  src/provider.rs     provider identifiers and shared limits
  src/api.rs          REST client (shelved backend)
  src/pairs.rs        languages, supported pairs, swap rules
  src/error.rs        ApiError + ErrorKind
  src/session_store.rs  local 0600 session storage
  src/types.rs        REST key status (shelved backend)

crates/deepl-engine/
  src/lib.rs          Command/Event engine on its own thread

apps/amigo/
  src/main.rs         eframe entry point
  src/app.rs          UI, provider state, shortcuts, settings, diagnostics
  src/logging.rs      file logger + in-memory ring buffer
  src/theme.rs        light/dark palette and egui style

scripts/
  make_icon.py        procedural 1024px source icon
  make_icons.py       .icns + .iconset generation
  bundle-macos.sh     .app bundle
```

## Roadmap

- Run enabled providers in parallel instead of sequentially
- Chunk long text on paragraph boundaries for the 5,000-character limit
- Expose TranslateGemma's full language list rather than the curated five pairs
- Detect the DeepL plan and allow larger requests on seat-based plans
- Optionally expose DeepL `rephrase-text` and `correct-text`
- Glossary and style-rule tools for seat-based plans
- Revisit the REST API-key backend if a key-based mode is wanted again
