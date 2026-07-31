//! Lightweight launcher popover UI and controller.
//!
//! Now implements .desktop discovery (simple parser) and shows a grid of apps
//! with icons using IconsService. Clicking an app launches its Exec command.

use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, FlowBox, FlowBoxChild, Image, Label, Orientation, Widget};
use gtk4::gio;
use gtk4::glib;
use std::rc::Rc;
use std::cell::RefCell;
use std::process::{Command, Stdio};
use tracing::warn;
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::env;

use crate::widgets::launcher::LauncherConfig;
use crate::services::icons::IconsService;

/// Minimal representation of a .desktop entry for the launcher.
#[derive(Clone, Debug)]
struct DesktopEntry {
    id: String,
    name: String,
    exec: Option<String>,
    icon: Option<String>,
}

/// Controller for the launcher popover.
#[derive(Clone)]
pub struct LauncherPopoverController {
    cfg: LauncherConfig,
    pub container: GtkBox,
    search: Entry,
    flow: FlowBox,
    entries: Rc<RefCell<Vec<DesktopEntry>>>,
}

impl LauncherPopoverController {
    /// Create a new controller from config. Builds the widget tree.
    pub fn new(cfg: LauncherConfig) -> Rc<Self> {
        let container = GtkBox::new(Orientation::Vertical, 6);
        container.add_css_class("launcher-popover");
        container.set_size_request(480, -1);

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

        let entries = Rc::new(RefCell::new(Vec::new()));

        let controller = Rc::new(Self {
            cfg,
            container,
            search,
            flow,
            entries: entries.clone(),
        });

        // initial scan and population
        controller.refresh();

        // Hook search changes to filter
        {
            let ctrl = Rc::clone(&controller);
            controller.search.connect_changed(move |e| {
                ctrl.filter_and_update(&e.text().to_string());
            });
        }

        controller
    }

    /// Scan standard XDG application directories for .desktop files.
    fn discover_desktop_entries(&self) -> Vec<DesktopEntry> {
        let mut results = Vec::new();

        let mut dirs = Vec::new();
        if let Ok(xdg) = env::var("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(xdg).join("applications"));
        } else if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".local/share/applications"));
        }
        dirs.push(PathBuf::from("/usr/share/applications"));

        for dir in dirs {
            if !dir.exists() { continue; }
            if let Ok(read_dir) = std::fs::read_dir(&dir) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        if ext == "desktop" {
                            if let Ok(d) = Self::parse_desktop_file(&path) {
                                // skip NoDisplay or Hidden handled in parser
                                results.push(d);
                            }
                        }
                    }
                }
            }
        }

        // sort by name
        results.sort_by(|a,b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        results
    }

    /// Parse a .desktop file for Name, Exec, Icon and NoDisplay/Hidden flags.
    fn parse_desktop_file(path: &Path) -> Result<DesktopEntry, ()> {
        let file = File::open(path).map_err(|_| ())?;
        let reader = BufReader::new(file);
        let mut in_desktop_entry = false;
        let mut name: Option<String> = None;
        let mut exec: Option<String> = None;
        let mut icon: Option<String> = None;
        let mut nodisplay = false;
        let mut hidden = false;

        for line in reader.lines().flatten() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            if line.starts_with('[') {
                in_desktop_entry = line.eq_ignore_ascii_case("[Desktop Entry]");
                continue;
            }
            if !in_desktop_entry { continue; }
            if let Some(pos) = line.find('=') {
                let key = &line[..pos];
                let val = line[pos+1..].trim();
                match key {
                    "Name" => if name.is_none() { name = Some(val.to_string()); },
                    "Exec" => if exec.is_none() { exec = Some(val.to_string()); },
                    "Icon" => if icon.is_none() { icon = Some(val.to_string()); },
                    "NoDisplay" => if val.eq_ignore_ascii_case("true") { nodisplay = true; },
                    "Hidden" => if val.eq_ignore_ascii_case("true") { hidden = true; },
                    _ => {}
                }
            }
        }

        if hidden || nodisplay { return Err(()); }
        let name = name.ok_or(())?;
        let id = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        Ok(DesktopEntry { id, name, exec, icon })
    }

    fn populate_from_entries(&self, entries: &[DesktopEntry]) {
        // Clear existing
        for child in self.flow.children() {
            self.flow.remove(&child);
        }

        let icons = IconsService::global();

        for de in entries.iter().take(256) {
            let btn = Button::new();
            btn.add_css_class("launcher-app-button");

            // build content: icon (if available) and label
            let vbox = GtkBox::new(Orientation::Vertical, 2);

            if let Some(ref icon_name) = de.icon {
                // Use icons service when available, otherwise fallback to stock image
                let widget = icons.create_icon(icon_name, &[]).widget();
                widget.set_margin_bottom(2);
                vbox.append(&widget);
            }

            let lbl = Label::new(Some(&de.name));
            lbl.set_wrap(false);
            lbl.set_max_width_chars(12);
            vbox.append(&lbl);

            btn.set_child(Some(&vbox));

            let de_cloned = de.clone();
            btn.connect_clicked(move |_| {
                if let Some(exec_line) = de_cloned.exec.clone() {
                    // remove field codes like %U %u %f etc.
                    let cleaned = exec_line.split_whitespace()
                        .filter(|tok| !tok.starts_with('%'))
                        .collect::<Vec<_>>()
                        .join(" ");
                    let cmd = cleaned.clone();
                    glib::spawn_future_local(async move {
                        let _ = gio::spawn_blocking(move || {
                            match Command::new("sh")
                                .args(["-c", &cmd])
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .spawn()
                            {
                                Ok(mut child) => {
                                    let _ = child.wait();
                                }
                                Err(e) => {
                                    warn!("failed to spawn app exec '{}': {}", cmd, e);
                                }
                            }
                        }).await;
                    });
                }
            });

            let child = FlowBoxChild::new();
            child.set_child(Some(&btn));
            self.flow.append(&child);
        }
    }

    fn filter_and_update(&self, query: &str) {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            let list = self.entries.borrow();
            self.populate_from_entries(&list);
            return;
        }
        let filtered: Vec<DesktopEntry> = self.entries.borrow().iter()
            .filter(|e| e.name.to_lowercase().contains(&q))
            .cloned()
            .collect();
        self.populate_from_entries(&filtered);
    }

    /// Return the root widget to embed in a popover.
    pub fn widget(&self) -> Widget {
        self.container.clone().upcast::<Widget>()
    }

    /// Refresh/populate entries again (call on popover show if desired).
    pub fn refresh(&self) {
        let discovered = self.discover_desktop_entries();
        *self.entries.borrow_mut() = discovered.clone();
        self.populate_from_entries(&discovered);
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
