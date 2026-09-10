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
    let mut table_state = TableState::default();
    if !entries.is_empty() {
        table_state.select(Some(0));
    }

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
                Span::styled("• ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled("rinode live  ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!("[{}]  ", time_str), Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{} preserved files", entries.len()),
                    Style::default().fg(Color::Gray),
                ),
            ]);
            let header_border = Line::from(Span::styled(
                "─".repeat(size.width as usize),
                Style::default().fg(Color::DarkGray),
            ));
            let header_widget = Paragraph::new(vec![header_line, header_border]);
            f.render_widget(header_widget, root_chunks[0]);

            // 2. CENTER SPLIT (LHS: Table, RHS: Details)
            let center_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(root_chunks[1]);

            // LHS Table
            let selected_idx = table_state.selected().unwrap_or(0);
            let header_cells = ["ID", "NAME", "SIZE", "DELETED AT", "INODE"]
                .iter()
                .map(|h| Span::styled(*h, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
            let table_header = Row::new(header_cells).height(1).bottom_margin(1);

            let rows = entries.iter().enumerate().map(|(i, entry)| {
                let is_selected = i == selected_idx;
                let prefix = if is_selected { "▶ " } else { "  " };

                let size_text = if entry.is_directory {
                    "<DIR>".to_string()
                } else if entry.link_type == "SYMLINK" {
                    "<SYMLINK>".to_string()
                } else {
                    format_bytes(entry.file_size)
                };

                let name_display = format!("{}{}", prefix, entry.filename);
                let date_text = entry.deleted_at.format("%Y-%m-%d %H:%M").to_string();

                let style = if is_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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
                    Constraint::Length(12),
                    Constraint::Length(18),
                    Constraint::Min(8),
                ],
            )
            .header(table_header)
            .block(Block::default().borders(Borders::NONE));

            f.render_widget(table, center_chunks[0]);

            // RHS Details Pane
            let details_block = Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray));

            if let Some(entry) = entries.get(selected_idx) {
                let file_type = if entry.is_directory {
                    "DIR"
                } else if entry.link_type == "SYMLINK" {
                    "SYMLINK"
                } else {
                    "FILE"
                };

                let mut details_lines = vec![
                    Line::from(Span::styled(
                        "ENTRY DETAILS",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )),
                    Line::from(vec![
                        Span::styled("• ", Style::default().fg(Color::Cyan)),
                        Span::styled(&entry.filename, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                        Span::styled(format!("  [{}]", file_type), Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Path:        ", Style::default().fg(Color::LightBlue)),
                        Span::styled(&entry.original_path, Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Inode:       ", Style::default().fg(Color::LightBlue)),
                        Span::styled(entry.inode_no.to_string(), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Device:      ", Style::default().fg(Color::LightBlue)),
                        Span::styled(
                            format!("{}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Size:        ", Style::default().fg(Color::LightBlue)),
                        Span::styled(
                            format!("{} ({} bytes)", format_bytes(entry.file_size), entry.file_size),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    Line::from(vec![
                        Span::styled("Permissions: ", Style::default().fg(Color::LightBlue)),
                        Span::styled(format!("{:04o}", entry.mode), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Owner:       ", Style::default().fg(Color::LightBlue)),
                        Span::styled(format!("UID {} / GID {}", entry.uid, entry.gid), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Deleted:     ", Style::default().fg(Color::LightBlue)),
                        Span::styled(entry.deleted_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(), Style::default().fg(Color::White)),
                    ]),
                    Line::from(vec![
                        Span::styled("Status:      ", Style::default().fg(Color::LightBlue)),
                        Span::styled(&entry.status, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    ]),
                ];

                if let Some(target) = &entry.symlink_target {
                    details_lines.push(Line::from(vec![
                        Span::styled("Symlink To:  ", Style::default().fg(Color::LightBlue)),
                        Span::styled(target, Style::default().fg(Color::Yellow)),
                    ]));
                }

                if let Some(fp) = &entry.quick_fingerprint {
                    details_lines.push(Line::from(vec![
                        Span::styled("Fingerprint: ", Style::default().fg(Color::LightBlue)),
                        Span::styled(fp, Style::default().fg(Color::White)),
                    ]));
                }

                details_lines.push(Line::from(""));
                details_lines.push(Line::from(vec![
                    Span::styled("Vault:       ", Style::default().fg(Color::LightBlue)),
                    Span::styled(&entry.vault_path, Style::default().fg(Color::White)),
                ]));

                let details_para = Paragraph::new(details_lines).block(details_block);
                f.render_widget(details_para, center_chunks[1]);
            } else {
                let empty_para = Paragraph::new("No files in vault.").block(details_block);
                f.render_widget(empty_para, center_chunks[1]);
            }

            // 3. BOTTOM FOOTER
            let footer_border = Line::from(Span::styled(
                "─".repeat(size.width as usize),
                Style::default().fg(Color::Gray),
            ));

            let status_span = if let Some(msg) = &status_message {
                Span::styled(format!("  {}  ", msg), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            };

            let footer_nav = match view_mode {
                ViewMode::Browsing => Line::from(vec![
                    Span::styled("[↑/↓/j/k] Navigate  ", Style::default().fg(Color::LightCyan)),
                    Span::styled("|  [Enter] Menu  ", Style::default().fg(Color::White)),
                    Span::styled("|  [r] Restore  ", Style::default().fg(Color::White)),
                    Span::styled("|  [x] Purge  ", Style::default().fg(Color::White)),
                    Span::styled("|  [e] Rules  ", Style::default().fg(Color::Cyan)),
                    Span::styled("|  [q] Quit", Style::default().fg(Color::Gray)),
                    status_span,
                ]),
                ViewMode::ActionMenu => Line::from(vec![
                    Span::styled("[↑/↓/1-5] Choose Option  ", Style::default().fg(Color::LightCyan)),
                    Span::styled("|  [Enter] Execute  ", Style::default().fg(Color::White)),
                    Span::styled("|  [Esc/q] Close Menu", Style::default().fg(Color::Gray)),
                    status_span,
                ]),
                ViewMode::InspectModal => Line::from(vec![
                    Span::styled("[Esc/q/Enter] Close Inspection", Style::default().fg(Color::White)),
                ]),
                ViewMode::ExclusionModal => Line::from(vec![
                    Span::styled("[Esc/q/e] Close Rules View", Style::default().fg(Color::White)),
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
                        .title(title)
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::White));

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
                            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
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
                        Style::default().fg(Color::LightCyan),
                    )));

                    let menu_widget = Paragraph::new(menu_lines).block(modal_block);
                    f.render_widget(menu_widget, popup_area);
                }
            }

            // 5. INSPECT MODAL
            if view_mode == ViewMode::InspectModal {
                if let Some(entry) = entries.get(selected_idx) {
                    let popup_area = centered_rect(65, 60, size);
                    f.render_widget(Clear, popup_area);

                    let modal_block = Block::default()
                        .title(format!(" Metadata Inspection: {} ", entry.filename))
                        .title_alignment(Alignment::Center)
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Cyan));

                    let inspect_lines = vec![
                        Line::from(""),
                        Line::from(format!("  Database ID:        {}", entry.id)),
                        Line::from(format!("  Filename:           {}", entry.filename)),
                        Line::from(format!("  Original Path:      {}", entry.original_path)),
                        Line::from(format!("  Inode Number:       {}", entry.inode_no)),
                        Line::from(format!("  Device / Mount:     {}:{} (mnt_id: {})", entry.dev_major, entry.dev_minor, entry.mnt_id)),
                        Line::from(format!("  Size (bytes):       {} ({} bytes)", format_bytes(entry.file_size), entry.file_size)),
                        Line::from(format!("  Permissions (Oct):  {:04o}", entry.mode)),
                        Line::from(format!("  Owner UID / GID:    {} / {}", entry.uid, entry.gid)),
                        Line::from(format!("  Link/Move Type:     {}", entry.link_type)),
                        Line::from(format!("  Vault Status:       {}", entry.status)),
                        Line::from(format!("  Deletion Time:      {}", entry.deleted_at.to_rfc3339())),
                        Line::from(format!("  Fast Fingerprint:   {}", entry.quick_fingerprint.as_deref().unwrap_or("none"))),
                        Line::from(format!("  Vault File Path:    {}", entry.vault_path)),
                        Line::from(""),
                        Line::from(Span::styled("  Press [Esc/Enter] to close", Style::default().fg(Color::LightCyan))),
                    ];

                    let inspect_widget = Paragraph::new(inspect_lines).block(modal_block);
                    f.render_widget(inspect_widget, popup_area);
                }
            }

            // 6. EXCLUSION MODAL
            if view_mode == ViewMode::ExclusionModal {
                let popup_area = centered_rect(65, 60, size);
                f.render_widget(Clear, popup_area);

                let modal_block = Block::default()
                    .title(" Active Exclusion Rules ")
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan));

                let mut lines = vec![
                    Line::from(""),
                    Line::from(Span::styled("  System Paths (Prefix match):", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))),
                ];
                for p in &config.exclusions.system_paths {
                    lines.push(Line::from(format!("    • {}", p)));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Path Patterns (Regex):", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))));
                for r in &config.exclusions.path_regex {
                    lines.push(Line::from(format!("    • {}", r)));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Filename Patterns (Regex):", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
                for f in &config.exclusions.filename_regex {
                    lines.push(Line::from(format!("    • {}", f)));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("  Press [Esc/q/e] to return to dashboard", Style::default().fg(Color::LightCyan))));

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
                        KeyCode::Char('j') | KeyCode::Down => {
                            if !entries.is_empty() {
                                let curr = table_state.selected().unwrap_or(0);
                                let next = (curr + 1).min(entries.len() - 1);
                                table_state.select(Some(next));
                                status_message = None;
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if !entries.is_empty() {
                                let curr = table_state.selected().unwrap_or(0);
                                let prev = curr.saturating_sub(1);
                                table_state.select(Some(prev));
                                status_message = None;
                            }
                        }
                        KeyCode::Enter => {
                            if !entries.is_empty() {
                                view_mode = ViewMode::ActionMenu;
                                action_index = 0;
                            }
                        }
                        KeyCode::Char('r') => {
                            // Quick restore
                            if let Some(idx) = table_state.selected() {
                                if let Some(entry) = entries.get(idx) {
                                    let restore_mgr = RestoreManager::new(db);
                                    match restore_mgr.restore_entry(entry, false, false) {
                                        Ok(_) => {
                                            status_message = Some(format!("Restored '{}'", entry.filename));
                                            entries = db.list_active(None)?;
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
                        KeyCode::Char('x') => {
                            // Quick purge
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
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('2') => {
                            action_index = 1;
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('3') => {
                            action_index = 2;
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('4') => {
                            action_index = 3;
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Char('5') => {
                            action_index = 4;
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
                        }
                        KeyCode::Enter => {
                            execute_action(action_index, &mut entries, &mut table_state, &mut view_mode, &mut status_message, db);
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
                    *status_message = Some(format!("Restored '{}'", entry.filename));
                    *entries = db.list_active(None).unwrap_or_default();
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
                    *status_message = Some(format!("Restored snapshot of '{}'", entry.filename));
                    *entries = db.list_active(None).unwrap_or_default();
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
