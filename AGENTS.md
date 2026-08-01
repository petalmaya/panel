# AGENTS.md

Guidance for AI coding agents (and humans) working in this repo. This is a
fork of [VibePanel](https://github.com/prankstr/vibepanel) — a Wayland
panel/status bar written in Rust + GTK4. If something here conflicts with
what you observe in the code, trust the code and update this file.

## Layout

```
crates/
  vibepanel-core/   # config parsing, theming, error types — no GTK deps
  vibepanel/        # the actual app: widgets, services, bar management
    src/widgets/           # one file per widget/popover
    src/widgets/css/       # one CSS module per widget/popover, aggregated in css/mod.rs
    src/widgets/quick_settings/  # QS panel + its cards/rows (reusable ListRow builder lives here)
    src/services/           # process-wide singleton services (system integration)
config.toml          # example/reference user config
docs/architecture.md  # longer-form architecture notes — read this too
docs/ui-regression-tests.md
```

`vibepanel-core` has no GTK dependency on purpose — keep it that way so it
stays testable without a display server.

## Build / test / lint

```sh
cargo build --release -p vibepanel   # or: ./run-debug.sh (build + run + tee log)
cargo test                           # regular suite, no windows presented
scripts/run-ui-regression-tests.sh   # window-presenting UI regression tests (needs xvfb-run)
cargo test -p vibepanel layer_shell -- --ignored --test-threads=1  # layer-shell contracts; needs a real Wayland compositor with layer-shell, not run in CI
cargo clippy
cargo fmt
```

There's no GTK4 / gtk-layer-shell dev environment in every sandbox — if you
can't build, say so rather than guessing; don't claim a change compiles that
you haven't actually built. Check gtk4-rs / gio API signatures (e.g. via
docs.rs) before relying on memory for anything beyond the very common
widgets, since it's easy to get exact trait/method signatures subtly wrong.

Pre-commit runs `cargo test --all`, clippy, rustfmt, and the UI regression
script when `xvfb-run` is available.

## Widget conventions

Every widget lives in `src/widgets/<name>.rs` and follows this shape:

- `<Name>Config` struct + `impl WidgetConfig for <Name>Config` — parses a
  `WidgetEntry` from `config.toml`. Call `warn_unknown_options(name, entry,
  KNOWN_OPTIONS)` first so typos in user config get logged instead of
  silently ignored. Give every option a sensible default; a widget with no
  config should render something reasonable.
- `<Name>Widget::new(cfg) -> Self`, wrapping a `BaseWidget` (in `base.rs`).
  `BaseWidget::new(&["css-class"])` gives you the container, ripple, hover
  state, and click-handler plumbing for free.
- `widget()` returning `&gtk4::Box` — this is what gets mounted on the bar.
- Register the widget in `widgets/mod.rs`: `mod <name>;`, `pub use
  <name>::{...}`, and a `"<name>" => { ... }` arm in the factory match.

### Popovers / menus

Don't hand-roll a `GestureClick` + your own popover window. Use
`BaseWidget::create_menu(|| -> Widget { ... })` — it wires up:

- left-click toggle (open/close) on the trigger widget
- ripple + hover CSS state
- the underlying `LayerShellPopover` (open/close animation, ESC-to-close,
  click-outside dismiss, deferred keyboard nav that kicks in on the first
  Tab/arrow press)

The builder closure runs fresh on every `show()` unless you call
`menu_handle.set_reuse_content(true)` (see `battery.rs`) — prefer fresh
rebuilds for anything whose content should reflect current state when
opened (like the launcher's app list), and `reuse_content` only for
popovers that manage their own internal state/animations across opens.

To close the currently-open popover from inside a row/button click handler
(e.g. "launch this app and then close"), call
`crate::popover_tracker::PopoverTracker::global().dismiss_active()` — you
don't need a handle to the specific `MenuHandle`.

If you need a focused text entry the moment a popover opens, don't
`grab_focus()` at build time (the widget isn't mapped yet) — connect to
`entry.connect_map(...)` instead. But note `connect_map` fires *synchronously
inside* `window.present()`, and `LayerShellPopover::show_internal` clears
whatever focus `present()` assigned right after it returns (via
`prepare_keyboard_nav()`, so focus rings stay hidden until the first
Tab/arrow press) — a direct `grab_focus()` in the `connect_map` handler gets
silently wiped out. Defer it to the next main-loop tick instead:
`entry.connect_map(|e| { let e = e.clone(); glib::idle_add_local_once(move ||
e.grab_focus()); });`. See `launcher_popover.rs`'s search entry.

`GtkListBoxRow::connect_activate` is a *keybinding* signal — it only fires
for Enter/Space on a focused row, **not** for a mouse click. A click only
emits the parent `ListBox`'s own `row-activated`, which fires for both
click and keyboard ("by key or not" per the GTK docs). Use
`list_box.connect_row_activated(...)` as your single handler for "the user
picked this row" rather than per-row `connect_activate`.
**Do not** try to bridge the two by calling `row.activate()` from inside a
`row-activated` handler — `ListBoxRow`'s default handler for its own
`activate` signal (when parented in a `ListBox`) re-emits `row-activated`,
so that bridge creates infinite mutual recursion and reliably stack-overflows
the process the first time a row fires. If each row needs its own
per-row data (e.g. "launch this app"), keep a `Vec<(ListBoxRow, T)>` (or
similar) populated alongside the rows and look it up by row identity
(`glib::Object`/widget types implement `PartialEq` by pointer) inside the
single `row-activated` handler. See `launcher_popover.rs`.

### Reusable pieces worth knowing about before you rebuild them

- `quick_settings::components::ListRow` — builder for icon + title +
  subtitle (+ optional trailing widget) rows, already styled with
  `styles::row::QS*`. Used well beyond the QS panel itself (e.g. the app
  launcher) — it's the standard "clickable row in a popover list" widget.
- `quick_settings::ui_helpers::{create_qs_list_box, clear_list_box,
  add_placeholder_row}` — list box setup/reset helpers.
- `services::icons::IconsService` — themed icon resolution, including
  app-id → icon-name lookups used by the taskbar. For raw `gio::AppInfo`
  icons (arbitrary themed name or file path), prefer
  `gtk4::Image::from_gicon(&app_info.icon())` over resolving to a name
  string first — it's simpler and handles both cases.
- `services::config_manager::ConfigManager::global().theme_sizes()` — use
  `pixmap_icon_size` to size any raw `GtkImage` app/window icon so it
  matches the taskbar/media/tray sizing instead of guessing a pixel value.

## CSS conventions

- CSS lives in `src/widgets/css/<name>.rs`, one file per widget/popover
  concern, each exposing `pub fn css() -> &'static str` (or `fn css(flag)`
  if it needs to vary, e.g. by animation setting). Register new modules in
  `widgets/css/mod.rs`: add `mod <name>;`, call it, and fold its output into
  the `format!()` at the bottom.
- CSS class name *constants* live in `styles.rs`, grouped into `pub mod`
  blocks per surface (e.g. `styles::row`, `styles::launcher_popover`).
  Reference these constants from widget code (`add_css_class(...)`) instead
  of hardcoding string literals, and reference the same literal class names
  in the CSS itself — grep `styles.rs` before inventing a new class, a
  generic one (`row::QS`, `card::BASE`, `state::CLICKABLE`, `surface::*`)
  often already exists.
- `LayerShellPopover` auto-adds `.popover`, `.vp-surface-popover`, and
  `.<widget_name>-popover` to whatever your builder returns — you don't
  need to add that last class yourself if your widget's primary CSS class
  matches (e.g. widget name `launcher` → auto class `launcher-popover`).
- Don't put a `color:`/`font-size:` rule in your CSS on top of a widget
  that already has a generic color class (`styles::color::MUTED`, etc.)
  applied in Rust — they fight for specificity. And remember `font-size`
  does nothing for a `GtkImage`; size those with `set_pixel_size()` in Rust.

## Config (`config.toml`)

Options are read via `entry.options.get("key")` returning a `toml::Value`;
use `.as_str()`, `.as_integer()`, `.as_float()`, `.as_bool()` as
appropriate (see `media.rs`/`taskbar.rs` for the float-or-integer fallback
pattern). Keep `config.toml` at the repo root in sync as a working example
whenever you add or rename widget options — it's the first thing a new
contributor (human or agent) reads.

## Testing widgets

Widget/popover tests commonly live inline (`#[cfg(test)] mod tests`) and
use helpers from `ui_regression_test_support.rs`
(`init_gtk_or_skip(context, required_env)`, `find_descendant_with_class`)
to skip gracefully in headless/non-GTK environments rather than failing.
Follow that pattern for new widget tests instead of assuming a display is
always available.

## General

- This is a hot-reloading config-driven panel — prefer additive,
  backward-compatible config options over breaking changes. If you're
  replacing a widget's default behavior (e.g. swapping an external-command
  fallback for a native implementation), keep the old behavior reachable
  via an explicit opt-in option rather than deleting it outright.
- Match existing patterns before introducing new ones. This codebase has
  a lot of small, consistent conventions (see above) — a few extra minutes
  grepping for a precedent (e.g. "how does an existing popover do X")
  saves a lot of inconsistency later.
