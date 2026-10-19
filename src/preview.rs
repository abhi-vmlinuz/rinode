use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::Command;

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::db::EntryRecord;
use crate::format_bytes;
use crate::theme::Theme;

pub struct PreviewContent {
    pub category: String,
    pub summary: String,
    pub lines: Vec<Line<'static>>,
}

fn detect_file_type(path: &Path) -> String {
    if let Ok(output) = Command::new("file").arg("-b").arg(path).output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "Unknown".to_string()
}

pub fn expand_tabs(s: &str, tab_size: usize) -> String {
    let mut result = String::with_capacity(s.len() + 16);
    let mut col = 0;
    for ch in s.chars() {
        if ch == '\t' {
            let spaces = tab_size - (col % tab_size);
            for _ in 0..spaces {
                result.push(' ');
            }
            col += spaces;
        } else if ch == '\r' {
            continue;
        } else {
            result.push(ch);
            col += 1;
        }
    }
    result
}

pub fn is_text_file(path: &Path) -> bool {
    if let Ok(mut f) = File::open(path) {
        let mut sample = [0u8; 8192];
        let n = f.read(&mut sample).unwrap_or(0);
        n == 0 || (!sample[..n].contains(&0) && std::str::from_utf8(&sample[..n]).is_ok())
    } else {
        false
    }
}

pub fn format_hex_dump(bytes: &[u8], offset_start: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (row_idx, chunk) in bytes.chunks(16).enumerate() {
        let offset = offset_start + row_idx * 16;
        let mut hex_spans = Vec::new();

        hex_spans.push(Span::styled(
            format!("{:08x}  ", offset),
            Style::default().fg(theme.label_fg),
        ));

        let mut hex_part = String::with_capacity(50);
        for i in 0..16 {
            if i == 8 {
                hex_part.push(' ');
            }
            if i < chunk.len() {
                hex_part.push_str(&format!("{:02x} ", chunk[i]));
            } else {
                hex_part.push_str("   ");
            }
        }
        hex_spans.push(Span::styled(
            hex_part,
            Style::default().fg(theme.value_fg),
        ));

        let mut ascii_part = String::with_capacity(18);
        ascii_part.push('|');
        for &b in chunk {
            if (0x20..=0x7E).contains(&b) {
                ascii_part.push(b as char);
            } else {
                ascii_part.push('.');
            }
        }
        ascii_part.push('|');
        hex_spans.push(Span::styled(
            ascii_part,
            Style::default().fg(theme.accent),
        ));

        lines.push(Line::from(hex_spans));
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn walk_dir_shallow(
    dir: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    total_files: &mut usize,
    total_dirs: &mut usize,
    total_bytes: &mut u64,
    tree_lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    max_lines: usize,
) {
    let read_entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return,
    };

    let mut entries = Vec::new();
    for entry in read_entries.flatten() {
        entries.push(entry);
    }
    entries.sort_by_key(|e| (e.file_type().map(|ft| !ft.is_dir()).unwrap_or(true), e.file_name()));

    let count = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let is_last = idx == count - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let name = entry.file_name().to_string_lossy().to_string();
        let meta = entry.metadata();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        if is_dir {
            *total_dirs += 1;
            if tree_lines.len() < max_lines {
                tree_lines.push(Line::from(vec![
                    Span::raw(format!("{}{}", prefix, connector)),
                    Span::styled(
                        format!("{}/", name),
                        Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD),
                    ),
                ]));
            }
            if depth < max_depth {
                let next_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                walk_dir_shallow(
                    &entry.path(),
                    &next_prefix,
                    depth + 1,
                    max_depth,
                    total_files,
                    total_dirs,
                    total_bytes,
                    tree_lines,
                    theme,
                    max_lines,
                );
            }
        } else {
            *total_files += 1;
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            *total_bytes += size;
            if tree_lines.len() < max_lines {
                tree_lines.push(Line::from(vec![
                    Span::raw(format!("{}{}", prefix, connector)),
                    Span::styled(name, Style::default().fg(theme.value_fg)),
                    Span::styled(format!(" ({})", format_bytes(size)), Style::default().fg(theme.label_fg)),
                ]));
            }
        }
    }
}

fn format_directory_preview(path: &Path, filename: &str, theme: &Theme) -> PreviewContent {
    let mut total_files = 0;
    let mut total_dirs = 0;
    let mut total_bytes = 0u64;
    let mut tree_lines = Vec::new();

    walk_dir_shallow(
        path,
        "  ",
        0,
        3,
        &mut total_files,
        &mut total_dirs,
        &mut total_bytes,
        &mut tree_lines,
        theme,
        60,
    );

    let summary = format!(
        "{} items ({} files, {} dirs) • {}",
        total_files + total_dirs,
        total_files,
        total_dirs,
        format_bytes(total_bytes)
    );

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Directory:    ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" ({})", format_bytes(total_bytes)), Style::default().fg(theme.value_fg)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Summary:      ", Style::default().fg(theme.label_fg)),
        Span::styled(
            format!("{} subdirectories, {} files", total_dirs, total_files),
            Style::default().fg(theme.value_fg),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Directory Tree Contents:",
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    lines.extend(tree_lines);

    PreviewContent {
        category: "Directory".into(),
        summary,
        lines,
    }
}

fn format_archive_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let is_zip = filename.ends_with(".zip");
    let mut entries_list = Vec::new();

    if is_zip {
        if let Ok(output) = Command::new("unzip").arg("-l").arg(path).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().skip(3) {
                    if line.starts_with(" -----") || line.is_empty() {
                        break;
                    }
                    if let Some(name) = line.split_whitespace().nth(3) {
                        entries_list.push(name.to_string());
                    }
                }
            }
        }
    } else if let Ok(output) = Command::new("tar").arg("-taf").arg(path).output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().take(60) {
                if !line.trim().is_empty() {
                    entries_list.push(line.trim().to_string());
                }
            }
        }
    }

    let summary = if !detected_type.is_empty() && detected_type != "Unknown" {
        detected_type.to_string()
    } else {
        "Compressed Archive".to_string()
    };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Archive:      ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Type:         ", Style::default().fg(theme.label_fg)),
        Span::styled(summary.clone(), Style::default().fg(theme.value_fg)),
    ]));
    lines.push(Line::from(""));

    if !entries_list.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("Archive Table of Contents (showing first {} entries):", entries_list.len()),
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )));
        for entry in &entries_list {
            let is_dir = entry.ends_with('/');
            let style = if is_dir {
                Style::default().fg(theme.secondary).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.value_fg)
            };
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(theme.label_fg)),
                Span::styled(entry.clone(), style),
            ]));
        }
        if entries_list.len() >= 60 {
            lines.push(Line::from(Span::styled(
                "  ... and additional entries in archive",
                Style::default().fg(theme.label_fg),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Unable to list archive contents with tar/unzip.",
            Style::default().fg(theme.warning),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press [o] to open with default system archive viewer.",
            Style::default().fg(theme.accent),
        )));
    }

    PreviewContent {
        category: "Archive".into(),
        summary,
        lines,
    }
}

fn format_media_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let mut streams_info = Vec::new();
    let mut format_info = Vec::new();

    if let Ok(output) = Command::new("ffprobe")
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-show_entries",
            "format=format_long_name,duration,bit_rate:stream=index,codec_type,codec_name,width,height,r_frame_rate,sample_rate,channels",
            "-of",
            "default=noprint_wrappers=1",
            path.to_str().unwrap_or(""),
        ])
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut current_stream_type = String::new();
            let mut current_stream_codec = String::new();
            let mut current_stream_desc = String::new();
            let mut stream_idx = 0;

            for line in stdout.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    match k {
                        "format_long_name" => format_info.push(("Container", v.to_string())),
                        "duration" => {
                            if let Ok(secs) = v.parse::<f64>() {
                                let total_s = secs as u64;
                                let h = total_s / 3600;
                                let m = (total_s % 3600) / 60;
                                let s = total_s % 60;
                                format_info.push(("Duration", format!("{:02}:{:02}:{:02}", h, m, s)));
                            }
                        }
                        "bit_rate" => {
                            if let Ok(br) = v.parse::<u64>() {
                                format_info.push(("Bitrate", format!("{:.1} kbps", br as f64 / 1000.0)));
                            }
                        }
                        "index" => {
                            if !current_stream_codec.is_empty() {
                                streams_info.push((
                                    stream_idx,
                                    current_stream_type.clone(),
                                    current_stream_codec.clone(),
                                    current_stream_desc.clone(),
                                ));
                                current_stream_desc.clear();
                            }
                            stream_idx = v.parse::<usize>().unwrap_or(0);
                        }
                        "codec_type" => current_stream_type = v.to_string(),
                        "codec_name" => current_stream_codec = v.to_string(),
                        "width" => {
                            current_stream_desc = format!("{}x", v);
                        }
                        "height" => {
                            current_stream_desc.push_str(v);
                        }
                        "r_frame_rate" => {
                            if let Some((num, den)) = v.split_once('/') {
                                if let (Ok(n), Ok(d)) = (num.parse::<f64>(), den.parse::<f64>()) {
                                    if d > 0.0 {
                                        current_stream_desc.push_str(&format!(" @ {:.1} fps", n / d));
                                    }
                                }
                            }
                        }
                        "sample_rate" => {
                            current_stream_desc = format!("{} Hz", v);
                        }
                        "channels" => {
                            current_stream_desc.push_str(&format!(", {} ch", v));
                        }
                        _ => {}
                    }
                }
            }
            if !current_stream_codec.is_empty() {
                streams_info.push((stream_idx, current_stream_type, current_stream_codec, current_stream_desc));
            }
        }
    }

    let is_video = streams_info.iter().any(|(_, t, _, _)| t == "video");
    let category = if is_video { "Video" } else { "Audio" };

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("File:         ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Detected:     ", Style::default().fg(theme.label_fg)),
        Span::styled(detected_type.to_string(), Style::default().fg(theme.value_fg)),
    ]));

    for (k, v) in format_info {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<14}", format!("{}:", k)), Style::default().fg(theme.label_fg)),
            Span::styled(v, Style::default().fg(theme.value_fg)),
        ]));
    }

    if !streams_info.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Media Streams:",
            Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
        )));
        for (idx, stype, codec, desc) in streams_info {
            let type_color = if stype == "video" { theme.secondary } else { theme.status_restored };
            let extra = if desc.is_empty() { String::new() } else { format!(" ({})", desc) };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  Stream #{} [{}]: ", idx, stype),
                    Style::default().fg(type_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(codec, Style::default().fg(theme.value_fg)),
                Span::styled(extra, Style::default().fg(theme.label_fg)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Press [o] to open and play in default media player.",
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));

    PreviewContent {
        category: category.into(),
        summary: detected_type.to_string(),
        lines,
    }
}

fn format_image_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Image File:   ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Format:       ", Style::default().fg(theme.label_fg)),
        Span::styled(detected_type.to_string(), Style::default().fg(theme.value_fg)),
    ]));

    if let Ok(meta) = path.metadata() {
        lines.push(Line::from(vec![
            Span::styled("File Size:    ", Style::default().fg(theme.label_fg)),
            Span::styled(format_bytes(meta.len()), Style::default().fg(theme.value_fg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Image Preview Info:",
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "  Terminal raster preview requires external viewer.",
        Style::default().fg(theme.label_fg),
    )));
    lines.push(Line::from(vec![
        Span::styled("  Press ", Style::default().fg(theme.value_fg)),
        Span::styled("[o]", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" to open image in system viewer.", Style::default().fg(theme.value_fg)),
    ]));

    PreviewContent {
        category: "Image".into(),
        summary: detected_type.to_string(),
        lines,
    }
}

fn format_document_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Document:     ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Detected:     ", Style::default().fg(theme.label_fg)),
        Span::styled(detected_type.to_string(), Style::default().fg(theme.value_fg)),
    ]));

    if let Ok(meta) = path.metadata() {
        lines.push(Line::from(vec![
            Span::styled("File Size:    ", Style::default().fg(theme.label_fg)),
            Span::styled(format_bytes(meta.len()), Style::default().fg(theme.value_fg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Document Actions:",
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::styled("  Press ", Style::default().fg(theme.value_fg)),
        Span::styled("[o]", Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
        Span::styled(" to open document in system reader.", Style::default().fg(theme.value_fg)),
    ]));

    PreviewContent {
        category: "Document".into(),
        summary: detected_type.to_string(),
        lines,
    }
}

fn format_text_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let mut lines = Vec::new();
    let content = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return PreviewContent {
                category: "Error".into(),
                summary: format!("Read error: {}", e),
                lines: vec![Line::from(Span::styled(
                    format!("Failed to read file: {}", e),
                    Style::default().fg(theme.warning),
                ))],
            };
        }
    };

    let text = String::from_utf8_lossy(&content);
    let all_lines: Vec<String> = text.lines().map(|s| expand_tabs(s, 4)).collect();
    let total_lines = all_lines.len();
    let max_display_lines = 500;
    let line_no_width = total_lines.min(max_display_lines).to_string().len().max(3);

    lines.push(Line::from(vec![
        Span::styled("File:         ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!(" ({} lines, {})", total_lines, format_bytes(content.len() as u64)),
            Style::default().fg(theme.label_fg),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Type:         ", Style::default().fg(theme.label_fg)),
        Span::styled(detected_type.to_string(), Style::default().fg(theme.value_fg)),
    ]));
    lines.push(Line::from(""));

    for (i, line_str) in all_lines.iter().take(max_display_lines).enumerate() {
        let line_num = i + 1;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:>width$} │ ", line_num, width = line_no_width),
                Style::default().fg(theme.label_fg),
            ),
            Span::styled(line_str.clone(), Style::default().fg(theme.value_fg)),
        ]));
    }

    if total_lines > max_display_lines {
        lines.push(Line::from(Span::styled(
            format!("... and {} more lines truncated", total_lines - max_display_lines),
            Style::default().fg(theme.label_fg),
        )));
    }

    PreviewContent {
        category: "Text".into(),
        summary: format!("{} lines", total_lines),
        lines,
    }
}

fn format_binary_preview(path: &Path, filename: &str, detected_type: &str, theme: &Theme) -> PreviewContent {
    let mut lines = Vec::new();
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            return PreviewContent {
                category: "Error".into(),
                summary: format!("Open error: {}", e),
                lines: vec![Line::from(Span::styled(
                    format!("Failed to open file: {}", e),
                    Style::default().fg(theme.warning),
                ))],
            };
        }
    };

    let mut buffer = [0u8; 1024];
    let bytes_read = file.read(&mut buffer).unwrap_or(0);
    let total_size = path.metadata().map(|m| m.len()).unwrap_or(bytes_read as u64);

    lines.push(Line::from(vec![
        Span::styled("Binary File:  ", Style::default().fg(theme.label_fg)),
        Span::styled(filename.to_string(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" ({})", format_bytes(total_size)), Style::default().fg(theme.value_fg)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("Detected:     ", Style::default().fg(theme.label_fg)),
        Span::styled(detected_type.to_string(), Style::default().fg(theme.value_fg)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("Hex Dump (first {} bytes):", bytes_read),
        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
    )));

    lines.extend(format_hex_dump(&buffer[..bytes_read], 0, theme));

    if total_size > bytes_read as u64 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("... and {} remaining bytes in binary file", total_size - bytes_read as u64),
            Style::default().fg(theme.label_fg),
        )));
    }

    PreviewContent {
        category: "Binary".into(),
        summary: detected_type.to_string(),
        lines,
    }
}

pub fn generate_preview(entry: &EntryRecord, theme: &Theme) -> PreviewContent {
    let path = Path::new(&entry.vault_path);
    if entry.vault_path.is_empty() || !path.exists() {
        return PreviewContent {
            category: "Missing".into(),
            summary: "Vault copy not found on disk".into(),
            lines: vec![
                Line::from(Span::styled("Storage file not found on disk.", Style::default().fg(theme.warning))),
                Line::from(Span::styled(format!("Path: {}", entry.vault_path), Style::default().fg(theme.label_fg))),
                Line::from(Span::styled(format!("Status: {}", entry.status), Style::default().fg(theme.label_fg))),
            ],
        };
    }

    if let Some(target) = &entry.symlink_target {
        let target_path = Path::new(target);
        let exists = target_path.exists();
        let lines = vec![
            Line::from(vec![
                Span::styled("Symlink:      ", Style::default().fg(theme.label_fg)),
                Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("Target:       ", Style::default().fg(theme.label_fg)),
                Span::styled(target.clone(), Style::default().fg(theme.value_fg)),
            ]),
            Line::from(vec![
                Span::styled("Destination:  ", Style::default().fg(theme.label_fg)),
                Span::styled(
                    if exists { "Exists on disk" } else { "Broken (target does not exist)" },
                    Style::default().fg(if exists { theme.status_restored } else { theme.warning }).add_modifier(Modifier::BOLD),
                ),
            ]),
        ];
        return PreviewContent {
            category: "Symlink".into(),
            summary: format!("-> {}", target),
            lines,
        };
    }

    if entry.is_directory {
        return format_directory_preview(path, &entry.filename, theme);
    }

    let detected_type = detect_file_type(path);
    let lower_name = entry.filename.to_lowercase();
    let lower_type = detected_type.to_lowercase();

    if lower_name.ends_with(".tar.gz")
        || lower_name.ends_with(".tgz")
        || lower_name.ends_with(".tar.bz2")
        || lower_name.ends_with(".tbz2")
        || lower_name.ends_with(".tar.xz")
        || lower_name.ends_with(".txz")
        || lower_name.ends_with(".tar.zst")
        || lower_name.ends_with(".tar")
        || lower_name.ends_with(".zip")
        || lower_name.ends_with(".7z")
        || lower_type.contains("tar archive")
        || lower_type.contains("zip archive")
        || lower_type.contains("gzip compressed")
        || lower_type.contains("xz compressed")
        || lower_type.contains("bzip2 compressed")
        || lower_type.contains("zstandard compressed")
    {
        return format_archive_preview(path, &entry.filename, &detected_type, theme);
    }

    if lower_name.ends_with(".mp4")
        || lower_name.ends_with(".mkv")
        || lower_name.ends_with(".avi")
        || lower_name.ends_with(".mov")
        || lower_name.ends_with(".webm")
        || lower_name.ends_with(".flv")
        || lower_name.ends_with(".wmv")
        || lower_name.ends_with(".mp3")
        || lower_name.ends_with(".wav")
        || lower_name.ends_with(".flac")
        || lower_name.ends_with(".ogg")
        || lower_name.ends_with(".m4a")
        || lower_name.ends_with(".aac")
        || lower_name.ends_with(".opus")
        || lower_type.contains("audio")
        || lower_type.contains("video")
        || lower_type.contains("media")
        || lower_type.contains("matroska")
        || (lower_type.contains("riff") && lower_type.contains("audio"))
    {
        return format_media_preview(path, &entry.filename, &detected_type, theme);
    }

    if lower_name.ends_with(".png")
        || lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || lower_name.ends_with(".webp")
        || lower_name.ends_with(".gif")
        || lower_name.ends_with(".bmp")
        || lower_name.ends_with(".svg")
        || lower_name.ends_with(".ico")
        || lower_type.contains("image")
    {
        return format_image_preview(path, &entry.filename, &detected_type, theme);
    }

    if lower_name.ends_with(".pdf") || lower_type.contains("pdf") {
        return format_document_preview(path, &entry.filename, &detected_type, theme);
    }

    let is_text = is_text_file(path);

    if is_text || lower_type.contains("text") || lower_type.contains("json") || lower_type.contains("xml") {
        format_text_preview(path, &entry.filename, &detected_type, theme)
    } else {
        format_binary_preview(path, &entry.filename, &detected_type, theme)
    }
}

pub fn generate_diff(entry: &EntryRecord, theme: &Theme) -> Result<Vec<Line<'static>>, String> {
    let orig = Path::new(&entry.original_path);
    let vault = Path::new(&entry.vault_path);

    if !vault.exists() {
        return Err(format!("Storage copy not found at '{}'.", entry.vault_path));
    }

    if !orig.exists() {
        if entry.is_directory || vault.is_dir() {
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("Directory Diff: ", Style::default().fg(theme.label_fg)),
                    Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("(-) Storage: ", Style::default().fg(theme.status_purged)),
                    Span::styled(entry.vault_path.clone(), Style::default().fg(theme.value_fg)),
                ]),
                Line::from(vec![
                    Span::styled("(+) Current: ", Style::default().fg(theme.status_restored)),
                    Span::styled(format!("{} (not present on disk)", entry.original_path), Style::default().fg(theme.label_fg)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Destination is currently vacant on disk. Restoring will recreate this directory and its contents cleanly.",
                    Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            let dir_preview = format_directory_preview(vault, &entry.filename, theme);
            lines.extend(dir_preview.lines);
            return Ok(lines);
        } else if let Some(target) = &entry.symlink_target {
            let lines = vec![
                Line::from(vec![
                    Span::styled("Symlink Diff: ", Style::default().fg(theme.label_fg)),
                    Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("(-) Storage: ", Style::default().fg(theme.status_purged)),
                    Span::styled(format!("Symlink -> {}", target), Style::default().fg(theme.value_fg)),
                ]),
                Line::from(vec![
                    Span::styled("(+) Current: ", Style::default().fg(theme.status_restored)),
                    Span::styled(format!("{} (not present on disk)", entry.original_path), Style::default().fg(theme.label_fg)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Destination is currently vacant on disk. Restoring will recreate this symlink cleanly.",
                    Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD),
                )),
            ];
            return Ok(lines);
        } else {
            let is_text = is_text_file(vault);
            let mut lines = vec![
                Line::from(vec![
                    Span::styled("File Diff: ", Style::default().fg(theme.label_fg)),
                    Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(vec![
                    Span::styled("(-) Storage: ", Style::default().fg(theme.status_purged)),
                    Span::styled(format!("{} ({})", entry.vault_path, format_bytes(entry.file_size)), Style::default().fg(theme.value_fg)),
                ]),
                Line::from(vec![
                    Span::styled("(+) Current: ", Style::default().fg(theme.status_restored)),
                    Span::styled(format!("{} (not present on disk)", entry.original_path), Style::default().fg(theme.label_fg)),
                ]),
                Line::from(""),
                Line::from(Span::styled(
                    "Destination is currently vacant on disk. Restoring will recreate this file cleanly.",
                    Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];

            if is_text {
                lines.push(Line::from(Span::styled(
                    "@@ Content to be restored: @@",
                    Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                )));
                if let Ok(bytes) = std::fs::read(vault) {
                    let text = String::from_utf8_lossy(&bytes);
                    for line in text.lines().take(500) {
                        let expanded = expand_tabs(line, 4);
                        lines.push(Line::from(Span::styled(
                            format!("+ {}", expanded),
                            Style::default().fg(theme.status_restored),
                        )));
                    }
                }
            } else {
                lines.push(Line::from(Span::styled(
                    "Binary file will be recreated on restore.",
                    Style::default().fg(theme.accent),
                )));
            }
            return Ok(lines);
        }
    }

    if entry.is_directory || orig.is_dir() {
        let output = Command::new("diff")
            .arg("-rq")
            .arg(vault)
            .arg(orig)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("Directory Diff: ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("Storage:  ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.vault_path.clone(), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("Current:  ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.original_path.clone(), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(""),
                ];

                if stdout.trim().is_empty() {
                    lines.push(Line::from(Span::styled(
                        "Directories are identical.",
                        Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD),
                    )));
                } else {
                    lines.push(Line::from(Span::styled(
                        "Directory differences:",
                        Style::default().fg(theme.accent).add_modifier(Modifier::BOLD),
                    )));
                    for line in stdout.lines() {
                        let expanded = expand_tabs(line, 4);
                        lines.push(Line::from(Span::styled(expanded, Style::default().fg(theme.warning))));
                    }
                }
                Ok(lines)
            }
            Err(e) => Err(format!("Failed to run diff: {}", e)),
        }
    } else {
        let output = Command::new("diff")
            .arg("-u")
            .arg(vault)
            .arg(orig)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled("File Diff: ", Style::default().fg(theme.label_fg)),
                        Span::styled(entry.filename.clone(), Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD)),
                    ]),
                    Line::from(vec![
                        Span::styled("(-) Storage: ", Style::default().fg(theme.status_purged)),
                        Span::styled(entry.vault_path.clone(), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(vec![
                        Span::styled("(+) Current: ", Style::default().fg(theme.status_restored)),
                        Span::styled(entry.original_path.clone(), Style::default().fg(theme.value_fg)),
                    ]),
                    Line::from(""),
                ];

                if out.status.success() {
                    lines.push(Line::from(Span::styled(
                        "Files are identical (content on disk matches the deleted storage copy).",
                        Style::default().fg(theme.status_restored).add_modifier(Modifier::BOLD),
                    )));
                } else if stdout.contains("Binary files") {
                    lines.push(Line::from(Span::styled(
                        "Binary files differ (cannot display unified text diff).",
                        Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
                    )));
                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("Storage size: ", Style::default().fg(theme.label_fg)),
                        Span::styled(format_bytes(entry.file_size), Style::default().fg(theme.value_fg)),
                    ]));
                    if let Ok(m) = orig.metadata() {
                        lines.push(Line::from(vec![
                            Span::styled("Current size: ", Style::default().fg(theme.label_fg)),
                            Span::styled(format_bytes(m.len()), Style::default().fg(theme.value_fg)),
                        ]));
                    }
                } else {
                    for line in stdout.lines() {
                        let expanded = expand_tabs(line, 4);
                        let styled_line = if expanded.starts_with("---") || expanded.starts_with("+++") {
                            Span::styled(expanded, Style::default().fg(theme.active_title).add_modifier(Modifier::BOLD))
                        } else if expanded.starts_with("@@") {
                            Span::styled(expanded, Style::default().fg(theme.accent).add_modifier(Modifier::BOLD))
                        } else if expanded.starts_with('+') {
                            Span::styled(expanded, Style::default().fg(theme.status_restored))
                        } else if expanded.starts_with('-') {
                            Span::styled(expanded, Style::default().fg(theme.status_purged))
                        } else {
                            Span::styled(expanded, Style::default().fg(theme.value_fg))
                        };
                        lines.push(Line::from(styled_line));
                    }
                }
                Ok(lines)
            }
            Err(e) => Err(format!("Failed to run diff: {}", e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::THEMES;
    use std::fs;

    fn sample_entry(path: &Path, is_dir: bool) -> EntryRecord {
        EntryRecord {
            id: 1,
            dev_major: 0,
            dev_minor: 0,
            mnt_id: 0,
            inode_no: 12345,
            original_path: path.to_string_lossy().to_string(),
            filename: path.file_name().unwrap().to_string_lossy().to_string(),
            file_size: if is_dir { 0 } else { fs::metadata(path).map(|m| m.len()).unwrap_or(0) },
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            quick_fingerprint: None,
            vault_path: path.to_string_lossy().to_string(),
            deleted_at: chrono::Utc::now(),
            status: "PRESERVED".into(),
            is_directory: is_dir,
            symlink_target: None,
            link_type: "RENAME".into(),
            restored_at: None,
            purged_at: None,
        }
    }

    #[test]
    fn test_hex_dump_formatting() {
        let theme = &THEMES[0];
        let bytes = b"Hello, World!\n1234";
        let lines = format_hex_dump(bytes, 0, theme);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_text_preview() {
        let theme = &THEMES[0];
        let dir = std::env::temp_dir().join(format!("rinode_test_txt_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.json");
        fs::write(&file, "{\n  \"hello\": \"world\"\n}\n").unwrap();

        let entry = sample_entry(&file, false);
        let preview = generate_preview(&entry, theme);

        assert_eq!(preview.category, "Text");
        assert!(!preview.lines.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_binary_preview() {
        let theme = &THEMES[0];
        let dir = std::env::temp_dir().join(format!("rinode_test_bin_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.bin");
        fs::write(&file, [0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01, 0x02, 0x00]).unwrap();

        let entry = sample_entry(&file, false);
        let preview = generate_preview(&entry, theme);

        assert_eq!(preview.category, "Binary");
        assert!(!preview.lines.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_directory_preview() {
        let theme = &THEMES[0];
        let dir = std::env::temp_dir().join(format!("rinode_test_dir_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        fs::write(dir.join("a.txt"), "hello").unwrap();
        fs::write(dir.join("b.txt"), "world").unwrap();

        let entry = sample_entry(&dir, true);
        let preview = generate_preview(&entry, theme);

        assert_eq!(preview.category, "Directory");
        assert!(preview.lines.len() >= 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_diff_identical_and_differing() {
        let theme = &THEMES[0];
        let dir = std::env::temp_dir().join(format!("rinode_test_diff_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file1 = dir.join("vault.txt");
        let file2 = dir.join("orig.txt");

        fs::write(&file1, "line 1\nline 2\n").unwrap();
        fs::write(&file2, "line 1\nline 2\n").unwrap();

        let mut entry = sample_entry(&file1, false);
        entry.original_path = file2.to_string_lossy().to_string();

        let diff_res = generate_diff(&entry, theme).unwrap();
        assert!(!diff_res.is_empty());

        // Modify file2 to create difference
        fs::write(&file2, "line 1\nline 2 modified\n").unwrap();
        let diff_res2 = generate_diff(&entry, theme).unwrap();
        assert!(!diff_res2.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_expand_tabs() {
        assert_eq!(expand_tabs("\tfoo", 4), "    foo");
        assert_eq!(expand_tabs("a\tb", 4), "a   b");
        assert_eq!(expand_tabs("ab\tc", 4), "ab  c");
        assert_eq!(expand_tabs("abc\td", 4), "abc d");
        assert_eq!(expand_tabs("abcd\te", 4), "abcd    e");
        assert_eq!(expand_tabs("no_tabs", 4), "no_tabs");
        assert_eq!(expand_tabs("with\r\nreturn", 4), "with\nreturn");
    }

    #[test]
    fn test_diff_vacant_original_path() {
        let theme = &THEMES[0];
        let dir = std::env::temp_dir().join(format!("rinode_test_vacant_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let vault_file = dir.join("stored.txt");
        let orig_file = dir.join("non_existent_orig.txt");

        fs::write(&vault_file, "hello\nworld\n").unwrap();

        let mut entry = sample_entry(&vault_file, false);
        entry.original_path = orig_file.to_string_lossy().to_string();

        let diff_lines = generate_diff(&entry, theme).expect("diff must succeed for vacant destination");
        assert!(!diff_lines.is_empty());
        let joined: String = diff_lines.iter().map(|l| l.to_string()).collect();
        assert!(joined.contains("vacant"));

        let _ = fs::remove_dir_all(&dir);
    }
}
