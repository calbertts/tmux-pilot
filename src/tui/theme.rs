use ratatui::style::Color;

/// Gruvbox Dark color palette (matching tmux config)
pub struct Gruvbox;

impl Gruvbox {
    // Background
    pub const BG: Color = Color::Rgb(40, 40, 40);        // #282828 (colour235)
    pub const BG_SOFT: Color = Color::Rgb(50, 48, 47);   // #32302f
    pub const BG_POPUP: Color = Color::Rgb(60, 56, 54);  // #3c3836 — popup/dialog bg

    // Foreground
    pub const FG: Color = Color::Rgb(168, 153, 132);     // #a89984 (colour246)
    pub const FG_BRIGHT: Color = Color::Rgb(235, 219, 178); // #ebdbb2

    // Accent colors
    pub const GREEN: Color = Color::Rgb(184, 187, 38);   // #b8bb26 (colour142)
    pub const ORANGE: Color = Color::Rgb(254, 128, 25);  // #fe8019 (colour208)
    pub const RED: Color = Color::Rgb(251, 73, 52);      // #fb4934
    pub const YELLOW: Color = Color::Rgb(250, 189, 47);  // #fabd2f
    pub const BLUE: Color = Color::Rgb(131, 165, 152);   // #83a598
    pub const PURPLE: Color = Color::Rgb(211, 134, 155); // #d3869b
    pub const AQUA: Color = Color::Rgb(142, 192, 124);   // #8ec07c

    // Grays
    pub const GRAY: Color = Color::Rgb(146, 131, 116);   // #928374
    pub const DARK_GRAY: Color = Color::Rgb(80, 73, 69); // #504945 (colour239)
}
