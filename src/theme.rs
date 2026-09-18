use ratatui::style::Color;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub bg: Option<Color>, // None = transparent terminal (kitty background), Some = solid theme background
    pub active_border: Color,
    pub inactive_border: Color,
    pub active_title: Color,
    pub inactive_title: Color,
    pub active_badge_fg: Color,
    pub active_badge_bg: Color,
    pub header_fg: Color,
    pub selected_row_fg: Color,
    pub selected_row_inactive_fg: Color,
    pub unselected_row_fg: Color,
    pub cursor_active: Color,
    pub cursor_inactive: Color,
    pub label_fg: Color,
    pub value_fg: Color,
    pub status_preserved: Color,
    pub status_restored: Color,
    pub status_purged: Color,
    pub status_excluded: Color,
    pub accent: Color,
    pub secondary: Color,
    pub warning: Color,
}

impl Theme {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        bg: Option<Color>,
        accent: Color,
        secondary: Color,
        text: Color,
        muted: Color,
        comment: Color,
        green: Color,
        red: Color,
        yellow: Color,
        warning: Color,
    ) -> Self {
        Self {
            id,
            name,
            bg,
            active_border: accent,
            inactive_border: muted,
            active_title: accent,
            inactive_title: comment,
            active_badge_fg: match bg {
                Some(c) => c,
                None => Color::Black,
            },
            active_badge_bg: accent,
            header_fg: secondary,
            selected_row_fg: accent,
            selected_row_inactive_fg: comment,
            unselected_row_fg: text,
            cursor_active: accent,
            cursor_inactive: muted,
            label_fg: secondary,
            value_fg: text,
            status_preserved: green,
            status_restored: green,
            status_purged: red,
            status_excluded: yellow,
            accent,
            secondary,
            warning,
        }
    }
}

pub const THEMES: &[Theme] = &[
    Theme {
        id: "default",
        name: "Default",
        bg: None, // Fully transparent, preserving user's terminal/kitty transparent background
        active_border: Color::LightCyan,
        inactive_border: Color::DarkGray,
        active_title: Color::LightCyan,
        inactive_title: Color::DarkGray,
        active_badge_fg: Color::Black,
        active_badge_bg: Color::LightCyan,
        header_fg: Color::LightCyan,
        selected_row_fg: Color::LightCyan,
        selected_row_inactive_fg: Color::DarkGray,
        unselected_row_fg: Color::White,
        cursor_active: Color::LightCyan,
        cursor_inactive: Color::DarkGray,
        label_fg: Color::LightCyan,
        value_fg: Color::White,
        status_preserved: Color::LightGreen,
        status_restored: Color::LightGreen,
        status_purged: Color::LightRed,
        status_excluded: Color::Yellow,
        accent: Color::LightCyan,
        secondary: Color::Cyan,
        warning: Color::LightYellow,
    },
    Theme::new(
        "catppuccin",
        "Catppuccin Mocha",
        Some(Color::Rgb(30, 30, 46)),
        Color::Rgb(203, 166, 247), // Mauve
        Color::Rgb(137, 220, 235), // Sky
        Color::Rgb(205, 214, 244), // Text
        Color::Rgb(88, 91, 112),   // Surface2
        Color::Rgb(147, 153, 178), // Overlay2
        Color::Rgb(166, 227, 161), // Green
        Color::Rgb(243, 139, 168), // Red
        Color::Rgb(249, 226, 175), // Yellow
        Color::Rgb(250, 179, 135), // Peach
    ),
    Theme::new(
        "solarized",
        "Solarized Dark",
        Some(Color::Rgb(0, 43, 54)),
        Color::Rgb(42, 161, 152),  // Cyan
        Color::Rgb(38, 139, 210),  // Blue
        Color::Rgb(238, 232, 213), // Base2
        Color::Rgb(88, 110, 117),  // Base01
        Color::Rgb(101, 123, 131), // Base00
        Color::Rgb(133, 153, 0),   // Green
        Color::Rgb(220, 50, 47),   // Red
        Color::Rgb(181, 137, 0),   // Yellow
        Color::Rgb(203, 75, 22),   // Orange
    ),
    Theme::new(
        "dracula",
        "Dracula",
        Some(Color::Rgb(40, 42, 54)),
        Color::Rgb(189, 147, 249), // Purple
        Color::Rgb(139, 233, 253), // Cyan
        Color::Rgb(248, 248, 242), // Foreground
        Color::Rgb(98, 114, 164),  // Comment
        Color::Rgb(98, 114, 164),
        Color::Rgb(80, 250, 123),  // Green
        Color::Rgb(255, 85, 85),   // Red
        Color::Rgb(241, 250, 140), // Yellow
        Color::Rgb(255, 184, 108), // Orange
    ),
    Theme::new(
        "gruvbox",
        "Gruvbox Dark",
        Some(Color::Rgb(40, 40, 40)),
        Color::Rgb(254, 128, 25),  // Bright Orange
        Color::Rgb(131, 165, 152), // Blue
        Color::Rgb(235, 219, 178), // Foreground
        Color::Rgb(102, 92, 84),   // Dark Gray
        Color::Rgb(146, 131, 116), // Gray
        Color::Rgb(184, 187, 38),  // Green
        Color::Rgb(251, 73, 52),   // Red
        Color::Rgb(250, 189, 47),  // Yellow
        Color::Rgb(250, 189, 47),
    ),
    Theme::new(
        "tokyo_night",
        "Tokyo Night",
        Some(Color::Rgb(26, 27, 38)),
        Color::Rgb(187, 154, 247), // Magenta
        Color::Rgb(125, 207, 255), // Cyan
        Color::Rgb(192, 202, 245), // Text
        Color::Rgb(65, 72, 104),   // Border
        Color::Rgb(86, 95, 137),   // Comment
        Color::Rgb(158, 206, 106), // Green
        Color::Rgb(247, 118, 142), // Red
        Color::Rgb(224, 175, 104), // Yellow
        Color::Rgb(224, 175, 104),
    ),
    Theme::new(
        "nord",
        "Nord",
        Some(Color::Rgb(46, 52, 64)),
        Color::Rgb(136, 192, 208), // Frost Cyan
        Color::Rgb(129, 161, 193), // Frost Blue
        Color::Rgb(236, 239, 244), // Snow White
        Color::Rgb(67, 76, 94),    // Nord2
        Color::Rgb(94, 129, 172),  // Nord3
        Color::Rgb(163, 190, 140), // Green
        Color::Rgb(191, 97, 106),  // Red
        Color::Rgb(235, 203, 139), // Yellow
        Color::Rgb(235, 203, 139),
    ),
    Theme::new(
        "rose_pine",
        "Rosé Pine",
        Some(Color::Rgb(25, 23, 36)),
        Color::Rgb(235, 111, 146), // Rose
        Color::Rgb(156, 207, 216), // Foam
        Color::Rgb(224, 222, 244), // Text
        Color::Rgb(110, 106, 134), // Muted
        Color::Rgb(144, 140, 170), // Subtle
        Color::Rgb(49, 116, 143),  // Pine
        Color::Rgb(235, 111, 146), // Love
        Color::Rgb(246, 193, 119), // Gold
        Color::Rgb(246, 193, 119),
    ),
    Theme::new(
        "one_dark",
        "One Dark",
        Some(Color::Rgb(40, 44, 52)),
        Color::Rgb(97, 175, 239),  // Blue
        Color::Rgb(198, 120, 221), // Purple
        Color::Rgb(171, 178, 191), // Text
        Color::Rgb(75, 82, 99),    // Muted
        Color::Rgb(92, 99, 112),   // Comment
        Color::Rgb(152, 195, 121), // Green
        Color::Rgb(224, 108, 117), // Red
        Color::Rgb(229, 192, 123), // Yellow
        Color::Rgb(209, 154, 102), // Orange
    ),
    Theme::new(
        "monokai",
        "Monokai Pro",
        Some(Color::Rgb(45, 42, 46)),
        Color::Rgb(255, 97, 136),  // Pink
        Color::Rgb(120, 220, 232), // Cyan
        Color::Rgb(252, 252, 250), // Text
        Color::Rgb(114, 112, 114), // Muted
        Color::Rgb(147, 146, 147), // Comment
        Color::Rgb(169, 220, 106), // Green
        Color::Rgb(255, 97, 136),  // Red
        Color::Rgb(255, 216, 102), // Yellow
        Color::Rgb(252, 152, 103), // Orange
    ),
    Theme::new(
        "kanagawa",
        "Kanagawa",
        Some(Color::Rgb(31, 31, 40)),
        Color::Rgb(155, 206, 180), // Wave Aqua
        Color::Rgb(126, 156, 216), // Crystal Blue
        Color::Rgb(220, 223, 228), // Fuji White
        Color::Rgb(84, 84, 109),   // Sumi Ink
        Color::Rgb(114, 113, 133), // Fuji Gray
        Color::Rgb(152, 187, 108), // Spring Green
        Color::Rgb(232, 134, 138), // Autumn Red
        Color::Rgb(230, 195, 132), // Carp Yellow
        Color::Rgb(255, 160, 102), // Surimi Orange
    ),
    Theme::new(
        "cyberpunk",
        "Cyberpunk Neon",
        Some(Color::Rgb(20, 16, 38)),
        Color::Rgb(255, 0, 127),   // Neon Pink
        Color::Rgb(0, 240, 255),   // Neon Cyan
        Color::Rgb(240, 240, 255), // Bright Text
        Color::Rgb(70, 50, 95),    // Muted
        Color::Rgb(115, 80, 155),  // Comment
        Color::Rgb(57, 255, 20),   // Neon Lime
        Color::Rgb(255, 49, 49),   // Neon Red
        Color::Rgb(255, 238, 0),   // Neon Yellow
        Color::Rgb(255, 140, 0),   // Neon Orange
    ),
];

#[allow(dead_code)]
pub fn get_theme(id_or_name: &str) -> &'static Theme {
    let lower = id_or_name.to_lowercase();
    for theme in THEMES {
        if theme.id == lower || theme.name.to_lowercase() == lower {
            return theme;
        }
    }
    &THEMES[0] // Default
}

pub fn theme_index(id_or_name: &str) -> usize {
    let lower = id_or_name.to_lowercase();
    for (i, theme) in THEMES.iter().enumerate() {
        if theme.id == lower || theme.name.to_lowercase() == lower {
            return i;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_themes_lookup() {
        assert_eq!(THEMES.len(), 12);
        for theme in THEMES {
            assert_eq!(get_theme(theme.id).id, theme.id);
            assert_eq!(get_theme(theme.name).id, theme.id);
        }
        assert_eq!(get_theme("unknown_theme").id, "default");
    }
}
