//! App launcher popover content.
//!
//! Builds a search box + scrollable results list backed by `gio::AppInfo`
//! (i.e. the desktop's installed `.desktop` entries). This mirrors how
//! `taskbar.rs` / `services/icons.rs` already resolve app icons, but here we
//! enumerate *all* installed applications rather than resolving a single
//! known app_id.
//!
//! Rows are built with the same `ListRow` helper used by the Quick Settings
//! Wi-Fi/Bluetooth/VPN lists, so the launcher list looks and behaves
//! consistently with the rest of the popovers in the bar.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::gio::{self, prelude::*};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Image, Label, ListBox, ListBoxRow, Orientation, PolicyType,
    ScrolledWindow, SearchEntry, SelectionMode, Widget,
};

use crate::popover_tracker::PopoverTracker;
use crate::services::config_manager::ConfigManager;
use crate::styles::{color, launcher_popover as lp, row};
use crate::widgets::quick_settings::components::ListRow;
use crate::widgets::quick_settings::ui_helpers::clear_list_box;

/// A single enumerated application, with lowercased fields pre-computed for
/// cheap repeated filtering as the user types.
struct LauncherApp {
    info: gio::DesktopAppInfo,
    name: String,
    name_lower: String,
    subtitle: Option<String>,
    subtitle_lower: String,
}

/// Options controlling the launcher popover's appearance/behavior.
/// Mirrors what `LauncherConfig` (in `launcher.rs`) exposes from `config.toml`.
pub struct LauncherPopoverOptions {
    pub placeholder: String,
    pub max_results: usize,
}

/// Collect all "should show" desktop applications, sorted alphabetically.
///
/// Filters out entries hidden via `NoDisplay`/`Hidden`/`OnlyShowIn` (via
/// `should_show()`), matching what a standard app menu would list.
fn collect_apps() -> Vec<LauncherApp> {
    let mut apps: Vec<LauncherApp> = gio::AppInfo::all()
        .into_iter()
        .filter_map(|info| info.downcast::<gio::DesktopAppInfo>().ok())
        .filter(|info| info.should_show())
        .map(|info| {
            let display = info.display_name().trim().to_string();
            let name = if display.is_empty() {
                info.name().trim().to_string()
            } else {
                display
            };

            let subtitle = info
                .generic_name()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    info.description()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });

            LauncherApp {
                name_lower: name.to_lowercase(),
                subtitle_lower: subtitle.clone().unwrap_or_default().to_lowercase(),
                subtitle,
                name,
                info,
            }
        })
        .collect();

    apps.sort_by(|a, b| a.name_lower.cmp(&b.name_lower));
    apps
}

/// Score a query against an app entry; lower is a better match. `None` means
/// no match at all.
///
/// 0: name starts with the query
/// 1: some word in the name starts with the query
/// 2: name contains the query anywhere
/// 3: subtitle (generic name / description) contains the query
fn score(app: &LauncherApp, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    if app.name_lower.starts_with(query) {
        Some(0)
    } else if app.name_lower.split_whitespace().any(|w| w.starts_with(query)) {
        Some(1)
    } else if app.name_lower.contains(query) {
        Some(2)
    } else if app.subtitle_lower.contains(query) {
        Some(3)
    } else {
        None
    }
}

/// Launch an app via `GAppInfo::launch`, using the display's launch context
/// so the compositor gets proper startup-notification / activation-token
/// handling (correct focus behavior under Wayland).
fn launch_app(info: &gio::DesktopAppInfo) {
    let context = gtk4::gdk::Display::default().map(|d| d.app_launch_context());
    let files: &[gio::File] = &[];
    if let Err(e) = info.launch(files, context.as_ref()) {
        tracing::warn!("launcher: failed to launch '{}': {}", info.name(), e);
    }
}

/// Build an icon widget for an app, sized consistently with other app icons
/// in the bar (taskbar, media player, tray).
fn build_row_icon(info: &gio::DesktopAppInfo) -> Image {
    let image = match info.icon() {
        Some(icon) => Image::from_gicon(&icon),
        None => Image::from_icon_name("application-x-executable"),
    };
    image.add_css_class(lp::ROW_ICON);
    image.add_css_class(row::QS_ICON);
    image.set_halign(Align::Center);
    image.set_valign(Align::Center);
    image.set_pixel_size(ConfigManager::global().theme_sizes().pixmap_icon_size as i32);
    image
}

/// Build the empty/no-results state shown when a query matches nothing.
fn build_empty_state() -> GtkBox {
    let container = GtkBox::new(Orientation::Vertical, 6);
    container.add_css_class(lp::EMPTY);
    container.set_valign(Align::Center);
    container.set_halign(Align::Center);
    container.set_hexpand(true);
    container.set_visible(false);

    let icon = Image::from_icon_name("edit-find-symbolic");
    icon.add_css_class(lp::EMPTY_ICON);
    icon.add_css_class(color::MUTED);
    icon.set_halign(Align::Center);
    icon.set_pixel_size(28);
    container.append(&icon);

    let label = Label::new(Some("No matching apps"));
    label.add_css_class(lp::EMPTY_LABEL);
    label.add_css_class(color::MUTED);
    label.set_halign(Align::Center);
    container.append(&label);

    container
}

/// Rebuild the results list for the given query, updating `top_match` (used
/// by Enter-to-launch on the search entry) and `row_apps` (used to resolve
/// which app a `ListBox::row-activated` click/Enter corresponds to).
fn rebuild_results(
    list_box: &ListBox,
    empty_state: &GtkBox,
    scroll: &ScrolledWindow,
    apps: &[LauncherApp],
    query: &str,
    max_results: usize,
    top_match: &Rc<RefCell<Option<gio::DesktopAppInfo>>>,
    row_apps: &Rc<RefCell<Vec<(ListBoxRow, gio::DesktopAppInfo)>>>,
) {
    clear_list_box(list_box);
    row_apps.borrow_mut().clear();

    let mut matches: Vec<(u8, &LauncherApp)> = apps
        .iter()
        .filter_map(|app| score(app, query).map(|s| (s, app)))
        .collect();
    matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.name_lower.cmp(&b.1.name_lower)));
    matches.truncate(max_results);

    *top_match.borrow_mut() = matches.first().map(|(_, app)| app.info.clone());

    if matches.is_empty() {
        empty_state.set_visible(true);
        scroll.set_visible(false);
        return;
    }

    empty_state.set_visible(false);
    scroll.set_visible(true);

    for (_, app) in matches {
        let icon = build_row_icon(&app.info);
        let mut builder = ListRow::builder()
            .title(&app.name)
            .leading_widget(icon.upcast::<Widget>());
        if let Some(subtitle) = app.subtitle.as_deref() {
            builder = builder.subtitle(subtitle);
        }
        let result = builder.build();

        list_box.append(&result.row);
        row_apps.borrow_mut().push((result.row, app.info.clone()));
    }
}

/// Handle returned alongside the popover widget, letting the caller refresh
/// the app list and reset search state on each open without rebuilding the
/// widget tree (see [`LauncherPopoverOptions`] / `set_reuse_content`).
pub struct LauncherPopoverController {
    entry: SearchEntry,
    list_box: ListBox,
    empty_state: GtkBox,
    scroll: ScrolledWindow,
    apps: Rc<RefCell<Vec<LauncherApp>>>,
    top_match: Rc<RefCell<Option<gio::DesktopAppInfo>>>,
    row_apps: Rc<RefCell<Vec<(ListBoxRow, gio::DesktopAppInfo)>>>,
    max_results: usize,
}

impl LauncherPopoverController {
    /// Re-enumerate installed applications and reset the search box.
    ///
    /// Cheap compared to rebuilding the popover: `gio::AppInfo::all()` is a
    /// cached lookup, not a disk scan, so this only re-does the (much
    /// smaller) app-list diff + row rebuild instead of tearing down and
    /// recreating the whole layer-shell surface, CSS tree, and icon set on
    /// every open.
    pub fn refresh(&self) {
        *self.apps.borrow_mut() = collect_apps();
        self.entry.set_text("");
        rebuild_results(
            &self.list_box,
            &self.empty_state,
            &self.scroll,
            &self.apps.borrow(),
            "",
            self.max_results,
            &self.top_match,
            &self.row_apps,
        );
        // Deliberately not grabbing focus here: `on_show` fires before the
        // window is (re)presented, so a grab now would just be cleared by
        // `prepare_keyboard_nav()` right after. The entry's own
        // `connect_map` handler (idle-deferred) already re-focuses it on
        // every reopen, including reuse_content remap cycles.
    }
}

/// Build the launcher popover content: search entry + scrollable app list.
///
/// Returns the root widget plus a [`LauncherPopoverController`] for
/// refreshing it on subsequent opens. The search entry grabs keyboard focus
/// once the popover has finished its post-present focus reset (see the
/// `connect_map` handler below), so typing can start immediately after the
/// popover opens.
pub fn build_launcher_popover(options: &LauncherPopoverOptions) -> (Widget, LauncherPopoverController) {
    let container = GtkBox::new(Orientation::Vertical, 0);

    let entry = SearchEntry::new();
    entry.add_css_class(lp::SEARCH);
    entry.set_placeholder_text(Some(&options.placeholder));
    entry.set_hexpand(true);
    container.append(&entry);

    let list_box = ListBox::new();
    list_box.add_css_class(lp::LIST);
    list_box.set_selection_mode(SelectionMode::None);

    let empty_state = build_empty_state();

    let scroll = ScrolledWindow::new();
    scroll.add_css_class(lp::SCROLL);
    scroll.set_policy(PolicyType::Never, PolicyType::Automatic);
    scroll.set_child(Some(&list_box));
    scroll.set_propagate_natural_height(true);
    // Cap results height so a long app list scrolls instead of growing the
    // popover past the screen; roughly 8 rows before scrolling kicks in.
    scroll.set_max_content_height(360);

    let results_stack = GtkBox::new(Orientation::Vertical, 0);
    results_stack.append(&scroll);
    results_stack.append(&empty_state);
    container.append(&results_stack);

    let apps: Rc<RefCell<Vec<LauncherApp>>> = Rc::new(RefCell::new(collect_apps()));
    let top_match: Rc<RefCell<Option<gio::DesktopAppInfo>>> = Rc::new(RefCell::new(None));
    let row_apps: Rc<RefCell<Vec<(ListBoxRow, gio::DesktopAppInfo)>>> = Rc::new(RefCell::new(Vec::new()));
    let max_results = options.max_results;

    // `ListBox::row-activated` fires for both a mouse click and Enter/Space
    // on a focused row ("by key or not" per the GTK docs) — it's the single
    // correct place to handle "the user picked this row". Do NOT try to
    // additionally forward it into the row's own `activate` signal: that
    // signal's default handler (when the row is parented in a ListBox)
    // re-emits `row-activated`, so bridging the two creates infinite mutual
    // recursion (stack overflow) instead of firing once.
    list_box.connect_row_activated({
        let row_apps = row_apps.clone();
        move |_, row| {
            let info = row_apps
                .borrow()
                .iter()
                .find(|(r, _)| r == row)
                .map(|(_, info)| info.clone());
            if let Some(info) = info {
                launch_app(&info);
            }
            PopoverTracker::global().dismiss_active();
        }
    });

    // Initial (empty-query) population — full alphabetical list.
    rebuild_results(&list_box, &empty_state, &scroll, &apps.borrow(), "", max_results, &top_match, &row_apps);

    entry.connect_search_changed({
        let apps = apps.clone();
        let list_box = list_box.clone();
        let empty_state = empty_state.clone();
        let scroll = scroll.clone();
        let top_match = top_match.clone();
        let row_apps = row_apps.clone();
        move |entry| {
            let query = entry.text().to_lowercase();
            rebuild_results(&list_box, &empty_state, &scroll, &apps.borrow(), &query, max_results, &top_match, &row_apps);
        }
    });

    // Enter in the search box launches the best-ranked match.
    entry.connect_activate({
        let top_match = top_match.clone();
        move |_| {
            if let Some(info) = top_match.borrow().clone() {
                launch_app(&info);
            }
            PopoverTracker::global().dismiss_active();
        }
    });

    // Focus the search entry as soon as the popover maps, so the user can
    // start typing immediately without an extra click.
    //
    // `connect_map` fires synchronously inside `window.present()`, but the
    // popover machinery (`LayerShellPopover::prepare_keyboard_nav`) clears
    // whatever focus present() assigned *right after* present() returns, so
    // it can keep focus rings hidden until the first Tab/arrow press. A
    // direct `grab_focus()` here would just get wiped out immediately after.
    // Deferring to the next main-loop idle tick lets our grab happen after
    // that clearing instead of before it.
    entry.connect_map(|entry| {
        let entry = entry.clone();
        glib::idle_add_local_once(move || {
            entry.grab_focus();
        });
    });

    let controller = LauncherPopoverController {
        entry,
        list_box,
        empty_state,
        scroll,
        apps,
        top_match,
        row_apps,
        max_results,
    };

    (container.upcast::<Widget>(), controller)
}
