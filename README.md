# Amigo

A native desktop translation client in pure Rust: DeepL over its hosted MCP
service, plus local or cloud Ollama models. Pick the provider for a translation
and switch whenever you want a second opinion. The UI is GPUI (Zed's GPU
renderer); there is no WebView and no local web server.

![Amigo](assets/icon.png)

## Features

- **DeepL** via OAuth 2.1 + PKCE and the MCP `translate-text` tool — sign in
  from the app, no API key
- **Ollama** against the local daemon, with TranslateGemma 4B/12B/27B, Gemma 4
  and DeepSeek V4.1 Flash (cloud) as models — local models are single-translation
  only, since multi sampling runs several requests at once; the default is the
  Gemma 4 cloud model
- Choose the pair in the top bar; the languages it offers come from a catalogue
  spanning DeepL and TranslateGemma, with regional variants falling back to their
  base language where a provider lacks them. Settings → Languages decides which
  of them appear in the menus.
- **Multi-sample mode**: several translations of one sentence, shown as
  word-level diffs against the most typical sample, with duplicate samples
  grouped
- Register and free-text context hints appended to Ollama prompts
- Portuguese (PT/PT-PT/PT-BR/PT-AO) and Spanish (ES/AR/CO/MX) pairs alongside
  English
- Copy per result, character limit, stale-result marker, error banner,
  `Cmd+Enter` to translate and `Cmd+,` for settings

## Run it

Needs a recent stable Rust toolchain, plus `ollama serve` if you use the local
provider.

```sh
cargo run -p amigo
```

DeepL signs in from the Settings window and opens a browser; Ollama needs only
the daemon and a pulled model:

```sh
ollama pull translategemma:4b
```

## Build the macOS app

```sh
./scripts/bundle-macos.sh     # builds dist/Amigo.app
./scripts/run-macos.sh        # builds it and opens it
```

The bundle script compiles the release binary, generates `AppIcon.icns` in pure
Python, writes `Info.plist`, and ad-hoc signs the result.

## Where state lives

| Path | Contents |
|---|---|
| `~/Library/Application Support/Amigo/preferences.json` | providers, options |
| `~/Library/Application Support/Amigo/mcp-session.json` | DeepL OAuth session (0600) |
| `~/Library/Application Support/Amigo/mcp-client.json` | DeepL client registration (0600) |
| `~/Library/Logs/Amigo/amigo.log` | log, rotated once at 1 MB |

Settings → **Copy diagnostics** grabs the app version, sign-in state, provider
configuration and the last 500 log lines.

## Project layout

```text
apps/amigo            GPUI application: AppState model, views, theme
crates/deepl-engine   worker thread: provider jobs, OAuth lifecycle, validation
crates/deepl-core     UI-agnostic: MCP client, OAuth, Ollama, pairs, config, diff
scripts/              icons and the macOS bundle
```

## How it fits together

The engine runs on its own thread and reports over an async channel; the UI
awaits those events, so nothing polls and the UI thread never waits on the
network or on the browser sign-in.

`AppState` (`apps/amigo/src/app/state.rs`) owns the engine, preferences, session
and results, and implements no rendering at all. Views hold it, read it while
rendering, and call commands on it to change anything. GPUI repaints a view when
an entity it read notifies, so every open window stays in sync without
observers; the one subscription is the main view reacting to the state events
that ask for a window to open (sign-in / setup).

## Languages

`crates/deepl-core/src/pairs.rs` is a catalogue, not a hand-written pair table.
Each entry records its identifier, the tag TranslateGemma uses, the DeepL codes
for the source and target roles (`None` where DeepL does not offer that role),
and the coarser variant to fall back to.

- **DeepL** offers 114 source languages and 121 targets, taken from the
  `get-source-languages` / `get-target-languages` tools DeepL exposes on the MCP
  server the app talks to. Portuguese and Chinese are read under their base codes
  (`PT`, `ZH`) and written under regional ones (`PT-BR`/`PT-PT`,
  `ZH-HANS`/`ZH-HANT`); English is read as `EN` and written as `EN-US`/`EN-GB`.
- **TranslateGemma** offers the 55 languages its model card benchmarks (the
  WMT24++ set) plus the same regional variants DeepL offers (Brazilian/European
  Portuguese, American/British English, Latin American Spanish, Canadian French,
  Swiss German, both Chinese scripts) **as targets**. A language with
  localisations is never offered as a target itself — `English` gives way to
  American/British, `Spanish` to Spain/Latin America — and localisations are never
  sources, so `FR-CA` resolves to `FR` when read. The much larger experimental tail the chat template accepts is
  deliberately not offered.

`Language::resolve` walks the fallback chain, so `PT-PT` as a source becomes `PT`
for DeepL and `ZH-HANT` becomes `ZH`. It also resolves the other way when a base
language is written: `PT` becomes `PT-BR`, `EN` would become `EN-US` and `ZH`
becomes `ZH-HANS` — the most widely used variant of each. The FROM and TO menus are built per
provider and role from that table, then filtered by whatever Settings -> Languages
has switched on.

The pair is stored on each provider but behaves as one setting: picking a
language applies it to the selected provider and stores the closest equivalent on
the other, so switching provider keeps your intent.

## Tests

```sh
cargo test --workspace
```

Covers config persistence and preset operations, the word-level diff, language
pair mapping and swap rules, PKCE and OAuth callback parsing, MCP JSON-RPC
handling, and Ollama prompt construction.
