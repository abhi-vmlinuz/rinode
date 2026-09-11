use chrono::Utc;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState},
    Terminal,
};
use std::io::stdout;
use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::db::{Db, EntryRecord};
use crate::restore::RestoreManager;

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
    Restored,
}

pub fn run_tui(db: &Db, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = main_loop(&mut terminal, db, config);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("TUI Error: {}", e);
    }
    Ok(())
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
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
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut entries = db.list_active(None)?;
    let mut restored_entries = db.list_restored(None)?;
    let mut table_state = TableState::default();
    if !entries.is_empty() {
        table_state.select(Some(0));
    }
    let mut restored_table_state = TableState::default();
    if !restored_entries.is_empty() {
        restored_table_state.select(Some(0));
    }

    let mut active_pane = ActivePane::Preserved;
    let mut view_mode = ViewMode::Browsing;
    let mut action_index: usize = 0;
    let mut status_message: Option<String> = None;

    loop {
        terminal.draw(|f| {
            let size = f.area();

            // Main vertical layout: Top Header (2 lines), Center Split (remaining), Bottom Footer (2 lines)
            let root_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(5),
                    Constraint::Length(2),
                ])
                .split(size);

            // 1. TOP HEADER
            let time_str = Utc::now().format("%H:%M:%S").to_string();
            let header_line = Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                Span::styled("rinode live  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!("[{}]  ", time_str), Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("{} preserved", entries.len()),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  |  ", Style::default().fg(Color::LightCyan)),
                Span::styled(
                    format!("{} restored", restored_entries.len()),
                    Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD),
                ),
            ]);
            let header_border = Line::from(Span::styled(
                "─".repeat(size.width as usize),
                Style::default().fg(Color::Cyan),
            ));
            let header_widget = Paragraph::new(vec![header_line, header_border]);
            f.render_widget(header_widget, root_chunks[0]);

            // 2. CENTER SPLIT (LHS: Table, RHS: Details & Restore History)
            let center_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(root_chunks[1]);

            // LHS Table
            let selected_idx = table_state.selected().unwrap_or(0);
            let restored_selected_idx = restored_table_state.selected().unwrap_or(0);

            let header_cells = ["ID", "NAME", "SIZE", "DELETED AT", "INODE"]
                .iter()
                .map(|h| {
                    let color = if active_pane == ActivePane::Preserved {
                        Color::LightCyan
                    } else {
                        Color::Cyan
                    };
                    Span::styled(*h, Style::default().fg(color).add_modifier(Modifier::BOLD))
                });
            let table_header = Row::new(header_cells).height(1).bottom_margin(1);

            let rows = entries.iter().enumerate().map(|(i, entry)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected {
                    if active_pane == ActivePane::Preserved { "▶ " } else { "▷ " }
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

                let name_display = format!("{}{}", prefix, entry.filename);
                let date_text = entry.deleted_at.format("%Y-%m-%d %H:%M").to_string();

                let style = if is_selected && active_pane == ActivePane::Preserved {
                    Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                Row::new(vec![
                    Span::styled(entry.id.to_string(), style),
                    Span::styled(name_display, style),
                    Span::styled(size_text, style),
                    Span::styled(date_text, style),
                    Span::styled(entry.inode_no.to_string(), style),
                ])
            });

            let table = Table::new(
                rows,
                [
                    Constraint::Length(5),
                    Constraint::Percentage(40),
                    Constraint::Length(10),
                    Constraint::Length(17),
                    Constraint::Min(8),
                ],
            )
            .header(table_header)
            .block(Block::default().borders(Borders::NONE));

            f.render_widget(table, center_chunks[0]);

            // RHS Vertical Split: Top Details, Bottom Restore History
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

            let (current_entry, is_restored_view) = match active_pane {
                ActivePane::Preserved => (entries.get(selected_idx), false),
                ActivePane::Restored => (restored_entries.get(restored_selected_idx), true),
            };

            let details_block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::Cyan));

            if let Some(entry) = current_entry {
                let file_type = if entry.is_directory {
                    "DIR"
                } else if entry.link_type == "SYMLINK" {
                    "SYMLINK"
                } else {
                    "FILE"
                };

                let title_text = if is_restored_view {
                    "ENTRY DETAILS (RESTORED)"
                } else {
                    "ENTRY DETAILS"
                };
                let title_color = if is_restored_view {
                    Color::LightGreen
                } else {
                    Color::LightCyan
                };

                let mut details_lines = vec![
                    Line::from(Span::styled(
                        title_text,
                        Style::default().fg(title_color).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(vec![
                        Span::styled("• ", Style::default().fg(Color::LightCyan)),
                        Span::styled(&entry.filename, Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  [{}]", file_type), Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Path:        ", Style::default().fg(Color::LightCyan)),
                        Span::styled(&entry.original_path, Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Inode:       ", Style::default().fg(Color::LightCyan)),
                        Span::styled(entry.inode_no.to_string(), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Device:      ", Style::default().fg(Color::LightCyan)),
                        Span::styled(
                            format!("{}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Size:        ", Style::default().fg(Color::LightCyan)),
                        Span::styled(
                            format!("{} ({} bytes)", format_bytes(entry.file_size), entry.file_size),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Permissions: ", Style::default().fg(Color::LightCyan)),
                        Span::styled(format!("{:04o}", entry.mode), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Owner:       ", Style::default().fg(Color::LightCyan)),
                        Span::styled(format!("UID {} / GID {}", entry.uid, entry.gid), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Deleted:     ", Style::default().fg(Color::LightCyan)),
                        Span::styled(entry.deleted_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(), Style::default().fg(Color::White)),
                    ]),
                ];

                if let Some(restored_at) = entry.restored_at {
                    details_lines.push(Line::from(vec![
                        Span::styled("Restored:    ", Style::default().fg(Color::LightCyan)),
                        Span::styled(restored_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(), Style::default().fg(Color::LightGreen)),
                    ]));
                }

                details_lines.push(Line::from(vec![
                    Span::styled("Status:      ", Style::default().fg(Color::LightCyan)),
                    Span::styled(&entry.status, Style::default().fg(if is_restored_view { Color::LightCyan } else { Color::LightGreen }).add_modifier(Modifier::BOLD)),
                ]));

                if let Some(target) = &entry.symlink_target {
                    details_lines.push(Line::from(vec![
                        Span::styled("Symlink To:  ", Style::default().fg(Color::LightCyan)),
                        Span::styled(target, Style::default().fg(Color::LightYellow)),
                    ]));
                }

                if let Some(fp) = &entry.quick_fingerprint {
                    details_lines.push(Line::from(vec![
                        Span::styled("Fingerprint: ", Style::default().fg(Color::LightCyan)),
                        Span::styled(fp, Style::default().fg(Color::White)),
                    ]));
                }

                details_lines.push(Line::from(""));
                details_lines.push(Line::from(vec![
                    Span::styled("Vault:       ", Style::default().fg(Color::LightCyan)),
                    Span::styled(&entry.vault_path, Style::default().fg(Color::White)),
                ]));

                let details_para = Paragraph::new(details_lines).block(details_block);
                f.render_widget(details_para, right_chunks[0]);
            } else {
                let empty_para = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  No file selected.", Style::default().fg(Color::LightCyan))),
                ]).block(details_block);
                f.render_widget(empty_para, right_chunks[0]);
            }

            // Bottom Right Pane: RESTORE HISTORY
            let history_border_color = if active_pane == ActivePane::Restored {
                Color::LightCyan
            } else {
                Color::Cyan
            };
            let history_title_color = if active_pane == ActivePane::Restored {
                Color::LightCyan
            } else {
                Color::LightGreen
            };

            let history_block = Block::default()
                .borders(Borders::LEFT | Borders::TOP)
                .border_style(Style::default().fg(history_border_color))
                .title(Span::styled(
                    format!(" RESTORE HISTORY ({}) ", restored_entries.len()),
                    Style::default().fg(history_title_color).add_modifier(Modifier::BOLD),
                ));

            if restored_entries.is_empty() {
                let empty_para = Paragraph::new(vec![
                    Line::from(""),
                    Line::from(Span::styled("  No restored files in history.", Style::default().fg(Color::LightCyan))),
                    Line::from(Span::styled("  Files restored with [r] will appear here.", Style::default().fg(Color::White))),
                ])
                .block(history_block);
                f.render_widget(empty_para, right_chunks[1]);
            } else {
                let history_header = Row::new(["ID", "NAME", "INODE", "RESTORED"].iter().map(|h| {
                    Span::styled(
                        *h,
                        Style::default()
                            .fg(if active_pane == ActivePane::Restored { Color::LightCyan } else { Color::Cyan })
                            .add_modifier(Modifier::BOLD),
                    )
                })).height(1);

                let history_rows = restored_entries.iter().enumerate().map(|(i, entry)| {
                    let is_sel = i == restored_selected_idx;
                    let prefix = if is_sel {
                        if active_pane == ActivePane::Restored { "▶ " } else { "▷ " }
                    } else {
                        "  "
                    };
                    let style = if is_sel && active_pane == ActivePane::Restored {
                        Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
                    } else if is_sel {
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };

                    let time_str = entry.restored_at
                        .map(|dt| dt.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| entry.deleted_at.format("%m-%d %H:%M").to_string());

                    let name_display = format!("{}{}", prefix, entry.filename);

                    Row::new(vec![
                        Span::styled(entry.id.to_string(), style),
                        Span::styled(name_display, style),
                        Span::styled(entry.inode_no.to_string(), style),
                        Span::styled(time_str, style),
                    ])
                });

                let history_table = Table::new(
                    history_rows,
                    [
                        Constraint::Length(4),
                        Constraint::Percentage(45),
                        Constraint::Length(10),
                        Constraint::Min(11),
                    ],
                )
                .header(history_header)
                .block(history_block);

                f.render_widget(history_table, right_chunks[1]);
            }

            // 3. BOTTOM FOOTER
            let footer_border = Line::from(Span::styled(
                "─".repeat(size.width as usize),
                Style::default().fg(Color::Cyan),
            ));

            let status_span = if let Some(msg) = &status_message {
                Span::styled(format!("  {}  ", msg), Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            };

            let footer_nav = match view_mode {
                ViewMode::Browsing => {
                    if active_pane == ActivePane::Preserved {
                        Line::from(vec![
                            Span::styled("[Tab/l] History Pane  ", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [↑/↓/j/k] Navigate  ", Style::default().fg(Color::White)),
                            Span::styled("|  [Enter] Menu  ", Style::default().fg(Color::White)),
                            Span::styled("|  [r] Restore  ", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [x] Purge  ", Style::default().fg(Color::LightRed).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [e] Rules  ", Style::default().fg(Color::LightCyan)),
                            Span::styled("|  [q] Quit", Style::default().fg(Color::LightYellow)),
                            status_span,
                        ])
                    } else {
                        Line::from(vec![
                            Span::styled("[Tab/h] Preserved Pane  ", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                            Span::styled("|  [↑/↓/j/k] Navigate  ", Style::default().fg(Color::White)),
                            Span::styled("|  [Enter] Inspect  ", Style::default().fg(Color::White)),
                            Span::styled("|  [e] Rules  ", Style::default().fg(Color::LightCyan)),
                            Span::styled("|  [q] Quit", Style::default().fg(Color::LightYellow)),
                            status_span,
                        ])
                    }
                }
                ViewMode::ActionMenu => Line::from(vec![
                    Span::styled("[↑/↓/1-5] Choose Option  ", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                    Span::styled("|  [Enter] Execute  ", Style::default().fg(Color::White)),
                    Span::styled("|  [Esc/q] Close Menu", Style::default().fg(Color::LightYellow)),
                    status_span,
                ]),
                ViewMode::InspectModal => Line::from(vec![
                    Span::styled("[Esc/q/Enter] Close Inspection", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                ]),
                ViewMode::ExclusionModal => Line::from(vec![
                    Span::styled("[Esc/q/e] Close Rules View", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)),
                ]),
            };

            let footer_widget = Paragraph::new(vec![footer_border, footer_nav]);
            f.render_widget(footer_widget, root_chunks[2]);

            // 4. ACTION SUBMENU (Floating Modal matching Image 2)
            if view_mode == ViewMode::ActionMenu {
                if let Some(entry) = entries.get(selected_idx) {
                    let popup_area = centered_rect(50, 40, size);
                    f.render_widget(Clear, popup_area);

                    let title = format!(" Actions: {} (ID: {}) ", entry.filename, entry.id);
                    let modal_block = Block::default()
                        .title(Span::styled(title, Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));

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
                            Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::White)
                        };
                        menu_lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(*opt, style),
                        ]));
                    }
                    menu_lines.push(Line::from(""));
                    menu_lines.push(Line::from(Span::styled(
                        "  [1-5] Choose  |  [Esc] Close",
                        Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
                    )));

                    let menu_widget = Paragraph::new(menu_lines).block(modal_block);
                    f.render_widget(menu_widget, popup_area);
                }
            }

            // 5. INSPECT MODAL
            if view_mode == ViewMode::InspectModal {
                let current_inspected = match active_pane {
                    ActivePane::Preserved => entries.get(selected_idx),
                    ActivePane::Restored => restored_entries.get(restored_selected_idx),
                };
                if let Some(entry) = current_inspected {
                    let popup_area = centered_rect(65, 60, size);
                    f.render_widget(Clear, popup_area);

                    let modal_block = Block::default()
                        .title(Span::styled(
                            format!(" Metadata Inspection: {} ", entry.filename),
                            Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD),
                        ))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));

                    let mut inspect_lines = vec![
                        Line::from(""),
                        Line::from(vec![
                            Span::styled("  Database ID:        ", Style::default().fg(Color::LightCyan)),
                            Span::styled(entry.id.to_string(), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Filename:           ", Style::default().fg(Color::LightCyan)),
                            Span::styled(&entry.filename, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Original Path:      ", Style::default().fg(Color::LightCyan)),
                            Span::styled(&entry.original_path, Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Inode Number:       ", Style::default().fg(Color::LightCyan)),
                            Span::styled(entry.inode_no.to_string(), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Device / Mount:     ", Style::default().fg(Color::LightCyan)),
                            Span::styled(format!("{}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Size (bytes):       ", Style::default().fg(Color::LightCyan)),
                            Span::styled(format!("{} ({} bytes)", format_bytes(entry.file_size), entry.file_size), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Permissions (Oct):  ", Style::default().fg(Color::LightCyan)),
                            Span::styled(format!("{:04o}", entry.mode), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Owner UID / GID:    ", Style::default().fg(Color::LightCyan)),
                            Span::styled(format!("{} / {}", entry.uid, entry.gid), Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Link/Move Type:     ", Style::default().fg(Color::LightCyan)),
                            Span::styled(&entry.link_type, Style::default().fg(Color::White)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Vault Status:       ", Style::default().fg(Color::LightCyan)),
                            Span::styled(&entry.status, Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD)),
                        ]),
                        Line::from(vec![
                            Span::styled("  Deletion Time:      ", Style::default().fg(Color::LightCyan)),
                            Span::styled(entry.deleted_at.to_rfc3339(), Style::default().fg(Color::White)),
                        ]),
                    ];

                    if let Some(restored_at) = entry.restored_at {
                        inspect_lines.push(Line::from(vec![
                            Span::styled("  Restoration Time:   ", Style::default().fg(Color::LightCyan)),
                            Span::styled(restored_at.to_rfc3339(), Style::default().fg(Color::LightGreen)),
                        ]));
                    }

                    inspect_lines.push(Line::from(vec![
                        Span::styled("  Fast Fingerprint:   ", Style::default().fg(Color::LightCyan)),
                        Span::styled(entry.quick_fingerprint.as_deref().unwrap_or("none"), Style::default().fg(Color::White)),
                    ]));
                    inspect_lines.push(Line::from(vec![
                        Span::styled("  Vault File Path:    ", Style::default().fg(Color::LightCyan)),
                        Span::styled(&entry.vault_path, Style::default().fg(Color::White)),
                    ]));
                    inspect_lines.push(Line::from(""));
                    inspect_lines.push(Line::from(Span::styled("  Press [Esc/Enter] to close", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD))));

                    let inspect_widget = Paragraph::new(inspect_lines).block(modal_block);
                    f.render_widget(inspect_widget, popup_area);
                }
            }

            // 6. EXCLUSION MODAL
            if view_mode == ViewMode::ExclusionModal {
                let popup_area = centered_rect(65, 60, size);
                f.render_widget(Clear, popup_area);

                let modal_block = Block::default()
                    .title(Span::styled(" Active Exclusion Rules ", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD)))
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan));

                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("  System Paths (Prefix match):", Style::default().fg(Color::LightYellow).add_modifier(Modifier::BOLD))),
                ];
                for p in &config.exclusions.system_paths {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(Color::LightCyan)),
                        Span::styled(p.clone(), Style::default().fg(Color::White)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Path Patterns (Regex):", Style::default().fg(Color::LightGreen).add_modifier(Modifier::BOLD))));
                for r in &config.exclusions.path_regex {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(Color::LightGreen)),
                        Span::styled(r.clone(), Style::default().fg(Color::White)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Filename Patterns (Regex):", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD))));
                for f in &config.exclusions.filename_regex {
                    lines.push(Line::from(vec![
                        Span::styled("    • ", Style::default().fg(Color::LightCyan)),
                        Span::styled(f.clone(), Style::default().fg(Color::White)),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Press [Esc/q/e] to return to dashboard", Style::default().fg(Color::LightCyan).add_modifier(Modifier::BOLD))));

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
                        KeyCode::Tab | KeyCode::BackTab => {
                            if active_pane == ActivePane::Preserved {
                                if !restored_entries.is_empty() {
                                    active_pane = ActivePane::Restored;
                                    if restored_table_state.selected().is_none() {
                                        restored_table_state.select(Some(0));
                                    }
                                    status_message = None;
                                } else {
                                    status_message = Some("No restored files in history yet".into());
                                }
                            } else {
                                active_pane = ActivePane::Preserved;
                                status_message = None;
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if active_pane == ActivePane::Preserved {
                                if !restored_entries.is_empty() {
                                    active_pane = ActivePane::Restored;
                                    if restored_table_state.selected().is_none() {
                                        restored_table_state.select(Some(0));
                                    }
                                    status_message = None;
                                } else {
                                    status_message = Some("No restored files in history yet".into());
                                }
                            }
                        }
                        KeyCode::Char('h') | KeyCode::Left => {
                            if active_pane == ActivePane::Restored {
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
                                ActivePane::Restored => {
                                    if !restored_entries.is_empty() {
                                        let curr = restored_table_state.selected().unwrap_or(0);
                                        let next = (curr + 1).min(restored_entries.len() - 1);
                                        restored_table_state.select(Some(next));
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
                                ActivePane::Restored => {
                                    if !restored_entries.is_empty() {
                                        let curr = restored_table_state.selected().unwrap_or(0);
                                        let prev = curr.saturating_sub(1);
                                        restored_table_state.select(Some(prev));
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
                                ActivePane::Restored => {
                                    if !restored_entries.is_empty() {
                                        view_mode = ViewMode::InspectModal;
                                    }
                                }
                            }
                        }
                        KeyCode::Char('r') => {
                            // Quick restore (applies to Preserved pane)
                            if active_pane == ActivePane::Preserved {
                                if let Some(idx) = table_state.selected() {
                                    if let Some(entry) = entries.get(idx) {
                                        let restore_mgr = RestoreManager::new(db);
                                        match restore_mgr.restore_entry(entry, false, false) {
                                            Ok(_) => {
                                                status_message = Some(format!("Restored '{}' to original path", entry.filename));
                                                entries = db.list_active(None)?;
                                                restored_entries = db.list_restored(None)?;
                                                if restored_table_state.selected().is_none() && !restored_entries.is_empty() {
                                                    restored_table_state.select(Some(0));
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
                                        let vault_path = Path::new(&entry.vault_path);
                                        if vault_path.exists() {
                                            if entry.is_directory {
                                                std::fs::remove_dir_all(vault_path).ok();
                                            } else {
                                                std::fs::remove_file(vault_path).ok();
                                            }
                                        }
                                        db.mark_purged(entry.id).ok();
                                        status_message = Some(format!("Purged '{}'", entry.filename));
                                        entries = db.list_active(None)?;
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
                        KeyCode::Char('1') => {
                            action_index = 0;
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('2') => {
                            action_index = 1;
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('3') => {
                            action_index = 2;
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('4') => {
                            action_index = 3;
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('5') => {
                            action_index = 4;
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Enter => {
                            execute_action(action_index, &mut entries, &mut table_state, &mut restored_entries, &mut restored_table_state, &mut view_mode, &mut status_message, db);
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
    restored_entries: &mut Vec<EntryRecord>,
    restored_table_state: &mut TableState,
    view_mode: &mut ViewMode,
    status_message: &mut Option<String>,
    db: &Db,
) {
    let sel = match table_state.selected() {
        Some(s) => s,
        None => return,
    };
    let entry = match entries.get(sel) {
        Some(e) => e.clone(),
        None => return,
    };

    match action_idx {
        0 => {
            // Restore (Consume)
            let restore_mgr = RestoreManager::new(db);
            match restore_mgr.restore_entry(&entry, false, false) {
                Ok(_) => {
                    *status_message = Some(format!("Restored '{}' to original path", entry.filename));
                    *entries = db.list_active(None).unwrap_or_default();
                    *restored_entries = db.list_restored(None).unwrap_or_default();
                    if restored_table_state.selected().is_none() && !restored_entries.is_empty() {
                        restored_table_state.select(Some(0));
                    }
                    if sel >= entries.len() && !entries.is_empty() {
                        table_state.select(Some(entries.len() - 1));
                    }
                    *view_mode = ViewMode::Browsing;
                }
                Err(e) => {
                    *status_message = Some(format!("Error: {}", e));
                    *view_mode = ViewMode::Browsing;
                }
            }
        }
        1 => {
            // Restore (Keep vault copy / Reflink)
            let restore_mgr = RestoreManager::new(db);
            match restore_mgr.restore_entry(&entry, true, false) {
                Ok(_) => {
                    *status_message = Some(format!("Restored snapshot of '{}' (reflink/copy)", entry.filename));
                    *entries = db.list_active(None).unwrap_or_default();
                    *restored_entries = db.list_restored(None).unwrap_or_default();
                    if restored_table_state.selected().is_none() && !restored_entries.is_empty() {
                        restored_table_state.select(Some(0));
                    }
                    *view_mode = ViewMode::Browsing;
                }
                Err(e) => {
                    *status_message = Some(format!("Error: {}", e));
                    *view_mode = ViewMode::Browsing;
                }
            }
        }
        2 => {
            // Inspect raw details
            *view_mode = ViewMode::InspectModal;
        }
        3 => {
            // Purge permanently
            let vault_path = Path::new(&entry.vault_path);
            if vault_path.exists() {
                if entry.is_directory {
                    std::fs::remove_dir_all(vault_path).ok();
                } else {
                    std::fs::remove_file(vault_path).ok();
                }
            }
            db.mark_purged(entry.id).ok();
            *status_message = Some(format!("Purged '{}'", entry.filename));
            *entries = db.list_active(None).unwrap_or_default();
            if sel >= entries.len() && !entries.is_empty() {
                table_state.select(Some(entries.len() - 1));
            }
            *view_mode = ViewMode::Browsing;
        }
        4 => {
            // Copy original path / display
            *status_message = Some(format!("Path: {}", entry.original_path));
            *view_mode = ViewMode::Browsing;
        }
        _ => {}
    }
}
