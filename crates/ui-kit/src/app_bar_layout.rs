//! Clean-room application bar spacing constants.

pub mod application {
    pub const CONTENT_PADDING: f32 = 8.0;
    pub const CARD_PADDING: f32 = 6.0;
    pub const GAP: f32 = 6.0;
    pub const PILL_MIN: f32 = 28.0;
    pub const PILL_ICON: f32 = 14.0;
    pub const VERSION_PADDING: f32 = 8.0;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Measurements {
        pub width: f32,
    }
}
