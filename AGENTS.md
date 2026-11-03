# Notes for agents working on this repo

Amigo is a GPUI translation client. This file records the things that cost real
time to discover, so they don't have to be rediscovered.

## Repo and commands

- Remote: `https://github.com/stephenjjbrown/amigo.git`, branch `main`.
- The local checkout directory may still be called `deepl-client`; nothing in the
  code depends on that name.
- `cargo run -p amigo` runs the app. `./scripts/bundle-macos.sh` builds
  `dist/Amigo.app` (`com.stephen.amigo`); `./scripts/run-macos.sh` builds and
  opens it.
- Screenshots of windows are possible here: screen-recording permission is
  granted. List window ids with `CGWindowListCopyWindowInfo` (a small Swift
  script), then `screencapture -x -o -l<id> out.png`. `screencapture` without
  `-l` also works. To capture a dialog, temporarily open it from
  `AmigoApp::new` (via `App::defer`, as `open_dialog` does) and revert after.
  Driving clicks with `CGEvent` needs **accessibility** permission, which is not
  granted: hover states, menus and popovers cannot be exercised that way.

## The GPUI stack

- One dependency: `gpui-kit = "0.8"`, which pins `gpui-pre 0.3.10` (a snapshot of
  Zed), `gpui-component 0.8.1` and `gpui-pre-platform`. Do not hand-wire the
  `gpui-pre-*` packages or alias them in our manifest; the kit exists to own
  that, including the exact pins.
- This machine has **no Xcode Metal toolchain**. The build works because the
  platform dependency enables GPUI's `runtime_shaders` feature, which compiles
  the Metal shaders at launch. Upstream's `build.rs` has a TODO to deprecate
  that feature. If it ever goes away: `xcodebuild -downloadComponent
  MetalToolchain`, or the `macos-blade` feature.
- **Creating a window from a listener must go through `App::defer`.** GPUI
  renders a new window immediately, and its view reads the one being updated, so
  a synchronous `open_window` is a double lease and a panic.
  `Context::defer_in` and `Context::on_next_frame` do **not** help: they re-enter
  an update of the same view. `app/windows.rs::open_dialog` implements this
  correctly — use it for any new dialog.
- **A window's root container must set `.text_color(...)` and
  `.font_family(...)`.** Text style inherits down the element tree, and the
  crate's `Input`, `Switch`, `Slider` and `Textarea` depend on that inheritance.
  The "black text in dialogs" bug was exactly this: the main window is wrapped
  in `gpui_kit::open_window`, which mounts a `Root` that sets the foreground,
  while our dialogs did not. It was not a theme problem — the theme's colours
  were correct all along. `ui::window_root` / `ui::dialog_root` are now the only
  places that set it; do not restate the family or colour further down.
- **`svg()` does not inherit.** Text inheritance reaches text elements but not an
  `Svg`: it paints nothing unless given a text colour of its own. `ui::icon`
  therefore always takes a colour — passing `palette.text_secondary` beside a
  control's own `text_color` is deliberate, not redundant.
- **`.hover(...)` may be set only once per element.** Calling it twice panics with
  "hover style already set", so a variant control must build its own chain
  rather than layering a second hover on a shared recipe.
- `Theme::update(cx, |theme| ...)` is what reconciles the theme's derived
  tokens with `theme.colors`. Assigning through `Theme::global_mut` skips that.
- **Reacting: `notify` is the whole mechanism.** GPUI records which entities a
  view reads while rendering and repaints that view when any of them notifies.
  So a view needs **no `cx.observe` merely to stay current** — an open Settings
  window updates when the state changes because it read the state. `cx.observe`
  and `cx.subscribe` are for *side effects* (open a window, log, write a file).
  The one subscription in this app is `AmigoApp` reacting to `AppEvent`.
  There is no automatic change detection: mutating state without `cx.notify()`
  leaves the UI stale. Reads inside listeners create no dependency; only reads
  during render do.

### gpui-component 0.8 specifics

- `Button::dropdown_caret(true)` both adds the caret **and** switches the row to
  `justify_between`, which is how the label ends up left-aligned. Without it the
  label is centred.
- `Size` ladder is 20 / 24 / 32 / 44 px; `Medium` (the default) is our 32 px
  control height. `.small()` is 24 px, so pin height when using it.
- Menu items are `PopupMenuItem` (`MenuItem` is the accesskit role enum):
  `PopupMenuItem::element(...)` for custom rows.
- Component modules are not all re-exported at the crate root: `Checkbox`,
  `Slider` and `Switch` live under `component::checkbox|slider|switch`, and
  `Tooltip` under `component::tooltip`.
- `InputState`, `TextareaState` and `SliderState` are entities: construct with
  `cx.new(|cx| ...)` and read values back on submit. They notify themselves when
  they change, so a view that reads one repaints without an observer.
- `ThemeColor` is `#[non_exhaustive]` — assign field by field (with
  `#[allow(clippy::field_reassign_with_default)]`), never as a struct literal.

## App structure

```text
apps/amigo/src/app/state.rs    AppState: the model (engine, prefs, results). No Render.
apps/amigo/src/app/mod.rs      the main window: panes, cards, action bar
apps/amigo/src/app/windows.rs  open_dialog + dialog launchers (App::defer rule)
apps/amigo/src/settings.rs     Settings window
apps/amigo/src/provider.rs     Add/Edit provider window
apps/amigo/src/dialogs.rs      name prompt and delete confirmation
apps/amigo/src/ui.rs           component vocabulary (controls, roots, labels…)
apps/amigo/src/format.rs       text derived from a slot (card title/subtitle)
apps/amigo/src/theme.rs        palette, type scale, radii, crate theme mapping
apps/amigo/src/assets.rs       AssetSource over assets/icons/*.svg
crates/deepl-core/src/config.rs  Preferences, ProviderSlot, Preset, JSON I/O
crates/deepl-core/src/diff.rs    word-level diff used by the result cards
```

## State and views

- One model, `AppState` (`app/state.rs`), owns the engine, preferences, session,
  Ollama status and results. It implements **no `Render`** and holds **no window
  handles**, so it cannot depend on a view.
- Views hold `Entity<AppState>` and read it during render; they never hold a
  second copy of its data. Window handles live on `AmigoApp` because they belong
  to the view layer.
- State changes are commands (`sign_in`, `save_provider`, `load_preset`, …).
  Views call them with `state.update(cx, |state, cx| ...)`.
- When state needs something only a view can do — opening a window — it emits
  `AppEvent` and `AmigoApp` handles it in `subscribe`. Do not add a window handle
  or an `open_*` call to state.
- Keep `cx.notify()` at the end of every command that mutates. It is what
  repaints every window showing the state.
- The engine delivers events over an `async_channel`; `AppState::new` awaits
  `engine.events()` in one `cx.spawn` task and calls `handle` per event. **Do not
  reintroduce a polling timer.** The engine owns no UI knowledge and takes no
  repaint callback.
- Reading `AmigoApp` while it is being updated panics (`cannot read … while it is
  already being updated`). Inside `open_dialog` that read must sit **inside** the
  `cx.defer` closure, not before it.

## Design conventions

- Appearance follows the app's original egui design, but implemented the GPUI
  way. Do not reintroduce egui idioms (full-width bars inside a column, in-page
  titles duplicating the window title).
- Booleans are `Switch`es; continuous values are `Slider`s; small integer counts
  are − / + steppers. Compact actions sit in a row, not stretched across it.
- Icon-only buttons need a tooltip **and** an accesskit label — `ui::labeled`
  does both; `ui::icon_button`, `ui::circle_icon_button` and
  `ui::menu_action` apply it for you.
- Keep font family and text colour at window roots; call sites set only size and
  colour. New screens start from `ui::window_root` or `ui::dialog_root`.
- Build controls from the shared recipes in `ui.rs` (`control`, `icon_control`,
  `text`, `heading`, `footer`, `error_card`, `error_banner`) rather than
  re-deriving the same `div()` chain; the dialog footer, the pane header and the
  danger wash each used to exist three times over.

## Known traps

- `ProviderJob::Ollama { model }` takes `model.id()` (`gemma4:cloud`), never
  `model.label()` (`Gemma 4 31B (cloud)`).
- Ollama's Settings count must be *supported* models
  (`OllamaModel::ALL` filtered by `status.can_use`), not `status.models.len()`.
- Preferences live in `preferences.json` only. The import of eframe's `app.ron`
  was removed deliberately; there is no legacy migration to maintain.
- Tokens are stored in `~/Library/Application Support/Amigo/` with 0600, not the
  Keychain — ad-hoc signing invalidated Keychain items.

## Editing discipline

Scripted edits in this codebase have repeatedly failed silently: an anchor that
didn't match (no-op, but the file compiled because nothing changed), or a range
that cut the wrong block (duplicated docs, deleted a neighbouring function).
Always `assert` anchors, and afterwards `grep` for a symbol you expect to exist
rather than trusting a successful compile.

## Open work

- Provider dialog value readouts are plain numbers rather than editable boxes.
- No "Model library" link to `ollama.com/library` (no external-link icon yet).
