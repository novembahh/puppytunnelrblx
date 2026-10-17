

use eframe::egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Home,
    Play,
    Community,
    Profile,
    Tools,
    About,
    Host,
    Join,
    Servers,
    Feed,
    Messages,
    Guilds,
    Bell,
    Search,
    Shield,
    Gavel,
    Key,
    Echo,
    Cube,
    Refresh,
    Plus,
    Check,
    Close,
    Copy,
    Send,
    Trash,
    Heart,
    Repost,
    Flag,
    Lock,
    Update,
    External,
}

impl Icon {
    fn glyph(self) -> char {
        let cp = match self {
            Icon::Home => 0xe88a,
            Icon::Play => 0xea28,
            Icon::Community => 0xf233,
            Icon::Profile => 0xe7fd,
            Icon::Tools => 0xe429,
            Icon::About => 0xe88e,
            Icon::Host => 0xe875,
            Icon::Join => 0xea77,
            Icon::Servers => 0xe9b0,
            Icon::Feed => 0xea14,
            Icon::Messages => 0xe0ca,
            Icon::Guilds => 0xe9e0,
            Icon::Bell => 0xe7f4,
            Icon::Search => 0xe8b6,
            Icon::Shield => 0xe8e8,
            Icon::Gavel => 0xe90e,
            Icon::Key => 0xe0da,
            Icon::Echo => 0xe1b8,
            Icon::Cube => 0xe9fe,
            Icon::Refresh => 0xe5d5,
            Icon::Plus => 0xe145,
            Icon::Check => 0xe5ca,
            Icon::Close => 0xe5cd,
            Icon::Copy => 0xe14d,
            Icon::Send => 0xe163,
            Icon::Trash => 0xe872,
            Icon::Heart => 0xe87d,
            Icon::Repost => 0xe040,
            Icon::Flag => 0xe153,
            Icon::Lock => 0xe897,
            Icon::Update => 0xe62a,
            Icon::External => 0xe89e,
        };
        char::from_u32(cp).unwrap_or('?')
    }
}


pub fn paint(painter: &Painter, center: Pos2, size: f32, icon: Icon, color: Color32) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        icon.glyph(),
        FontId::new(size * 1.12, FontFamily::Name("icons".into())),
        color,
    );
}
