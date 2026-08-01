//! App launcher widget.
//!
//! By default this opens a native GTK popover listing installed
//! applications (see `launcher_popover.rs`), built the same way as every
//! other widget menu in the bar: `BaseWidget::create_menu()` wires up the
//! layer-shell popover, left-click toggle, ripple, and hover styling
//! automatically.
//!
//! For anyone who prefers an external launcher (rofi, wofi, anyrun, fuzzel,
//! ...), setting `launch_cmd` in `[widgets.launcher]` switches the widget
//! back to the old "click runs a shell command" behavior instead of opening
//! the popover.

use gtk4::prelude::*;
use gtk4::{Align, GestureClick, Label};
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;

use vibepanel_core::config::WidgetEntry;

use crate::widgets::base::{BaseWidget, describe_exit_status};
use crate::widgets::launcher_popover::{LauncherPopoverOptions, build_launcher_popover};
use crate::widgets::{WidgetConfig, warn_unknown_options};
use crate::styles::{icon as icon_style, state, widget as wgt};
use crate::services::icons::{IconHandle, IconsService};
use tracing::warn;

/// Known options for the launcher widget.
const KNOWN_OPTIONS: &[&str] = &["icon", "label", "launch_cmd", "placeholder", "max_results"];

const DEFAULT_PLACEHOLDER: &str = "Search apps…";
const DEFAULT_MAX_RESULTS: usize = 50;

#[derive(Debug, Clone)]
pub struct LauncherConfig {
    pub icon: Option<String>,
    pub label: Option<String>,
    /// Placeholder text for the popover's search entry.
    pub placeholder: String,
    /// Maximum number of results shown in the popover's app list.
    pub max_results: usize,
    /// Legacy escape hatch: when set, clicking the widget runs this shell
    /// command instead of opening the native popover (e.g. "rofi -show drun").
    pub launch_cmd: Option<String>,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            icon: None,
            label: None,
            placeholder: DEFAULT_PLACEHOLDER.to_string(),
            max_results: DEFAULT_MAX_RESULTS,
            launch_cmd: None,
        }
    }
}

impl WidgetConfig for LauncherConfig {
    fn from_entry(entry: &WidgetEntry) -> Self {
        warn_unknown_options(&entry.name, entry, KNOWN_OPTIONS);
        let icon = entry.options.get("icon").and_then(|v| v.as_str()).map(|s| s.to_string());
        let label = entry.options.get("label").and_then(|v| v.as_str()).map(|s| s.to_string());
        let placeholder = entry
            .options
            .get("placeholder")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| DEFAULT_PLACEHOLDER.to_string());
        let max_results = entry
            .options
            .get("max_results")
            .and_then(|v| v.as_integer())
            .filter(|v| *v > 0)
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_MAX_RESULTS);
        // Empty string counts as "not set" so `launch_cmd = ""` in config
        // doesn't silently disable the popover.
        let launch_cmd = entry
            .options
            .get("launch_cmd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty());

        Self {
            icon,
            label,
            placeholder,
            max_results,
            launch_cmd,
        }
    }
}

pub struct LauncherWidget {
    base: BaseWidget,
    // keep icon handle alive when using IconsService
    _icon_handle: Option<IconHandle>,
    // Only populated in legacy `launch_cmd` mode — keeps the controller
    // registered on the widget for the lifetime of the widget.
    _gesture: Option<GestureClick>,
}

impl LauncherWidget {
    pub fn new(cfg: LauncherConfig) -> Self {
        // primary css class: launcher
        let css_class = "launcher";
        let base = BaseWidget::new(&[css_class]);

        let mut icon_handle: Option<IconHandle> = None;

        if let Some(ref icon_name) = cfg.icon {
            if let Some(glyph) = icon_name.strip_prefix("glyph:") {
                // render literal glyph/emoji as a label (matches custom.rs styling)
                let glyph_lbl = Label::new(Some(glyph));
                glyph_lbl.add_css_class(icon_style::ROOT);
                glyph_lbl.add_css_class(wgt::CUSTOM_ICON_GLYPH);
                glyph_lbl.set_halign(Align::Center);
                glyph_lbl.set_hexpand(true);
                base.content().prepend(&glyph_lbl);
            } else {
                // Create a proper icon handle so it reacts to theme changes
                let handle = IconsService::global().create_icon(icon_name, &[]);
                let widget = handle.widget();
                widget.set_halign(gtk4::Align::Center);
                widget.set_hexpand(true);
                widget.set_visible(true);
                base.content().prepend(&widget);
                icon_handle = Some(handle);
            }
        }

        if let Some(lbl) = cfg.label.as_deref() {
            let label = base.add_label(Some(lbl), &[]);
            label.set_halign(Align::Center);
        }

        let gesture = if let Some(cmd) = cfg.launch_cmd {
            // Legacy mode: no menu is registered, so BaseWidget's own click
            // gesture is a no-op for us — wire our own primary-click handler
            // that shells out, same as the original minimal implementation.
            base.widget().add_css_class(state::CLICKABLE);

            let gesture = GestureClick::new();
            gesture.set_button(gdk::BUTTON_PRIMARY);
            gesture.connect_released(move |_g, _n_press, _x, _y| {
                let cmd = cmd.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _ = gio::spawn_blocking(move || {
                        use std::process::{Command, Stdio};
                        match Command::new("sh")
                            .args(["-c", &cmd])
                            .stdin(Stdio::null())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .spawn()
                        {
                            Ok(mut child) => match child.wait() {
                                Ok(status) if !status.success() => {
                                    warn!("launcher command '{}' failed: {}", cmd, describe_exit_status(status));
                                }
                                Err(e) => {
                                    warn!("launcher command '{}' wait failed: {}", cmd, e);
                                }
                                _ => {}
                            },
                            Err(e) => {
                                warn!("failed to spawn launcher command '{}': {}", cmd, e);
                            }
                        }
                    }).await;
                });
            });
            base.widget().add_controller(gesture.clone());
            Some(gesture)
        } else {
            // Native mode (default): open the app-search popover, wired up
            // the same way every other widget menu in the bar is (left-click
            // toggle, ripple, ESC-to-close, click-outside-to-close, etc. all
            // come from BaseWidget::create_menu()).
            let options = LauncherPopoverOptions {
                placeholder: cfg.placeholder,
                max_results: cfg.max_results,
            };
            base.create_menu(move || build_launcher_popover(&options));
            None
        };

        Self {
            base,
            _icon_handle: icon_handle,
            _gesture: gesture,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        self.base.widget()
    }
}
