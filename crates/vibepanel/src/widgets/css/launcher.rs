//! App launcher popover CSS.

/// Return launcher popover CSS.
pub fn css() -> &'static str {
    r#"
/* ===== LAUNCHER POPOVER ===== */

.popover.launcher-popover {
    margin: 0;
    padding: 10px;
    /* Fixed width so the results list doesn't reflow as the query changes. */
    min-width: 320px;
}

/* Search entry ------------------------------------------------------- */

.launcher-search {
    margin-bottom: 8px;
    min-height: 0;
    padding: 8px 10px;
    border-radius: var(--radius-widget);
    background: var(--color-card-overlay);
    border: none;
    box-shadow: none;
    font-size: var(--font-size-md);
}

.launcher-search:focus-within {
    background: var(--color-card-overlay-hover);
}

.launcher-search image {
    color: var(--color-foreground-muted);
}

/* Results ------------------------------------------------------------- */

.launcher-scroll {
    /* Cap height so a large app list scrolls instead of pushing the
       popover off-screen; individual rows are ~40px tall. */
    min-height: 0;
}

.launcher-list {
    background: transparent;
}

.launcher-list .qs-row {
    margin: 2px 0;
}

.launcher-row-icon {
    margin-left: 1px;
    margin-right: 3px;
}

/* Empty / no-results state --------------------------------------------- */

.launcher-empty {
    padding: 24px 16px;
}

/* Icon is a GtkImage (set_pixel_size in Rust); nothing size-related needed here. */

.launcher-empty-label {
    font-size: var(--font-size-sm);
}
"#
}
