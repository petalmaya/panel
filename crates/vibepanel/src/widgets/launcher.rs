//! Simple application launcher widget.
//! Minimal first implementation: click runs a configured command (default: rofi -show drun).
//! Future improvements: detailed popover with .desktop discovery and search.

use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{GestureClick, Label, Align};
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;

use vibepanel_core::config::WidgetEntry;

use crate::widgets::base::{BaseWidget, describe_exit_status};
use crate::widgets::{WidgetConfig, warn_unknown_options};
use crate::styles::{icon as icon_style, state};
use tracing::warn;

/// Known options for launcher widget
const KNOWN_OPTIONS: &[&str] = &["icon", "label", "launch_cmd"];

#[derive(Debug, Clone, Default)]
pub struct LauncherConfig {
    pub icon: Option<String>,
    pub label: Option<String>,
    pub launch_cmd: String,
}

impl WidgetConfig for LauncherConfig {
    fn from_entry(entry: &WidgetEntry) -> Self {
        warn_unknown_options(&entry.name, entry, KNOWN_OPTIONS);
        let icon = entry.options.get("icon").and_then(|v| v.as_str()).map(|s| s.to_string());
        let label = entry.options.get("label").and_then(|v| v.as_str()).map(|s| s.to_string());
        let launch_cmd = entry
            .options
            .get("launch_cmd")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "rofi -show drun".to_string());

        Self { icon, label, launch_cmd }
    }
}

pub struct LauncherWidget {
    base: BaseWidget,
    // kept for lifetime
    _icon_name: Option<String>,
    _gesture: GestureClick,
}

impl LauncherWidget {
    pub fn new(cfg: LauncherConfig) -> Self {
        // primary css class: launcher
        let css_class = "launcher";
        let base = BaseWidget::new(&[css_class]);

        if let Some(ref icon_name) = cfg.icon {
            // small icon label fallback; integration with IconsService could be added
            let icon_lbl = Label::new(Some(icon_name));
            icon_lbl.add_css_class(icon_style::ROOT);
            icon_lbl.set_halign(Align::Center);
            base.content().append(&icon_lbl);
        }

        if let Some(lbl) = cfg.label.as_deref() {
            let label = base.add_label(Some(lbl), &[]);
            label.set_halign(Align::Center);
        }

        // make clickable (hover style)
        base.widget().add_css_class(state::CLICKABLE);

        let gesture = GestureClick::new();
        let cmd = cfg.launch_cmd.clone();
        gesture.set_button(gdk::BUTTON_PRIMARY);
        gesture.connect_released(move |_g, _n_press, _x, _y| {
            let cmd = cmd.clone();
            glib::spawn_future_local(async move {
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
        base.widget().add_controller(&gesture);

        Self {
            base,
            _icon_name: cfg.icon,
            _gesture: gesture,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        self.base.widget()
    }
}
