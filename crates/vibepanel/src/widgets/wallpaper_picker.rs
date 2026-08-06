//! Wallpaper picker widget.
//!
//! Opens a native GTK popover (see `wallpaper_picker_popover.rs`) listing
//! wallpaper images from a configured directory. Picking one applies it via
//! `waypaper` and updates `theme.wallpaper` in `config.toml`, which feeds
//! back into the existing Material You theming pipeline automatically.
//!
//! Built the same way as every other widget menu in the bar:
//! `BaseWidget::create_menu()` wires up the layer-shell popover, left-click
//! toggle, ripple, and hover styling automatically — this widget just
//! supplies the content, same shape as `launcher.rs`.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::Align;

use vibepanel_core::config::WidgetEntry;

use crate::services::icons::{IconHandle, IconsService};
use crate::services::wallpaper::{waypaper_available, WallpaperFillMode};
use crate::widgets::base::BaseWidget;
use crate::widgets::wallpaper_picker_popover::{
    build_wallpaper_picker_popover, WallpaperPickerController, WallpaperPickerOptions,
};
use crate::widgets::{warn_unknown_options, WidgetConfig};
use tracing::warn;

/// Known options for the wallpaper picker widget.
const KNOWN_OPTIONS: &[&str] = &["icon", "label", "directory", "monitor", "fill", "thumbnail_size"];

const DEFAULT_ICON: &str = "preferences-desktop-wallpaper-symbolic";
const DEFAULT_SUBDIR: &str = "Pictures/Wallpapers";
const DEFAULT_THUMBNAIL_SIZE: i32 = 96;

#[derive(Debug, Clone)]
pub struct WallpaperPickerConfig {
    pub icon: Option<String>,
    pub label: Option<String>,
    /// Directory scanned for wallpaper images. Defaults to
    /// `~/Pictures/Wallpapers`.
    pub directory: PathBuf,
    /// Default target monitor connector (e.g. "eDP-1"). Unset means "all
    /// monitors" — also changeable live from the popover's dropdown.
    pub monitor: Option<String>,
    /// Default fill mode — also changeable live from the popover's dropdown.
    pub fill: WallpaperFillMode,
    /// Thumbnail edge length in logical pixels.
    pub thumbnail_size: i32,
}

fn default_wallpaper_directory() -> PathBuf {
    dirs_home().map(|home| home.join(DEFAULT_SUBDIR)).unwrap_or_else(|| PathBuf::from("."))
}

/// Small local `$HOME` lookup so this widget doesn't need to pull in the
/// `dirs` crate just for one path — mirrors how other widgets in this
/// codebase resolve `$HOME` (see e.g. the config loader's own default path
/// resolution).
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

impl Default for WallpaperPickerConfig {
    fn default() -> Self {
        Self {
            icon: Some(DEFAULT_ICON.to_string()),
            label: None,
            directory: default_wallpaper_directory(),
            monitor: None,
            fill: WallpaperFillMode::default(),
            thumbnail_size: DEFAULT_THUMBNAIL_SIZE,
        }
    }
}

impl WidgetConfig for WallpaperPickerConfig {
    fn from_entry(entry: &WidgetEntry) -> Self {
        warn_unknown_options(&entry.name, entry, KNOWN_OPTIONS);
        let defaults = Self::default();

        let icon = entry
            .options
            .get("icon")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or(defaults.icon);
        let label = entry
            .options
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let directory = entry
            .options
            .get("directory")
            .and_then(|v| v.as_str())
            .map(|s| shellexpand_home(s))
            .unwrap_or(defaults.directory);
        let monitor = entry
            .options
            .get("monitor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());
        let fill = entry
            .options
            .get("fill")
            .and_then(|v| v.as_str())
            .map(WallpaperFillMode::from_config_str)
            .unwrap_or(defaults.fill);
        let thumbnail_size = entry
            .options
            .get("thumbnail_size")
            .and_then(|v| v.as_integer())
            .filter(|v| *v > 0)
            .map(|v| v as i32)
            .unwrap_or(defaults.thumbnail_size);

        Self {
            icon,
            label,
            directory,
            monitor,
            fill,
            thumbnail_size,
        }
    }
}

/// Expand a leading `~` (or bare `$HOME`) to the user's home directory.
/// Deliberately minimal — full shell expansion isn't needed for a single
/// directory-path option.
fn shellexpand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

pub struct WallpaperPickerWidget {
    base: BaseWidget,
    _icon_handle: Option<IconHandle>,
}

impl WallpaperPickerWidget {
    pub fn new(cfg: WallpaperPickerConfig) -> Self {
        let css_class = "wallpaper-picker";
        let base = BaseWidget::new(&[css_class]);

        // Not a hard error — the popover still opens and shows the
        // configured directory either way — but nothing will actually
        // happen when you pick a wallpaper without waypaper on PATH, so
        // it's worth a one-time log line to explain why.
        if !waypaper_available() {
            warn!(
                "Wallpaper picker: `waypaper` was not found on PATH. Install it (or add it to \
                 PATH) for wallpaper selections to take effect — see \
                 https://github.com/anufrievroman/waypaper"
            );
        }

        let mut icon_handle: Option<IconHandle> = None;
        if let Some(ref icon_name) = cfg.icon {
            let handle = IconsService::global().create_icon(icon_name, &[]);
            let widget = handle.widget();
            widget.set_halign(Align::Center);
            widget.set_hexpand(true);
            widget.set_visible(true);
            base.content().prepend(&widget);
            icon_handle = Some(handle);
        }

        if let Some(lbl) = cfg.label.as_deref() {
            let label = base.add_label(Some(lbl), &[]);
            label.set_halign(Align::Center);
        }

        let options = WallpaperPickerOptions {
            directory: cfg.directory,
            monitor: cfg.monitor,
            fill: cfg.fill,
            thumbnail_size: cfg.thumbnail_size,
        };

        // Same pattern as `launcher.rs`: the popover builder only runs
        // once, so stash the controller it returns for `set_on_show`.
        let controller_cell: Rc<RefCell<Option<WallpaperPickerController>>> =
            Rc::new(RefCell::new(None));
        let controller_for_builder = controller_cell.clone();
        let menu_handle = base.create_menu(move || {
            let (widget, controller) = build_wallpaper_picker_popover(&options);
            *controller_for_builder.borrow_mut() = Some(controller);
            widget
        });

        // Reuse the built popover across opens (same rationale as the
        // launcher: rebuilding + re-decoding every thumbnail from scratch
        // on every open is the actual perf cost here, not idle memory).
        menu_handle.set_reuse_content(true);

        // Re-scan the directory each time the popover opens, so wallpapers
        // added/removed since the last open still show up.
        let controller_for_show = controller_cell.clone();
        menu_handle.set_on_show(move || {
            if let Some(ctrl) = controller_for_show.borrow().as_ref() {
                ctrl.refresh();
            }
        });

        Self {
            base,
            _icon_handle: icon_handle,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        self.base.widget()
    }
}
