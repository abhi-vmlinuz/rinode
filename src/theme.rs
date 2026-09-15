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
    Theme {
        id: "catppuccin",
        name: "Catppuccin Mocha",
        bg: Some(Color::Rgb(30, 30, 46)), // Mocha Base (#1e1e2e)
        active_border: Color::Rgb(203, 166, 247), // Mauve
        inactive_border: Color::Rgb(88, 91, 112), // Surface2
        active_title: Color::Rgb(203, 166, 247),
        inactive_title: Color::Rgb(147, 153, 178), // Overlay2
        active_badge_fg: Color::Rgb(17, 17, 27),   // Crust
        active_badge_bg: Color::Rgb(203, 166, 247),
        header_fg: Color::Rgb(137, 180, 250),      // Blue
        selected_row_fg: Color::Rgb(203, 166, 247),
        selected_row_inactive_fg: Color::Rgb(147, 153, 178),
        unselected_row_fg: Color::Rgb(205, 214, 244), // Text
        cursor_active: Color::Rgb(203, 166, 247),
        cursor_inactive: Color::Rgb(108, 112, 134),
        label_fg: Color::Rgb(137, 220, 235),          // Sky
        value_fg: Color::Rgb(205, 214, 244),
        status_preserved: Color::Rgb(166, 227, 161),  // Green
        status_restored: Color::Rgb(166, 227, 161),
        status_purged: Color::Rgb(243, 139, 168),    // Red
        status_excluded: Color::Rgb(249, 226, 175),  // Yellow
        accent: Color::Rgb(203, 166, 247),
        secondary: Color::Rgb(137, 180, 250),
        warning: Color::Rgb(250, 179, 135),          // Peach
    },
    Theme {
        id: "solarized",
        name: "Solarized Dark",
        bg: Some(Color::Rgb(0, 43, 54)), // Base03 (#002b36)
        active_border: Color::Rgb(42, 161, 152),   // Cyan
        inactive_border: Color::Rgb(88, 110, 117), // Base01
        active_title: Color::Rgb(42, 161, 152),
        inactive_title: Color::Rgb(101, 123, 131), // Base00
        active_badge_fg: Color::Rgb(0, 43, 54),
        active_badge_bg: Color::Rgb(42, 161, 152),
        header_fg: Color::Rgb(38, 139, 210),       // Blue
        selected_row_fg: Color::Rgb(42, 161, 152),
        selected_row_inactive_fg: Color::Rgb(101, 123, 131),
        unselected_row_fg: Color::Rgb(147, 161, 161), // Base1
        cursor_active: Color::Rgb(42, 161, 152),
        cursor_inactive: Color::Rgb(88, 110, 117),
        label_fg: Color::Rgb(38, 139, 210),
        value_fg: Color::Rgb(238, 232, 213),       // Base2
        status_preserved: Color::Rgb(133, 153, 0), // Green
        status_restored: Color::Rgb(133, 153, 0),
        status_purged: Color::Rgb(220, 50, 47),    // Red
        status_excluded: Color::Rgb(181, 137, 0),  // Yellow
        accent: Color::Rgb(42, 161, 152),
        secondary: Color::Rgb(38, 139, 210),
        warning: Color::Rgb(203, 75, 22),          // Orange
    },
    Theme {
        id: "dracula",
        name: "Dracula",
        bg: Some(Color::Rgb(40, 42, 54)), // Background (#282a36)
        active_border: Color::Rgb(189, 147, 249), // Purple
        inactive_border: Color::Rgb(98, 114, 164), // Comment
        active_title: Color::Rgb(189, 147, 249),
        inactive_title: Color::Rgb(98, 114, 164),
        active_badge_fg: Color::Rgb(40, 42, 54),
        active_badge_bg: Color::Rgb(189, 147, 249),
        header_fg: Color::Rgb(139, 233, 253),     // Cyan
        selected_row_fg: Color::Rgb(189, 147, 249),
        selected_row_inactive_fg: Color::Rgb(98, 114, 164),
        unselected_row_fg: Color::Rgb(248, 248, 242),
        cursor_active: Color::Rgb(189, 147, 249),
        cursor_inactive: Color::Rgb(98, 114, 164),
        label_fg: Color::Rgb(139, 233, 253),
        value_fg: Color::Rgb(248, 248, 242),
        status_preserved: Color::Rgb(80, 250, 123),  // Green
        status_restored: Color::Rgb(80, 250, 123),
        status_purged: Color::Rgb(255, 85, 85),     // Red
        status_excluded: Color::Rgb(241, 250, 140), // Yellow
        accent: Color::Rgb(189, 147, 249),
        secondary: Color::Rgb(255, 121, 198), // Pink
        warning: Color::Rgb(255, 184, 108),   // Orange
    },
    Theme {
        id: "gruvbox",
        name: "Gruvbox Dark",
        bg: Some(Color::Rgb(40, 40, 40)), // Dark0 (#282828)
        active_border: Color::Rgb(254, 128, 25),   // Bright Orange
        inactive_border: Color::Rgb(102, 92, 84),  // Dark Gray
        active_title: Color::Rgb(254, 128, 25),
        inactive_title: Color::Rgb(146, 131, 116), // Gray
        active_badge_fg: Color::Rgb(40, 40, 40),
        active_badge_bg: Color::Rgb(254, 128, 25),
        header_fg: Color::Rgb(250, 189, 47),       // Yellow
        selected_row_fg: Color::Rgb(254, 128, 25),
        selected_row_inactive_fg: Color::Rgb(146, 131, 116),
        unselected_row_fg: Color::Rgb(235, 219, 178),
        cursor_active: Color::Rgb(254, 128, 25),
        cursor_inactive: Color::Rgb(102, 92, 84),
        label_fg: Color::Rgb(131, 165, 152),       // Blue
        value_fg: Color::Rgb(235, 219, 178),
        status_preserved: Color::Rgb(184, 187, 38),  // Green
        status_restored: Color::Rgb(184, 187, 38),
        status_purged: Color::Rgb(251, 73, 52),     // Red
        status_excluded: Color::Rgb(250, 189, 47),  // Yellow
        accent: Color::Rgb(254, 128, 25),
        secondary: Color::Rgb(142, 192, 124), // Aqua
        warning: Color::Rgb(250, 189, 47),
    },
    Theme {
        id: "tokyo_night",
        name: "Tokyo Night",
        bg: Some(Color::Rgb(26, 27, 38)), // Storm (#1a1b26)
        active_border: Color::Rgb(187, 154, 247), // Magenta
        inactive_border: Color::Rgb(65, 72, 104), // Border
        active_title: Color::Rgb(187, 154, 247),
        inactive_title: Color::Rgb(86, 95, 137),  // Comment
        active_badge_fg: Color::Rgb(26, 27, 38),
        active_badge_bg: Color::Rgb(187, 154, 247),
        header_fg: Color::Rgb(125, 207, 255),     // Cyan
        selected_row_fg: Color::Rgb(187, 154, 247),
        selected_row_inactive_fg: Color::Rgb(86, 95, 137),
        unselected_row_fg: Color::Rgb(192, 202, 245),
        cursor_active: Color::Rgb(187, 154, 247),
        cursor_inactive: Color::Rgb(65, 72, 104),
        label_fg: Color::Rgb(122, 162, 247),      // Blue
        value_fg: Color::Rgb(192, 202, 245),
        status_preserved: Color::Rgb(158, 206, 106), // Green
        status_restored: Color::Rgb(158, 206, 106),
        status_purged: Color::Rgb(247, 118, 142),    // Red
        status_excluded: Color::Rgb(224, 175, 104),  // Orange/Yellow
        accent: Color::Rgb(187, 154, 247),
        secondary: Color::Rgb(125, 207, 255),
        warning: Color::Rgb(224, 175, 104),
    },
    Theme {
        id: "nord",
        name: "Nord",
        bg: Some(Color::Rgb(46, 52, 64)), // Polar Night (#2e3440)
        active_border: Color::Rgb(136, 192, 208), // Frost Cyan
        inactive_border: Color::Rgb(67, 76, 94),  // Nord2
        active_title: Color::Rgb(136, 192, 208),
        inactive_title: Color::Rgb(129, 161, 193), // Frost Blue
        active_badge_fg: Color::Rgb(46, 52, 64),
        active_badge_bg: Color::Rgb(136, 192, 208),
        header_fg: Color::Rgb(143, 188, 187),      // Frost Teal
        selected_row_fg: Color::Rgb(136, 192, 208),
        selected_row_inactive_fg: Color::Rgb(94, 129, 172),
        unselected_row_fg: Color::Rgb(236, 239, 244),
        cursor_active: Color::Rgb(136, 192, 208),
        cursor_inactive: Color::Rgb(76, 86, 106),
        label_fg: Color::Rgb(129, 161, 193),
        value_fg: Color::Rgb(236, 239, 244),
        status_preserved: Color::Rgb(163, 190, 140), // Green
        status_restored: Color::Rgb(163, 190, 140),
        status_purged: Color::Rgb(191, 97, 106),     // Red
        status_excluded: Color::Rgb(235, 203, 139),  // Yellow
        accent: Color::Rgb(136, 192, 208),
        secondary: Color::Rgb(129, 161, 193),
        warning: Color::Rgb(235, 203, 139),
    },
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
