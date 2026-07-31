//! Lightweight launcher popover UI and controller.
//!
//! MVP: provides a small popover with a search entry and a FlowBox of app
//! buttons. For now this displays a single "Open Apps" button that runs the
//! configured `launch_cmd`. Future work: enumerate .desktop files, icons, and
//! full search+activation.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, FlowBox, FlowBoxChild, Label, Orientation, Widget};
use gtk4::gio;
use gtk4::glib;
use std::rc::Rc;
use std::cell::RefCell;
use std::process::{Command, Stdio};
use tracing::warn;

use crate::widgets::launcher::LauncherConfig;

/// Controller for the launcher popover.
///
/// The controller owns the search entry and the FlowBox where app buttons are shown.
#[derive(Clone)]
pub struct LauncherPopoverController {
    cfg: LauncherConfig,
    pub container: GtkBox,
    search: Entry,
    flow: FlowBox,
}

impl LauncherPopoverController {
    /// Create a new controller from config. Builds the widget tree.
    pub fn new(cfg: LauncherConfig) -> Rc<Self> {
        let container = GtkBox::new(Orientation::Vertical, 6);
        container.add_css_class("launcher-popover");
        container.set_size_request(360, -1);

        let search = Entry::new();
        search.set_placeholder_text(Some("Search apps"));
        search.set_hexpand(true);
        container.append(&search);

        let flow = FlowBox::new();
        flow.set_selection_mode(gtk4::SelectionMode::None);
        flow.set_margin_top(6);
        flow.set_margin_bottom(6);
        flow.set_margin_start(6);
        flow.set_margin_end(6);
        container.append(&flow);

        let controller = Rc::new(Self {
            cfg,
            container,
            search,
            flow,
        });

        // initial population (MVP: single button that runs cfg.launch_cmd)
        controller.populate_initial();

        // Hook search changes to filter (no-op for MVP)
        {
            let ctrl = Rc::clone(&controller);
            controller.search.connect_changed(move |_e| {
                ctrl.filter_and_update();
            });
        }

        controller
    }

    fn populate_initial(&self) {
        // Clear existing
        for child in self.flow.children() {
            self.flow.remove(&child);
        }

        // Create one button that launches the configured command
        let btn = Button::new();
        btn.add_css_class("launcher-app-button");
        let vbox = GtkBox::new(Orientation::Vertical, 2);
        let lbl = Label::new(Some(self.cfg.label.as_deref().unwrap_or("Apps")));
        lbl.add_css_class("launcher-app-label");
        vbox.append(&lbl);
        btn.set_child(Some(&vbox));

        let cmd = self.cfg.launch_cmd.clone();
        btn.connect_clicked(move |_| {
            let cmd = cmd.clone();
            // spawn in background similar to other widgets
            glib::spawn_future_local(async move {
                let _ = gio::spawn_blocking(move || {
                    match Command::new("sh")
                        .args(["-c", &cmd])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(mut child) => match child.wait() {
                            Ok(status) if !status.success() => {
                                warn!("launcher command '{}' failed (nonzero exit)", cmd);
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

        let child = FlowBoxChild::new();
        child.set_child(Some(&btn));
        self.flow.append(&child);
    }

    fn filter_and_update(&self) {
        // For MVP, nothing to filter yet. Future: filter DesktopEntry list.
        if self.search.text().is_empty() {
            self.populate_initial();
        } else {
            // no-op for now, still show single launcher
            self.populate_initial();
        }
    }

    /// Return the root widget to embed in a popover.
    pub fn widget(&self) -> Widget {
        self.container.clone().upcast::<Widget>()
    }

    /// Refresh/populate entries again (call on popover show if desired).
    pub fn refresh(&self) {
        self.populate_initial();
    }
}

/// Build a popover widget and its controller from config.
///
/// Returns (widget, Rc<controller>) so callers can keep the controller alive
/// and call refresh/update on show.
pub fn build_launcher_popover_with_controller(cfg: &LauncherConfig) -> (Widget, Rc<LauncherPopoverController>) {
    let ctrl = LauncherPopoverController::new(cfg.clone());
    (ctrl.widget(), ctrl)
}
