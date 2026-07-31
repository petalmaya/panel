//! Simple application launcher widget.
//! Minimal integrated popover that runs a configured command (default: rofi -show drun).
//! Uses BaseWidget::create_menu to provide a LayerShell popover.

use gtk4::prelude::*;
use gtk4::{Label, Align};
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;
use std::rc::Rc;
use std::cell::RefCell;

use vibepanel_core::config::WidgetEntry;

use crate::widgets::base::{BaseWidget, describe_exit_status};
use crate::widgets::{WidgetConfig, warn_unknown_options};
use crate::styles::{icon as icon_style, state};
use tracing::warn;

use crate::widgets::launcher_popover::{build_launcher_popover_with_controller, LauncherPopoverController};

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
    // Keep controller alive for popover reuse
    _controller: Option<Rc<LauncherPopoverController>>,
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

        // Create a lazy menu and set a builder that creates the popover content.
        // Keep a controller holder so we retain the controller behind the builder.
        let controller_holder: Rc<RefCell<Option<Rc<LauncherPopoverController>>>> = Rc::new(RefCell::new(None));

        // Create a placeholder for the menu; actual builder will be set below.
        let menu_handle = base.create_menu(|| gtk4::Label::new(None).upcast::<gtk4::Widget>());

        {
            let controller_holder = controller_holder.clone();
            let cfg_clone = cfg.clone();
            menu_handle.set_builder_with_monitor(move |_monitor| {
                // Build the popover and retain controller so it stays alive.
                let (widget, controller) = build_launcher_popover_with_controller(&cfg_clone);
                *controller_holder.borrow_mut() = Some(controller.clone());
                widget
            });
        }

        // Reuse the popover across opens to avoid re-building desktop scanning on every open.
        menu_handle.set_reuse_content(true);

        // When the popover is shown, refresh its content in case the external state changed.
        {
            let ch = controller_holder.clone();
            menu_handle.set_on_show(move || {
                if let Some(ref ctrl) = *ch.borrow() {
                    ctrl.refresh();
                }
            });
        }

        Self {
            base,
            _icon_name: cfg.icon,
            _controller: controller_holder.borrow().clone(),
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        self.base.widget()
    }
}
