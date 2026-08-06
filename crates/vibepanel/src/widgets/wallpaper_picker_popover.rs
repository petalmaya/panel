//! Wallpaper picker popover content.
//!
//! Lists image files from a configured directory, shows an async-loaded
//! thumbnail per entry, and offers a per-monitor target plus a fill mode
//! (fill/fit/stretch/center/tile). On selection:
//!
//! 1. Applies the wallpaper via `waypaper --wallpaper <path> --fill <mode>
//!    [--monitor <name>]` (see `services::wallpaper::set_wallpaper_via_waypaper`).
//! 2. Persists it as `theme.wallpaper` in `config.toml`
//!    (`ConfigManager::persist_theme_wallpaper`), which the existing config
//!    file watcher then picks up to re-extract the Material You palette and
//!    reapply system-wide GTK-scoped theming — no separate theming code
//!    path needed here. Note the persisted theme source is always the
//!    picked image regardless of which monitor it was applied to; per-
//!    monitor *theming* (as opposed to per-monitor wallpaper) isn't a thing
//!    this codebase's single global palette supports today.
//!
//! Built the same way as `launcher_popover.rs`: a `ListBox` of `ListRow`s
//! inside a `ScrolledWindow`, reusing the launcher's popover CSS classes
//! since the visual shape (icon/thumbnail + title, scrollable list) is
//! identical.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk::{MemoryFormat, MemoryTexture};
use gtk4::gio;
use gtk4::glib;
use gtk4::glib::Bytes;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    Popover, ScrolledWindow, SelectionMode, Widget,
};
use tracing::warn;

use crate::popover_tracker::PopoverTracker;
use crate::services::config_manager::ConfigManager;
use crate::services::wallpaper::{list_wallpapers, set_wallpaper_via_waypaper, WallpaperFillMode};
use crate::styles::launcher_popover as lp;
use crate::styles::{button, qs, surface};
use crate::widgets::base::{configure_popover, vp_button};
use crate::widgets::quick_settings::components::ListRow;
use crate::widgets::quick_settings::ui_helpers::{
    clear_list_box, close_row_menu_popover, create_row_menu_action,
};
use crate::widgets::rounded_picture::RoundedPicture;

/// Options controlling the wallpaper picker popover's behavior. Mirrors
/// what `WallpaperPickerConfig` (in `wallpaper_picker.rs`) exposes from
/// `config.toml`; the monitor/fill mode here are just the *defaults* the
/// popover opens with — both are also changeable live via the dropdowns at
/// the top of the popover.
#[derive(Debug, Clone)]
pub struct WallpaperPickerOptions {
    pub directory: PathBuf,
    /// Default target monitor connector (e.g. "eDP-1"). `None` means "all
    /// monitors", which is also always the first dropdown entry.
    pub monitor: Option<String>,
    pub fill: WallpaperFillMode,
    /// Thumbnail edge length in logical pixels.
    pub thumbnail_size: i32,
}

/// Live, user-adjustable selection state for the header dropdowns, shared
/// between the dropdown callbacks and the row-activation handler.
#[derive(Debug, Clone, Default)]
struct Selection {
    monitor: Option<String>,
    fill: Option<WallpaperFillMode>,
}

/// Enumerate connected monitors' connector names (e.g. "eDP-1", "DP-2") via
/// GDK, for the "which screen" dropdown. Falls back to an empty list (i.e.
/// just "All monitors" is offered) if the display can't be reached, which
/// can happen in headless test/CI environments.
fn list_monitor_connectors() -> Vec<String> {
    let Some(display) = gtk4::gdk::Display::default() else {
        return Vec::new();
    };

    let monitors = display.monitors();
    let mut names = Vec::new();
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gtk4::gdk::Monitor>() {
                if let Some(connector) = monitor.connector() {
                    names.push(connector.to_string());
                }
            }
        }
    }
    names
}

/// Apply the picked wallpaper (via waypaper) and persist it as the theme's
/// Material You source, off the GTK main thread.
fn apply_wallpaper(path: PathBuf, monitor: Option<String>, fill: WallpaperFillMode) {
    glib::MainContext::default().spawn_local(async move {
        let path_for_daemon = path.clone();
        let monitor_for_daemon = monitor.clone();
        let result = gio::spawn_blocking(move || {
            set_wallpaper_via_waypaper(&path_for_daemon, monitor_for_daemon.as_deref(), fill)
        })
        .await
        // gio::spawn_blocking's join error is `Box<dyn Any + Send>` (the
        // caught panic payload), which implements neither `Display` nor
        // `Debug` — there's nothing meaningful to format, so just note that
        // the task panicked.
        .unwrap_or_else(|_| Err("wallpaper-set task panicked".to_string()));

        if let Err(e) = result {
            warn!("Wallpaper picker: failed to set wallpaper via waypaper: {e}");
            return;
        }

        let Some(path_str) = path.to_str() else {
            warn!(
                "Wallpaper picker: wallpaper path '{}' is not valid UTF-8, cannot persist to config.toml",
                path.display()
            );
            return;
        };

        if let Err(e) = ConfigManager::global().persist_theme_wallpaper(path_str) {
            warn!(
                "Wallpaper picker: applied wallpaper but failed to persist it to config.toml \
                 (theme won't update until you set `theme.wallpaper` yourself): {e}"
            );
        }
    });
}

/// Build a placeholder icon shown until a thumbnail finishes decoding (or
/// if decoding fails), sitting underneath the `RoundedPicture` in an
/// `Overlay` so it shows through until the thumbnail is ready.
fn build_placeholder_icon(size: i32) -> Image {
    let image = Image::from_icon_name("image-x-generic-symbolic");
    image.add_css_class(lp::ROW_ICON);
    image.set_pixel_size(size / 2);
    image.set_halign(Align::Center);
    image.set_valign(Align::Center);
    image
}

/// Decode and downscale a wallpaper thumbnail off the GTK main thread.
///
/// This deliberately uses the `image` crate rather than
/// `gdk_pixbuf::Pixbuf`: `Pixbuf` wraps a raw GObject pointer that is
/// neither `Send` nor `Sync`, so a `Result<Pixbuf, _>` can't cross
/// `gio::spawn_blocking`'s thread boundary at all (this is a hard compiler
/// error, not a perf choice). Plain `Vec<u8>` RGBA bytes are `Send`, so the
/// decode+resize happens here on the blocking thread pool, and the caller
/// builds a `gtk4::gdk::MemoryTexture` from the bytes back on the main
/// thread. Returns `None` on any decode failure — the caller just leaves
/// the placeholder icon showing rather than needing a real error type.
fn decode_thumbnail(path: &Path, size: u32) -> Option<(u32, u32, Vec<u8>)> {
    let image = image::open(path).ok()?;
    let thumbnail = image.thumbnail(size, size).to_rgba8();
    let (width, height) = thumbnail.dimensions();
    Some((width, height, thumbnail.into_raw()))
}

/// Kick off an async, off-main-thread thumbnail decode for `path` and swap
/// it into `target` once ready. Failures just leave the placeholder icon
/// underneath showing.
fn load_thumbnail_async(path: PathBuf, size: i32, target: RoundedPicture) {
    glib::MainContext::default().spawn_local(async move {
        let decode_path = path.clone();
        let decode_size = size.max(1) as u32;
        let decoded =
            gio::spawn_blocking(move || decode_thumbnail(&decode_path, decode_size)).await;

        match decoded {
            Ok(Some((width, height, rgba))) => {
                // 4 bytes/pixel (RGBA8), tightly packed rows.
                let stride = (width as usize) * 4;
                let bytes = Bytes::from_owned(rgba);
                let texture = MemoryTexture::new(
                    width as i32,
                    height as i32,
                    MemoryFormat::R8g8b8a8,
                    &bytes,
                    stride,
                );
                target.set_paintable(Some(&texture));
                target.set_visible(true);
            }
            Ok(None) => {
                warn!(
                    "Wallpaper picker: failed to decode thumbnail for '{}'",
                    path.display()
                );
            }
            Err(_) => {
                // See `decode_thumbnail`'s doc comment: the join error type
                // (`Box<dyn Any + Send>`) has nothing formattable in it.
                warn!(
                    "Wallpaper picker: thumbnail decode task panicked for '{}'",
                    path.display()
                );
            }
        }
    });
}

/// Build a leading widget for a row: a placeholder icon overlaid by a
/// `RoundedPicture` (the same widget used for album art elsewhere in this
/// codebase), the latter starting hidden and swapped in by
/// `load_thumbnail_async` once its thumbnail finishes decoding.
fn build_row_leading_widget(path: &Path, size: i32) -> gtk4::Overlay {
    let overlay = gtk4::Overlay::new();
    overlay.set_size_request(size, size);

    let placeholder = build_placeholder_icon(size);
    overlay.set_child(Some(&placeholder));

    let picture = RoundedPicture::new();
    picture.set_pixel_size(size);
    picture.set_corner_radius(6.0);
    picture.set_visible(false);
    overlay.add_overlay(&picture);

    load_thumbnail_async(path.to_path_buf(), size, picture);

    overlay
}

/// Build the empty state shown when the configured directory has no
/// recognized wallpaper images (or doesn't exist).
fn build_empty_state(directory: &Path) -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 6);
    container.add_css_class(lp::EMPTY);
    container.set_valign(Align::Center);
    container.set_halign(Align::Center);
    container.set_hexpand(true);

    let icon = Image::from_icon_name("folder-pictures-symbolic");
    icon.add_css_class(lp::EMPTY_ICON);
    icon.set_halign(Align::Center);
    icon.set_pixel_size(28);
    container.append(&icon);

    let label = Label::new(Some(&format!(
        "No wallpapers found in {}",
        directory.display()
    )));
    label.add_css_class(lp::EMPTY_LABEL);
    label.set_halign(Align::Center);
    label.set_wrap(true);
    container.append(&label);

    container
}

/// Build the header row with the "target monitor" and "fill mode"
/// dropdowns. Returns the row plus the two `DropDown`s so the caller can
/// wire `connect_selected_notify` against the shared `Selection` state.
/// Build one "chip" selector: a small ghost button showing the current
/// choice with a chevron, opening a QS-style popover menu (same visual
/// language — `.qs-row-menu-content` / `.qs-row-menu-item` — as the row
/// menus in the quick settings panel, e.g. the Wi-Fi network actions menu)
/// listing `choices` on click.
fn build_selector_chip(
    icon_name: &str,
    choices: &[String],
    initial_idx: usize,
    on_pick: Rc<dyn Fn(usize, &str)>,
) -> Button {
    let btn = vp_button();
    btn.set_has_frame(false);
    btn.add_css_class(button::GHOST);
    btn.add_css_class("wallpaper-picker-chip");

    let content = GtkBox::new(Orientation::Horizontal, 4);
    content.set_margin_start(6);
    content.set_margin_end(6);
    content.set_margin_top(2);
    content.set_margin_bottom(2);

    let icon = Image::from_icon_name(icon_name);
    icon.set_pixel_size(14);
    content.append(&icon);

    let label = Label::new(choices.get(initial_idx).map(String::as_str));
    label.set_xalign(0.0);
    content.append(&label);

    let chevron = Image::from_icon_name("pan-down-symbolic");
    chevron.set_pixel_size(10);
    content.append(&chevron);

    btn.set_child(Some(&content));

    let choices = choices.to_vec();
    btn.connect_clicked(move |btn| {
        let popover = Popover::new();
        configure_popover(&popover);

        let panel = GtkBox::new(Orientation::Vertical, 0);
        panel.add_css_class(surface::POPOVER);
        panel.add_css_class(surface::SURFACE_POPOVER);
        panel.add_css_class(surface::WIDGET_MENU_CONTENT);

        let content_box = GtkBox::new(Orientation::Vertical, 2);
        content_box.add_css_class(qs::ROW_MENU_CONTENT);

        for (idx, choice) in choices.iter().enumerate() {
            let text = choice.clone();
            let text_for_closure = text.clone();
            let popover_weak = popover.downgrade();
            let label = label.clone();
            let on_pick = on_pick.clone();
            let action = create_row_menu_action(&text, move || {
                // Close the popover before running the callback, matching
                // the same ordering used for the network/bluetooth row
                // menus (avoids a "still has children" warning on unparent).
                if let Some(p) = popover_weak.upgrade() {
                    close_row_menu_popover(&p);
                }
                label.set_label(&text_for_closure);
                on_pick(idx, &text_for_closure);
            });
            content_box.append(&action);
        }

        panel.append(&content_box);
        popover.set_child(Some(&panel));
        popover.set_parent(btn);
        popover.popup();
        popover.connect_closed(|p| {
            if p.parent().is_some() {
                p.unparent();
            }
        });
    });

    btn
}

/// Build the header row: two selector chips ("target monitor" and "fill
/// mode"), styled to match the quick settings panel rather than a bare
/// native GTK dropdown. Selections are written into `selection` directly
/// from each chip's popover, so the caller doesn't need to wire anything
/// further.
fn build_header(
    monitors: &[String],
    default: &WallpaperPickerOptions,
    selection: &Rc<RefCell<Selection>>,
) -> GtkBox {
    let header = GtkBox::new(Orientation::Horizontal, 6);
    header.add_css_class("wallpaper-picker-header");

    let mut monitor_choices: Vec<String> = vec!["All monitors".to_string()];
    monitor_choices.extend(monitors.iter().cloned());
    let monitor_idx = default
        .monitor
        .as_ref()
        .and_then(|m| monitors.iter().position(|x| x == m))
        .map(|i| i + 1)
        .unwrap_or(0);

    let selection_for_monitor = selection.clone();
    let monitors_for_pick = monitors.to_vec();
    let monitor_chip = build_selector_chip(
        "video-display-symbolic",
        &monitor_choices,
        monitor_idx,
        Rc::new(move |idx, _text| {
            selection_for_monitor.borrow_mut().monitor = if idx == 0 {
                None
            } else {
                monitors_for_pick.get(idx - 1).cloned()
            };
        }),
    );

    let fill_choices: Vec<String> = WallpaperFillMode::ALL
        .iter()
        .map(|f| f.label().to_string())
        .collect();
    let fill_idx = WallpaperFillMode::ALL
        .iter()
        .position(|f| *f == default.fill)
        .unwrap_or(0);

    let selection_for_fill = selection.clone();
    let fill_chip = build_selector_chip(
        "image-x-generic-symbolic",
        &fill_choices,
        fill_idx,
        Rc::new(move |idx, _text| {
            selection_for_fill.borrow_mut().fill = WallpaperFillMode::ALL.get(idx).copied();
        }),
    );

    header.append(&monitor_chip);
    header.append(&fill_chip);

    header
}

/// Rebuild the wallpaper list from `options.directory`, replacing whatever
/// rows are currently in `list_box`.
fn rebuild_rows(
    list_box: &ListBox,
    empty_state: &GtkBox,
    scroll: &ScrolledWindow,
    options: &WallpaperPickerOptions,
    row_paths: &Rc<RefCell<Vec<(ListBoxRow, PathBuf)>>>,
) {
    clear_list_box(list_box);
    row_paths.borrow_mut().clear();

    let wallpapers = list_wallpapers(&options.directory);

    if wallpapers.is_empty() {
        empty_state.set_visible(true);
        scroll.set_visible(false);
        return;
    }

    empty_state.set_visible(false);
    scroll.set_visible(true);

    for path in wallpapers {
        let leading = build_row_leading_widget(&path, options.thumbnail_size);

        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        let result = ListRow::builder()
            .title(&title)
            .leading_widget(leading.upcast::<Widget>())
            .build();

        list_box.append(&result.row);
        row_paths.borrow_mut().push((result.row, path));
    }
}

/// Handle returned alongside the popover widget, letting the caller refresh
/// the wallpaper list on each open (picking up files added/removed from the
/// directory since the last open, and any newly plugged-in monitors — see
/// `refresh`) without rebuilding the whole widget tree.
pub struct WallpaperPickerController {
    list_box: ListBox,
    empty_state: GtkBox,
    scroll: ScrolledWindow,
    options: WallpaperPickerOptions,
    row_paths: Rc<RefCell<Vec<(ListBoxRow, PathBuf)>>>,
}

impl WallpaperPickerController {
    /// Re-scan the wallpaper directory and rebuild the row list. Does *not*
    /// re-scan monitors — the dropdown is built once, since monitor
    /// hot-plug mid-session is a rare enough case that reopening the whole
    /// popover (which does rebuild it) is an acceptable workaround for now.
    pub fn refresh(&self) {
        rebuild_rows(
            &self.list_box,
            &self.empty_state,
            &self.scroll,
            &self.options,
            &self.row_paths,
        );
    }
}

/// Build the wallpaper picker popover content: a header with monitor/fill
/// dropdowns, and a scrollable list of wallpapers from `options.directory`.
///
/// Returns the root widget plus a [`WallpaperPickerController`] for
/// refreshing it on subsequent opens (see `WallpaperPickerWidget`, which
/// calls `refresh()` from `MenuHandle::set_on_show`, matching how the
/// launcher popover re-enumerates apps on every open).
pub fn build_wallpaper_picker_popover(
    options: &WallpaperPickerOptions,
) -> (Widget, WallpaperPickerController) {
    let container = GtkBox::new(Orientation::Vertical, 0);
    container.add_css_class(lp::ROOT);

    let monitors = list_monitor_connectors();

    let row_paths: Rc<RefCell<Vec<(ListBoxRow, PathBuf)>>> = Rc::new(RefCell::new(Vec::new()));

    let selection = Rc::new(RefCell::new(Selection {
        monitor: options.monitor.clone(),
        fill: Some(options.fill),
    }));

    let header = build_header(&monitors, options, &selection);
    container.append(&header);

    let list_box = ListBox::new();
    list_box.add_css_class(lp::LIST);
    list_box.set_selection_mode(SelectionMode::None);

    let empty_state = build_empty_state(&options.directory);

    let scroll = ScrolledWindow::new();
    scroll.add_css_class(lp::SCROLL);
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_child(Some(&list_box));
    scroll.set_propagate_natural_height(true);
    // Cap results height so a large wallpaper folder scrolls instead of
    // growing the popover past the screen.
    scroll.set_max_content_height(420);

    let results_stack = GtkBox::new(Orientation::Vertical, 0);
    results_stack.append(&scroll);
    results_stack.append(&empty_state);
    container.append(&results_stack);

    // `ListBox::row-activated` fires for both a mouse click and Enter/Space
    // on a focused row — see the identical note in `launcher_popover.rs`
    // about not bridging it to the row's own `activate` signal (infinite
    // recursion).
    list_box.connect_row_activated({
        let row_paths = row_paths.clone();
        let selection = selection.clone();
        move |_, row| {
            let path = row_paths
                .borrow()
                .iter()
                .find(|(r, _)| r == row)
                .map(|(_, path)| path.clone());
            if let Some(path) = path {
                let selection = selection.borrow();
                let monitor = selection.monitor.clone();
                let fill = selection.fill.unwrap_or_default();
                apply_wallpaper(path, monitor, fill);
            }
            PopoverTracker::global().dismiss_active();
        }
    });

    rebuild_rows(&list_box, &empty_state, &scroll, options, &row_paths);

    let controller = WallpaperPickerController {
        list_box,
        empty_state,
        scroll,
        options: options.clone(),
        row_paths,
    };

    (container.upcast::<Widget>(), controller)
}
