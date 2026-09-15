use chrono::Local;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
    Terminal,
};
use std::io::stdout;
use std::time::Duration;

use crate::config::Config;
use crate::db::{Db, EntryRecord};
use crate::format_bytes;
use crate::restore::restore_entry;
use crate::theme::{self, THEMES};

#[derive(PartialEq)]
enum ViewMode {
    Browsing,
    ActionMenu,
    InspectModal,
    ExclusionModal,
}

#[derive(PartialEq, Copy, Clone)]
enum ActivePane {
    Preserved,
    History,
}

pub fn run_tui(db: &Db, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut config_clone = config.clone();
    let res = main_loop(&mut terminal, db, &mut config_clone);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("TUI Error: {}", e);
    }
    Ok(())
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn main_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    db: &Db,
    config: &mut Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = db.list_active(None)?;
    let mut history_entries = db.list_history(None)?;
    let mut table_state = TableState::default();
    if !entries.is_empty() {
        table_state.select(Some(0));
    }
    let mut history_table_state = TableState::default();
    if !history_entries.is_empty() {
        history_table_state.select(Some(0));
    }

    let mut theme_idx = theme::theme_index(config.theme.as_deref().unwrap_or("default"));
    let mut active_pane = ActivePane::Preserved;
    let mut view_mode = ViewMode::Browsing;
    let mut action_index: usize = 0;
    let mut status_message: Option<String> = None;
    let mut last_data_version = db.get_data_version().unwrap_or(0);

    loop {
        // Auto-refresh: Detect database changes committed by external processes/terminals
        let current_data_version = db.get_data_version().unwrap_or(last_data_version);
        if current_data_version != last_data_version {
            last_data_version = current_data_version;

            let prev_preserved_id = table_state.selected().and_then(|i| entries.get(i).map(|e| e.id));
            let prev_history_id = history_table_state.selected().and_then(|i| history_entries.get(i).map(|e| e.id));

            if let Ok(new_entries) = db.list_active(None) {
                entries = new_entries;
                if entries.is_empty() {
                    table_state.select(None);
                    if view_mode == ViewMode::ActionMenu || view_mode == ViewMode::InspectModal {
                        view_mode = ViewMode::Browsing;
                    }
                } else if let Some(target_id) = prev_preserved_id {
                    if let Some(pos) = entries.iter().position(|e| e.id == target_id) {
                        table_state.select(Some(pos));
                    } else {
                        let curr = table_state.selected().unwrap_or(0);
                        table_state.select(Some(curr.min(entries.len() - 1)));
                    }
                } else {
                    table_state.select(Some(0));
                }
            }

            if let Ok(new_history) = db.list_history(None) {
                history_entries = new_history;
                if history_entries.is_empty() {
                    history_table_state.select(None);
                } else if let Some(target_id) = prev_history_id {
                    if let Some(pos) = history_entries.iter().position(|e| e.id == target_id) {
                        history_table_state.select(Some(pos));
                    } else {
                        let curr = history_table_state.selected().unwrap_or(0);
                        history_table_state.select(Some(curr.min(history_entries.len() - 1)));
                    }
                } else if active_pane == ActivePane::History {
                    history_table_state.select(Some(0));
                }
            }
        }

        terminal.draw(|f| {
            let size = f.area();
            let theme = &THEMES[theme_idx];

            if let Some(bg_color) = theme.bg {
                let bg_block = Block::default().style(Style::default().bg(bg_color));
                f.render_widget(bg_block, size);
            }

            let show_boxed_header = size.height >= 26;
            let header_height = if show_boxed_header { 3 } else { 1 };

            // Main vertical layout: Top Header, Center Split (remaining), Bottom Footer (1 line)
            let root_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(header_height),
                    Constraint::Min(6),
                    Constraint::Length(1),
                ])
                .split(size);

            // 1. TOP HEADER
            let time_str = Local::now().format("%H:%M:%S").to_string();
            if show_boxed_header {
                let top_block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.inactive_border))
                    .title(Span::styled(" rinode live ", Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)))
                    .title_alignment(Alignment::Left);

                let top_line = Line::from(vec![
                    Span::styled(" • ", Style::default().fg(theme.accent)),
                    Span::styled(format!("Time: {}  │  ", time_str), Style::default().fg(theme.value_fg)),
                    Span::styled(
                        format!("{} preserved items  │  ", entries.len()),
                        Style::default().fg(theme.status_preserved).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{} in history  │  ", history_entries.len()),
                        Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("Active: {}  │  ", if active_pane == ActivePane::Preserved { "Preserved Vault" } else { "History" }),
                        Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(format!("[t] Theme: {} ", theme.name), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                ]);
                let header_widget = Paragraph::new(vec![top_line]).block(top_block);
                f.render_widget(header_widget, root_chunks[0]);
            } else {
                let header_line = Line::from(vec![
                    Span::styled("• ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                    Span::styled("rinode live  ", Style::default().fg(theme.unselected_row_fg).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("[{}]  ", time_str), Style::default().fg(theme.value_fg)),
                    Span::styled(
                        format!("{} preserved", entries.len()),
                        Style::default().fg(theme.status_preserved).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  |  ", Style::default().fg(theme.inactive_border)),
                    Span::styled(
                        format!("{} in history", history_entries.len()),
                        Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("  |  ", Style::default().fg(theme.inactive_border)),
                    Span::styled(format!("[t] Theme: {}", theme.name), Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                ]);
                let header_widget = Paragraph::new(vec![header_line]);
                f.render_widget(header_widget, root_chunks[0]);
            }

            // 2. CENTER SPLIT (LHS: Table, RHS: Details & History)
            let center_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(root_chunks[1]);

            // LHS Table
            let selected_idx = table_state.selected().unwrap_or(0);
            let history_selected_idx = history_table_state.selected().unwrap_or(0);

            let header_cells = ["ID", "NAME", "SIZE", "DELETED AT", "INODE"]
                .iter()
                .map(|h| {
                    let (color, modifier) = if active_pane == ActivePane::Preserved {
                        (theme.header_fg, Modifier::BOLD)
                    } else {
                        (theme.inactive_title, Modifier::empty())
                    };
                    Span::styled(*h, Style::default().fg(color).add_modifier(modifier))
                });
            let table_header = Row::new(header_cells).height(1).bottom_margin(1);

            let rows = entries.iter().enumerate().map(|(i, entry)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected && active_pane == ActivePane::Preserved {
                    "▶ "
                } else {
                    "  "
                };

                let size_text = if entry.is_directory {
                    "<DIR>".to_string()
                } else if entry.link_type == "SYMLINK" {
                    "<SYMLINK>".to_string()
                } else {
                    format_bytes(entry.file_size)
                };

                let date_text = entry.deleted_at.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string();

                let (prefix_style, name_style) = if is_selected && active_pane == ActivePane::Preserved {
                    (
                        Style::default().fg(theme.cursor_active).add_modifier(Modifier::BOLD),
                        Style::default().fg(theme.selected_row_fg).add_modifier(Modifier::BOLD),
                    )
                } else {
                    (
                        Style::default().fg(theme.unselected_row_fg),
                        Style::default().fg(theme.unselected_row_fg),
                    )
                };

                Row::new(vec![
                    Cell::from(Span::styled(entry.id.to_string(), name_style)),
                    Cell::from(Line::from(vec![
                        Span::styled(prefix, prefix_style),
                        Span::styled(&entry.filename, name_style),
                    ])),
                    Cell::from(Span::styled(size_text, name_style)),
                    Cell::from(Span::styled(date_text, name_style)),
                    Cell::from(Span::styled(entry.inode_no.to_string(), name_style)),
                ])
            });

            let preserved_border_style = if active_pane == ActivePane::Preserved {
                Style::default().fg(theme.active_border).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.inactive_border)
            };

            let preserved_title = if active_pane == ActivePane::Preserved {
                Line::from(vec![
                    Span::styled(" PRESERVED VAULT ", Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("({}) ", entries.len()), Style::default().fg(theme.value_fg).add_modifier(Modifier::BOLD)),
                    Span::styled("[ACTIVE] ", Style::default().fg(theme.active_badge_fg).bg(theme.active_badge_bg).add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!(" PRESERVED VAULT ({}) ", entries.len()), Style::default().fg(theme.inactive_title)),
                ])
            };

            let preserved_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(preserved_border_style)
                .title(preserved_title);

            if entries.is_empty() {
                let empty_para = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  Vault is currently empty.", Style::default().fg(theme.accent))),
                    Line::from(Span::styled("  Files deleted via 'rinode rm' will appear here.", Style::default().fg(theme.value_fg))),
                ])
                .block(preserved_block);
                f.render_widget(empty_para, center_chunks[0]);
            } else {
                let table = Table::new(
                    rows,
                    [
                        Constraint::Length(5),
                        Constraint::Fill(1),
                        Constraint::Length(10),
                        Constraint::Length(17),
                        Constraint::Length(10),
                    ],
                )
                .header(table_header)
                .block(preserved_block);

                f.render_widget(table, center_chunks[0]);
            }

            // RHS Vertical Split: Top Details, Bottom History
            let details_height = if root_chunks[1].height > 25 {
                15
            } else {
                (root_chunks[1].height / 2).max(8)
            };
            let right_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(details_height),
                    Constraint::Min(6),
                ])
                .split(center_chunks[1]);

            let (current_entry, is_history_view) = match active_pane {
                ActivePane::Preserved => (entries.get(selected_idx), false),
                ActivePane::History => (history_entries.get(history_selected_idx), true),
            };

            let details_border_style = Style::default().fg(theme.inactive_border);

            let details_title = if is_history_view {
                if let Some(entry) = current_entry {
                    let (status_text, status_color) = match entry.status.as_str() {
                        "RESTORED" => ("HISTORY: RESTORED", theme.status_restored),
                        "PURGED" => ("HISTORY: PURGED", theme.status_purged),
                        "EXCLUDED" => ("HISTORY: EXCLUDED", theme.status_excluded),
                        _ => ("HISTORY", theme.secondary),
                    };
                    Line::from(vec![
                        Span::styled(" ENTRY DETAILS ", Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("• {} ", status_text), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled(" ENTRY DETAILS • HISTORY ", Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)),
                    ])
                }
            } else {
                Line::from(vec![
                    Span::styled(" ENTRY DETAILS ", Style::default().fg(theme.header_fg).add_modifier(Modifier::BOLD)),
                    Span::styled("• PRESERVED VAULT ", Style::default().fg(theme.status_preserved).add_modifier(Modifier::BOLD)),
                ])
            };

            let details_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(details_border_style)
                .title(details_title);

            if let Some(entry) = current_entry {
                let file_type = if entry.is_directory {
                    "DIR"
                } else if entry.link_type == "SYMLINK" {
                    "SYMLINK"
                } else {
                    "FILE"
                };

                let source_badge = if is_history_view {
                    let (status_text, status_color) = match entry.status.as_str() {
                        "RESTORED" => ("HISTORY / RESTORED", theme.status_restored),
                        "PURGED" => ("HISTORY / PURGED", theme.status_purged),
                        "EXCLUDED" => ("HISTORY / EXCLUDED", theme.status_excluded),
                        _ => ("HISTORY", theme.secondary),
                    };
                    Span::styled(format!("  [SOURCE: {}]", status_text), Style::default().fg(status_color).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("  [SOURCE: PRESERVED VAULT]", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                };

                let status_color = match entry.status.as_str() {
                    "RESTORED" => theme.status_restored,
                    "PURGED" => theme.status_purged,
                    "EXCLUDED" => theme.status_excluded,
                    _ => theme.secondary,
                };

                let mut details_lines = vec![
                    Line::from(vec![
                        Span::styled("• ", Style::default().fg(theme.accent)),
                        Span::styled(&entry.filename, Style::default().fg(theme.value_fg).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  [{}]", file_type), Style::default().fg(theme.status_preserved).add_modifier(Modifier::BOLD)),
                        source_badge,
                    ]),
                    Line::from(vec![
                        Span::styled("Path:        ", Style::default().fg(theme.label_fg)),
                        Span::styled(&entry.original_path, Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("Inode:       ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.inode_no.to_string(), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("Device:      ", Style::default().fg(theme.label_fg)),
                        Span::styled(
                            format!("{}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id),
                            Style::default().fg(theme.value_fg),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Size:        ", Style::default().fg(theme.label_fg)),
                        Span::styled(
                            format!("{} ({} bytes)", format_bytes(entry.file_size), entry.file_size),
                            Style::default().fg(theme.value_fg),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Permissions: ", Style::default().fg(theme.label_fg)),
                        Span::styled(format!("{:04o}", entry.mode), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("Owner:       ", Style::default().fg(theme.label_fg)),
                        Span::styled(format!("UID {} / GID {}", entry.uid, entry.gid), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("Deleted:     ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.deleted_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string(), Style::default().fg(theme.value_fg)),
                    ]),
                ];

                if let Some(restored_at) = entry.restored_at {
                    details_lines.push(Line::from(vec![
                        Span::styled("Restored:    ", Style::default().fg(theme.label_fg)),
                        Span::styled(restored_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string(), Style::default().fg(theme.status_restored)),
                    ]));
                }

                if let Some(purged_at) = entry.purged_at {
                    details_lines.push(Line::from(vec![
                        Span::styled("Purged:      ", Style::default().fg(theme.label_fg)),
                        Span::styled(purged_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S").to_string(), Style::default().fg(theme.status_purged)),
                    ]));
                }

                details_lines.push(Line::from(vec![
                    Span::styled("Status:      ", Style::default().fg(theme.label_fg)),
                    Span::styled(&entry.status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                ]));

                if let Some(target) = &entry.symlink_target {
                    details_lines.push(Line::from(vec![
                        Span::styled("Symlink To:  ", Style::default().fg(theme.label_fg)),
                        Span::styled(target, Style::default().fg(theme.warning)),
                    ]));
                }

                if let Some(fp) = &entry.quick_fingerprint {
                    details_lines.push(Line::from(vec![
                        Span::styled("Fingerprint: ", Style::default().fg(theme.label_fg)),
                        Span::styled(fp, Style::default().fg(theme.value_fg)),
                    ]));
                }

                let vault_display = if entry.status == "EXCLUDED" {
                    "(none - excluded by rule)"
                } else {
                    &entry.vault_path
                };
                details_lines.push(Line::from(vec![
                    Span::styled("Vault:       ", Style::default().fg(theme.label_fg)),
                    Span::styled(vault_display, Style::default().fg(theme.value_fg)),
                ]));

                let details_para = Paragraph::new(details_lines).block(details_block);
                f.render_widget(details_para, right_chunks[0]);
            } else {
                let empty_para = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  No file selected.", Style::default().fg(theme.accent))),
                ]).block(details_block);
                f.render_widget(empty_para, right_chunks[0]);
            }

            // Bottom Right Pane: HISTORY
            let history_border_style = if active_pane == ActivePane::History {
                Style::default().fg(theme.active_border).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.inactive_border)
            };

            let history_title = if active_pane == ActivePane::History {
                Line::from(vec![
                    Span::styled(" HISTORY ", Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("({}) ", history_entries.len()), Style::default().fg(theme.value_fg).add_modifier(Modifier::BOLD)),
                    Span::styled("[ACTIVE] ", Style::default().fg(theme.active_badge_fg).bg(theme.active_badge_bg).add_modifier(Modifier::BOLD)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!(" HISTORY ({}) ", history_entries.len()), Style::default().fg(theme.inactive_title)),
                ])
            };

            let history_block = Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(history_border_style)
                .title(history_title);

            if history_entries.is_empty() {
                let empty_para = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  No records in history.", Style::default().fg(theme.accent))),
                    Line::from(Span::styled("  Restored or purged files will appear here.", Style::default().fg(theme.value_fg))),
                ])
                .block(history_block);
                f.render_widget(empty_para, right_chunks[1]);
            } else {
                let history_header = Row::new(["ID", "NAME", "INODE", "STATUS", "TIME"].iter().map(|h| {
                    let (color, modifier) = if active_pane == ActivePane::History {
                        (theme.header_fg, Modifier::BOLD)
                    } else {
                        (theme.inactive_title, Modifier::empty())
                    };
                    Span::styled(*h, Style::default().fg(color).add_modifier(modifier))
                })).height(1);

                let history_rows = history_entries.iter().enumerate().map(|(i, entry)| {
                    let is_selected = i == history_selected_idx;
                    let prefix = if is_selected && active_pane == ActivePane::History {
                        "▶ "
                    } else {
                        "  "
                    };

                    let (prefix_style, name_style) = if is_selected && active_pane == ActivePane::History {
                        (
                            Style::default().fg(theme.cursor_active).add_modifier(Modifier::BOLD),
                            Style::default().fg(theme.selected_row_fg).add_modifier(Modifier::BOLD),
                        )
                    } else {
                        (
                            Style::default().fg(theme.unselected_row_fg),
                            Style::default().fg(theme.unselected_row_fg),
                        )
                    };

                    let status_color = match entry.status.as_str() {
                        "RESTORED" => theme.status_restored,
                        "PURGED" => theme.status_purged,
                        "EXCLUDED" => theme.status_excluded,
                        _ => theme.secondary,
                    };

                    let time_dt = if entry.status == "RESTORED" {
                        entry.restored_at.unwrap_or(entry.deleted_at)
                    } else if entry.status == "PURGED" {
                        entry.purged_at.unwrap_or(entry.deleted_at)
                    } else {
                        entry.deleted_at
                    };

                    let pane_w = right_chunks[1].width;
                    let time_format = if pane_w >= 46 {
                        "%m-%d %H:%M"
                    } else {
                        "%H:%M"
                    };
                    let time_str = time_dt.with_timezone(&Local).format(time_format).to_string();

                    Row::new(vec![
                        Cell::from(Span::styled(entry.id.to_string(), name_style)),
                        Cell::from(Line::from(vec![
                            Span::styled(prefix, prefix_style),
                            Span::styled(&entry.filename, name_style),
                        ])),
                        Cell::from(Span::styled(entry.inode_no.to_string(), name_style)),
                        Cell::from(Span::styled(&entry.status, Style::default().fg(status_color).add_modifier(Modifier::BOLD))),
                        Cell::from(Span::styled(time_str, name_style)),
                    ])
                });

                let pane_w = right_chunks[1].width;
                let time_len = if pane_w >= 46 { 11 } else { 5 };

                let history_table = Table::new(
                    history_rows,
                    [
                        Constraint::Length(4),
                        Constraint::Fill(1),
                        Constraint::Length(8),
                        Constraint::Length(8),
                        Constraint::Length(time_len),
                    ],
                )
                .header(history_header)
                .block(history_block);

                f.render_widget(history_table, right_chunks[1]);
            }

            // 3. BOTTOM FOOTER
            let status_span = if let Some(msg) = &status_message {
                Span::styled(format!("  {}  ", msg), Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            };

            let footer_nav = match view_mode {
                ViewMode::Browsing => {
                    if active_pane == ActivePane::Preserved {
                        Line::from(vec![
                            Span::styled("[Tab/l] Switch to History  ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [↑/↓/j/k] Navigate  ", Style::default().fg(theme.value_fg)),
                            Span::styled("|  [Enter] Menu  ", Style::default().fg(theme.value_fg)),
                            Span::styled("|  [r] Restore  ", Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [x] Purge  ", Style::default().fg(theme.status_purged).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [t] Theme  ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [e] Rules  ", Style::default().fg(theme.header_fg)),
                            Span::styled("|  [q] Quit", Style::default().fg(theme.warning)),
                            status_span,
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled("[Tab/h] Switch to Preserved  ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [↑/↓/j/k] Navigate  ", Style::default().fg(theme.value_fg)),
                            Span::styled("|  [Enter] Inspect  ", Style::default().fg(theme.value_fg)),
                            Span::styled("|  [t] Theme  ", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [e] Rules  ", Style::default().fg(theme.header_fg)),
                            Span::styled("|  [q] Quit", Style::default().fg(theme.warning)),
                            status_span,
                        ])
                    }
                }
                ViewMode::ActionMenu => Line::from(vec![
                    Span::styled("[↑/↓/1-5] Choose Option  ", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                    Span::styled("|  [Enter] Execute  ", Style::default().fg(theme.value_fg)),
                    Span::styled("|  [Esc/q] Close Menu", Style::default().fg(theme.warning)),
                    status_span,
                ]),
                ViewMode::InspectModal => Line::from(vec![
                    Span::styled("[Esc/q/Enter] Close Inspection", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                ]),
                ViewMode::ExclusionModal => Line::from(vec![
                    Span::styled("[Esc/q/e] Close Rules View", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
                ]),
            };

            let footer_widget = Paragraph::new(vec![footer_nav]);
            f.render_widget(footer_widget, root_chunks[2]);

            // 4. ACTION SUBMENU (Floating Modal)
            if view_mode == ViewMode::ActionMenu {
                if let Some(entry) = entries.get(selected_idx) {
                    let popup_area = centered_rect(50, 40, size);
                    f.render_widget(Clear, popup_area);

                    let title = format!(" Actions: {} (ID: {}) ", entry.filename, entry.id);
                    let mut modal_block = Block::default()
                        .title(Span::styled(title, Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme.active_border));

                    if let Some(bg_color) = theme.bg {
                        modal_block = modal_block.style(Style::default().bg(bg_color));
                    }

                    let options = [
                        "1. Restore (Consume & Move back)",
                        "2. Restore (Keep vault copy / Reflink)",
                        "3. Inspect raw details",
                        "4. Purge permanently",
                        "5. Copy original path",
                    ];

                    let mut menu_lines = vec![Line::from("")];
                    for (i, opt) in options.iter().enumerate() {
                        let is_active = i == action_index;
                        let prefix = if is_active { "▶ " } else { "  " };
                        let style = if is_active {
                            Style::default().fg(theme.selected_row_fg).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(theme.value_fg)
                        };
                        menu_lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(*opt, style),
                        ]));
                    }
                    menu_lines.push(Line::from(""));
                    menu_lines.push(Line::from(Span::styled(
                        "  [1-5] Choose  |  [Esc] Close",
                        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                    )));

                    let menu_widget = Paragraph::new(menu_lines).block(modal_block);
                    f.render_widget(menu_widget, popup_area);
                }
            }

            // 5. INSPECT MODAL
            if view_mode == ViewMode::InspectModal {
                let current_inspected = match active_pane {
                    ActivePane::Preserved => entries.get(selected_idx),
                    ActivePane::History => history_entries.get(history_selected_idx),
                };
                if let Some(entry) = current_inspected {
                    let popup_area = centered_rect(65, 60, size);
                    f.render_widget(Clear, popup_area);

                    let mut modal_block = Block::default()
                        .title(Span::styled(
                            format!(" Metadata Inspection: {} ", entry.filename),
                            Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD),
                        ))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(theme.active_border));

                    if let Some(bg_color) = theme.bg {
                        modal_block = modal_block.style(Style::default().bg(bg_color));
                    }

                    let status_color = match entry.status.as_str() {
                        "RESTORED" => theme.status_restored,
                        "PURGED" => theme.status_purged,
                        "EXCLUDED" => theme.status_excluded,
                        _ => theme.secondary,
                    };

                    let mut inspect_lines = vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("  Database ID:        ", Style::default().fg(theme.label_fg)),
                            Span::styled(entry.id.to_string(), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Filename:           ", Style::default().fg(theme.label_fg)),
                            Span::styled(&entry.filename, Style::default().fg(theme.value_fg).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Original Path:      ", Style::default().fg(theme.label_fg)),
                            Span::styled(&entry.original_path, Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Inode Number:       ", Style::default().fg(theme.label_fg)),
                            Span::styled(entry.inode_no.to_string(), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Device / Mount:     ", Style::default().fg(theme.label_fg)),
                            Span::styled(format!("{}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Size (bytes):       ", Style::default().fg(theme.label_fg)),
                            Span::styled(format!("{} ({} bytes)", format_bytes(entry.file_size), entry.file_size), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Permissions (Oct):  ", Style::default().fg(theme.label_fg)),
                            Span::styled(format!("{:04o}", entry.mode), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Owner UID / GID:    ", Style::default().fg(theme.label_fg)),
                            Span::styled(format!("{} / {}", entry.uid, entry.gid), Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Link/Move Type:     ", Style::default().fg(theme.label_fg)),
                            Span::styled(&entry.link_type, Style::default().fg(theme.value_fg)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Vault Status:       ", Style::default().fg(theme.label_fg)),
                            Span::styled(&entry.status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Deletion Time:      ", Style::default().fg(theme.label_fg)),
                            Span::styled(entry.deleted_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %z").to_string(), Style::default().fg(theme.value_fg)),
                        ]),
                    ];

                    if let Some(restored_at) = entry.restored_at {
                        inspect_lines.push(Line::from(vec![
                            Span::styled("  Restoration Time:   ", Style::default().fg(theme.label_fg)),
                            Span::styled(restored_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %z").to_string(), Style::default().fg(theme.status_restored)),
                        ]));
                    }

                    if let Some(purged_at) = entry.purged_at {
                        inspect_lines.push(Line::from(vec![
                            Span::styled("  Purge Time:         ", Style::default().fg(theme.label_fg)),
                            Span::styled(purged_at.with_timezone(&Local).format("%Y-%m-%d %H:%M:%S %z").to_string(), Style::default().fg(theme.status_purged)),
                        ]));
                    }

                    let vault_display = if entry.status == "EXCLUDED" {
                        "(none - excluded by rule)"
                    } else {
                        &entry.vault_path
                    };

                    inspect_lines.push(Line::from(vec![
                        Span::styled("  Fast Fingerprint:   ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.quick_fingerprint.as_deref().unwrap_or("none"), Style::default().fg(theme.value_fg)),
                    ]));
                    inspect_lines.push(Line::from(vec![
                        Span::styled("  Vault File Path:    ", Style::default().fg(theme.label_fg)),
                        Span::styled(vault_display, Style::default().fg(theme.value_fg)),
                    ]));
                    inspect_lines.push(Line::from(""));
                    inspect_lines.push(Line::from(Span::styled("  Press [Esc/Enter] to close", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))));

                    let inspect_widget = Paragraph::new(inspect_lines).block(modal_block);
                    f.render_widget(inspect_widget, popup_area);
                }
            }

            // 6. EXCLUSION MODAL
            if view_mode == ViewMode::ExclusionModal {
                let popup_area = centered_rect(65, 60, size);
                f.render_widget(Clear, popup_area);

                let mut modal_block = Block::default()
                    .title(Span::styled(" Active Exclusion Rules ", Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)))
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(theme.active_border));

                if let Some(bg_color) = theme.bg {
                    modal_block = modal_block.style(Style::default().bg(bg_color));
                }

                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("  System Paths (Prefix match):", Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))),
                ];
                for p in &config.exclusions.system_paths {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(theme.accent)),
                        Span::styled(p.clone(), Style::default().fg(theme.value_fg)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Path Patterns (Regex):", Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD))));
                for r in &config.exclusions.path_regex {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(theme.status_restored)),
                        Span::styled(r.clone(), Style::default().fg(theme.value_fg)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Filename Patterns (Regex):", Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD))));
                for f in &config.exclusions.filename_regex {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(theme.secondary)),
                        Span::styled(f.clone(), Style::default().fg(theme.value_fg)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Press [Esc/q/e] to return to dashboard", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))));

                let excl_widget = Paragraph::new(lines).block(modal_block);
                f.render_widget(excl_widget, popup_area);
            }
        })?;

        // EVENT HANDLING
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match view_mode {
                    ViewMode::Browsing => match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('t') => {
                            theme_idx = (theme_idx + 1) % THEMES.len();
                            let cur = &THEMES[theme_idx];
                            let _ = config.set_theme(cur.id);
                            status_message = Some(format!("Switched theme to {}", cur.name));
                        }
                        KeyCode::Tab | KeyCode::BackTab => {
                            if active_pane == ActivePane::Preserved {
                                if !history_entries.is_empty() {
                                    active_pane = ActivePane::History;
                                    if history_table_state.selected().is_none() {
                                        history_table_state.select(Some(0));
                                    }
                                    status_message = None;
                                } else {
                                    status_message = Some("No records in history yet".into());
                                }
                            } else {
                                active_pane = ActivePane::Preserved;
                                status_message = None;
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if active_pane == ActivePane::Preserved {
                                if !history_entries.is_empty() {
                                    active_pane = ActivePane::History;
                                    if history_table_state.selected().is_none() {
                                        history_table_state.select(Some(0));
                                    }
                                    status_message = None;
                                } else {
                                    status_message = Some("No records in history yet".into());
                                }
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Left => {
                            if active_pane == ActivePane::History {
                                active_pane = ActivePane::Preserved;
                                status_message = None;
                            }
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            match active_pane {
                                ActivePane::Preserved => {
                                    if !entries.is_empty() {
                                        let curr = table_state.selected().unwrap_or(0);
                                        let next = (curr + 1).min(entries.len() - 1);
                                        table_state.select(Some(next));
                                        status_message = None;
                                    }
                                }
                                ActivePane::History => {
                                    if !history_entries.is_empty() {
                                        let curr = history_table_state.selected().unwrap_or(0);
                                        let next = (curr + 1).min(history_entries.len() - 1);
                                        history_table_state.select(Some(next));
                                        status_message = None;
                                    }
                                }
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            match active_pane {
                                ActivePane::Preserved => {
                                    if !entries.is_empty() {
                                        let curr = table_state.selected().unwrap_or(0);
                                        let prev = curr.saturating_sub(1);
                                        table_state.select(Some(prev));
                                        status_message = None;
                                    }
                                }
                                ActivePane::History => {
                                    if !history_entries.is_empty() {
                                        let curr = history_table_state.selected().unwrap_or(0);
                                        let prev = curr.saturating_sub(1);
                                        history_table_state.select(Some(prev));
                                        status_message = None;
                                    }
                                }
                            }
                        }
                        KeyCode::Enter => {
                            match active_pane {
                                ActivePane::Preserved => {
                                    if !entries.is_empty() {
                                        view_mode = ViewMode::ActionMenu;
                                        action_index = 0;
                                    }
                                }
                                ActivePane::History => {
                                    if !history_entries.is_empty() {
                                        view_mode = ViewMode::InspectModal;
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            last_data_version = -1;
                            status_message = Some("Refreshed".into());
                        }
                        KeyCode::F(5) => {
                            last_data_version = -1;
                            status_message = Some("Refreshed".into());
                        }
                        KeyCode::Char('r') => {
                            // Quick restore (applies to Preserved pane)
                            if active_pane == ActivePane::Preserved {
                                if let Some(idx) = table_state.selected() {
                                    if let Some(entry) = entries.get(idx) {
                                        match restore_entry(db, entry, false, false) {
                                            Ok(_) => {
                                                status_message = Some(format!("Restored '{}' to original path", entry.filename));
                                                entries = db.list_active(None)?;
                                                history_entries = db.list_history(None)?;
                                                last_data_version = db.get_data_version().unwrap_or(last_data_version);
                                                if history_table_state.selected().is_none() && !history_entries.is_empty() {
                                                    history_table_state.select(Some(0));
                                                }
                                                if idx >= entries.len() && !entries.is_empty() {
                                                    table_state.select(Some(entries.len() - 1));
                                                }
                                            }
                                            Err(e) => {
                                                status_message = Some(format!("Restore error: {}", e));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('x') => {
                            // Quick purge (applies to Preserved pane)
                            if active_pane == ActivePane::Preserved {
                                if let Some(idx) = table_state.selected() {
                                    if let Some(entry) = entries.get(idx) {
                                        db.purge_entry(entry).ok();
                                        status_message = Some(format!("Purged '{}'", entry.filename));
                                        entries = db.list_active(None)?;
                                        history_entries = db.list_history(None)?;
                                        last_data_version = db.get_data_version().unwrap_or(last_data_version);
                                        if history_table_state.selected().is_none() && !history_entries.is_empty() {
                                            history_table_state.select(Some(0));
                                        }
                                        if idx >= entries.len() && !entries.is_empty() {
                                            table_state.select(Some(entries.len() - 1));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('e') => {
                            view_mode = ViewMode::ExclusionModal;
                        }
                        _ => {}
                    },

                    ViewMode::ActionMenu => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => {
                            view_mode = ViewMode::Browsing;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            action_index = (action_index + 1).min(4);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            action_index = action_index.saturating_sub(1);
                        }
                        KeyCode::Char(c @ '1'..='5') => {
                            action_index = (c as usize) - ('1' as usize);
                            execute_action(action_index, &mut entries, &mut table_state, &mut history_entries, &mut history_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Enter => {
                            execute_action(action_index, &mut entries, &mut table_state, &mut history_entries, &mut history_table_state, &mut view_mode, &mut status_message, db);
                        }
                        _ => {}
                    },

                    ViewMode::InspectModal => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => {
                            view_mode = ViewMode::Browsing;
                        }
                        _ => {}
                    },

                    ViewMode::ExclusionModal => match key.code {
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('e') | KeyCode::Enter => {
                            view_mode = ViewMode::Browsing;
                        }
                        _ => {}
                    },
                }
            }
        }
    }

    Ok(())
}

fn execute_action(
    action_idx: usize,
    entries: &mut Vec<EntryRecord>,
    table_state: &mut TableState,
    history_entries: &mut Vec<EntryRecord>,
    history_table_state: &mut TableState,
    view_mode: &mut ViewMode,
    status_message: &mut Option<String>,
    db: &Db,
) {
    let sel = match table_state.selected() {
        Some(i) => i,
        None => return,
    };
    let entry = match entries.get(sel) {
        Some(e) => e.clone(),
        None => return,
    };

    match action_idx {
        0 => {
            // 1. Restore (Consume & Move back)
            match restore_entry(db, &entry, false, false) {
                Ok(_) => {
                    *status_message = Some(format!("Restored '{}' (consumed from vault)", entry.filename));
                    *entries = db.list_active(None).unwrap_or_default();
                    *history_entries = db.list_history(None).unwrap_or_default();
                    if history_table_state.selected().is_none() && !history_entries.is_empty() {
                        history_table_state.select(Some(0));
                    }
                    if sel >= entries.len() && !entries.is_empty() {
                        table_state.select(Some(entries.len() - 1));
                    }
                }
                Err(e) => {
                    *status_message = Some(format!("Restore error: {}", e));
                }
            }
            *view_mode = ViewMode::Browsing;
        }
        1 => {
            // 2. Restore (Keep vault copy / Reflink)
            match restore_entry(db, &entry, true, false) {
                Ok(_) => {
                    *status_message = Some(format!("Restored '{}' (vault copy kept)", entry.filename));
                }
                Err(e) => {
                    *status_message = Some(format!("Restore error: {}", e));
                }
            }
            *view_mode = ViewMode::Browsing;
        }
        2 => {
            // 3. Inspect raw details
            *view_mode = ViewMode::InspectModal;
        }
        3 => {
            // 4. Purge permanently
            db.purge_entry(&entry).ok();
            *status_message = Some(format!("Purged '{}'", entry.filename));
            *entries = db.list_active(None).unwrap_or_default();
            *history_entries = db.list_history(None).unwrap_or_default();
            if history_table_state.selected().is_none() && !history_entries.is_empty() {
                history_table_state.select(Some(0));
            }
            if sel >= entries.len() && !entries.is_empty() {
                table_state.select(Some(entries.len() - 1));
            }
            *view_mode = ViewMode::Browsing;
        }
        4 => {
            // 5. Copy original path
            *status_message = Some(format!("Path: {}", entry.original_path));
            *view_mode = ViewMode::Browsing;
        }
        _ => {}
    }
}
