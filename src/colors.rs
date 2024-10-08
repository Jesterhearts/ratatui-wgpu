pub(crate) type Rgb = [u8; 3];

/// <https://www.w3.org/TR/SVG11/types.html#ColorKeywords>
pub(crate) mod named {
    use crate::colors::Rgb;

    pub(crate) const BLACK: Rgb = [0, 0, 0];
    pub(crate) const GREEN: Rgb = [0, 128, 0];
    pub(crate) const RED: Rgb = [255, 0, 0];
    pub(crate) const BLUE: Rgb = [0, 0, 255];
    pub(crate) const CYAN: Rgb = [0, 255, 255];
    pub(crate) const DARKGRAY: Rgb = [169, 169, 169];
    pub(crate) const GRAY: Rgb = [128, 128, 128];
    pub(crate) const LIGHTBLUE: Rgb = [173, 216, 230];
    pub(crate) const LIGHTCYAN: Rgb = [224, 255, 255];
    pub(crate) const LIGHTGREEN: Rgb = [144, 238, 144];
    pub(crate) const LIGHTMAGENTA: Rgb = [255, 128, 255];
    pub(crate) const LIGHTRED: Rgb = [240, 128, 128];
    pub(crate) const LIGHTYELLOW: Rgb = [255, 255, 224];
    pub(crate) const MAGENTA: Rgb = [255, 0, 255];
    pub(crate) const WHITE: Rgb = [255, 255, 255];
    pub(crate) const YELLOW: Rgb = [255, 255, 0];
}

/// This could be split into `[standard table]` + `[high intensity table]` +
/// `<6x6x6 cube fn>` + `<grayscale step fn>`, but a lookup table is only 768
/// bytes and way simpler to implement.
pub(crate) const ANSI_TO_RGB: [Rgb; 256] = [
    [0x00, 0x00, 0x00],
    [0x80, 0x00, 0x00],
    [0x00, 0x80, 0x00],
    [0x80, 0x80, 0x00],
    [0x00, 0x00, 0x80],
    [0x80, 0x00, 0x80],
    [0x00, 0x80, 0x80],
    [0xc0, 0xc0, 0xc0],
    [0x80, 0x80, 0x80],
    [0xff, 0x00, 0x00],
    [0x00, 0xff, 0x00],
    [0xff, 0xff, 0x00],
    [0x00, 0x00, 0xff],
    [0xff, 0x00, 0xff],
    [0x00, 0xff, 0xff],
    [0xff, 0xff, 0xff],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x5f],
    [0x00, 0x00, 0x87],
    [0x00, 0x00, 0xaf],
    [0x00, 0x00, 0xd7],
    [0x00, 0x00, 0xff],
    [0x00, 0x5f, 0x00],
    [0x00, 0x5f, 0x5f],
    [0x00, 0x5f, 0x87],
    [0x00, 0x5f, 0xaf],
    [0x00, 0x5f, 0xd7],
    [0x00, 0x5f, 0xff],
    [0x00, 0x87, 0x00],
    [0x00, 0x87, 0x5f],
    [0x00, 0x87, 0x87],
    [0x00, 0x87, 0xaf],
    [0x00, 0x87, 0xd7],
    [0x00, 0x87, 0xff],
    [0x00, 0xaf, 0x00],
    [0x00, 0xaf, 0x5f],
    [0x00, 0xaf, 0x87],
    [0x00, 0xaf, 0xaf],
    [0x00, 0xaf, 0xd7],
    [0x00, 0xaf, 0xff],
    [0x00, 0xd7, 0x00],
    [0x00, 0xd7, 0x5f],
    [0x00, 0xd7, 0x87],
    [0x00, 0xd7, 0xaf],
    [0x00, 0xd7, 0xd7],
    [0x00, 0xd7, 0xff],
    [0x00, 0xff, 0x00],
    [0x00, 0xff, 0x5f],
    [0x00, 0xff, 0x87],
    [0x00, 0xff, 0xaf],
    [0x00, 0xff, 0xd7],
    [0x00, 0xff, 0xff],
    [0x5f, 0x00, 0x00],
    [0x5f, 0x00, 0x5f],
    [0x5f, 0x00, 0x87],
    [0x5f, 0x00, 0xaf],
    [0x5f, 0x00, 0xd7],
    [0x5f, 0x00, 0xff],
    [0x5f, 0x5f, 0x00],
    [0x5f, 0x5f, 0x5f],
    [0x5f, 0x5f, 0x87],
    [0x5f, 0x5f, 0xaf],
    [0x5f, 0x5f, 0xd7],
    [0x5f, 0x5f, 0xff],
    [0x5f, 0x87, 0x00],
    [0x5f, 0x87, 0x5f],
    [0x5f, 0x87, 0x87],
    [0x5f, 0x87, 0xaf],
    [0x5f, 0x87, 0xd7],
    [0x5f, 0x87, 0xff],
    [0x5f, 0xaf, 0x00],
    [0x5f, 0xaf, 0x5f],
    [0x5f, 0xaf, 0x87],
    [0x5f, 0xaf, 0xaf],
    [0x5f, 0xaf, 0xd7],
    [0x5f, 0xaf, 0xff],
    [0x5f, 0xd7, 0x00],
    [0x5f, 0xd7, 0x5f],
    [0x5f, 0xd7, 0x87],
    [0x5f, 0xd7, 0xaf],
    [0x5f, 0xd7, 0xd7],
    [0x5f, 0xd7, 0xff],
    [0x5f, 0xff, 0x00],
    [0x5f, 0xff, 0x5f],
    [0x5f, 0xff, 0x87],
    [0x5f, 0xff, 0xaf],
    [0x5f, 0xff, 0xd7],
    [0x5f, 0xff, 0xff],
    [0x87, 0x00, 0x00],
    [0x87, 0x00, 0x5f],
    [0x87, 0x00, 0x87],
    [0x87, 0x00, 0xaf],
    [0x87, 0x00, 0xd7],
    [0x87, 0x00, 0xff],
    [0x87, 0x5f, 0x00],
    [0x87, 0x5f, 0x5f],
    [0x87, 0x5f, 0x87],
    [0x87, 0x5f, 0xaf],
    [0x87, 0x5f, 0xd7],
    [0x87, 0x5f, 0xff],
    [0x87, 0x87, 0x00],
    [0x87, 0x87, 0x5f],
    [0x87, 0x87, 0x87],
    [0x87, 0x87, 0xaf],
    [0x87, 0x87, 0xd7],
    [0x87, 0x87, 0xff],
    [0x87, 0xaf, 0x00],
    [0x87, 0xaf, 0x5f],
    [0x87, 0xaf, 0x87],
    [0x87, 0xaf, 0xaf],
    [0x87, 0xaf, 0xd7],
    [0x87, 0xaf, 0xff],
    [0x87, 0xd7, 0x00],
    [0x87, 0xd7, 0x5f],
    [0x87, 0xd7, 0x87],
    [0x87, 0xd7, 0xaf],
    [0x87, 0xd7, 0xd7],
    [0x87, 0xd7, 0xff],
    [0x87, 0xff, 0x00],
    [0x87, 0xff, 0x5f],
    [0x87, 0xff, 0x87],
    [0x87, 0xff, 0xaf],
    [0x87, 0xff, 0xd7],
    [0x87, 0xff, 0xff],
    [0xaf, 0x00, 0x00],
    [0xaf, 0x00, 0x5f],
    [0xaf, 0x00, 0x87],
    [0xaf, 0x00, 0xaf],
    [0xaf, 0x00, 0xd7],
    [0xaf, 0x00, 0xff],
    [0xaf, 0x5f, 0x00],
    [0xaf, 0x5f, 0x5f],
    [0xaf, 0x5f, 0x87],
    [0xaf, 0x5f, 0xaf],
    [0xaf, 0x5f, 0xd7],
    [0xaf, 0x5f, 0xff],
    [0xaf, 0x87, 0x00],
    [0xaf, 0x87, 0x5f],
    [0xaf, 0x87, 0x87],
    [0xaf, 0x87, 0xaf],
    [0xaf, 0x87, 0xd7],
    [0xaf, 0x87, 0xff],
    [0xaf, 0xaf, 0x00],
    [0xaf, 0xaf, 0x5f],
    [0xaf, 0xaf, 0x87],
    [0xaf, 0xaf, 0xaf],
    [0xaf, 0xaf, 0xd7],
    [0xaf, 0xaf, 0xff],
    [0xaf, 0xd7, 0x00],
    [0xaf, 0xd7, 0x5f],
    [0xaf, 0xd7, 0x87],
    [0xaf, 0xd7, 0xaf],
    [0xaf, 0xd7, 0xd7],
    [0xaf, 0xd7, 0xff],
    [0xaf, 0xff, 0x00],
    [0xaf, 0xff, 0x5f],
    [0xaf, 0xff, 0x87],
    [0xaf, 0xff, 0xaf],
    [0xaf, 0xff, 0xd7],
    [0xaf, 0xff, 0xff],
    [0xd7, 0x00, 0x00],
    [0xd7, 0x00, 0x5f],
    [0xd7, 0x00, 0x87],
    [0xd7, 0x00, 0xaf],
    [0xd7, 0x00, 0xd7],
    [0xd7, 0x00, 0xff],
    [0xd7, 0x5f, 0x00],
    [0xd7, 0x5f, 0x5f],
    [0xd7, 0x5f, 0x87],
    [0xd7, 0x5f, 0xaf],
    [0xd7, 0x5f, 0xd7],
    [0xd7, 0x5f, 0xff],
    [0xd7, 0x87, 0x00],
    [0xd7, 0x87, 0x5f],
    [0xd7, 0x87, 0x87],
    [0xd7, 0x87, 0xaf],
    [0xd7, 0x87, 0xd7],
    [0xd7, 0x87, 0xff],
    [0xd7, 0xaf, 0x00],
    [0xd7, 0xaf, 0x5f],
    [0xd7, 0xaf, 0x87],
    [0xd7, 0xaf, 0xaf],
    [0xd7, 0xaf, 0xd7],
    [0xd7, 0xaf, 0xff],
    [0xd7, 0xd7, 0x00],
    [0xd7, 0xd7, 0x5f],
    [0xd7, 0xd7, 0x87],
    [0xd7, 0xd7, 0xaf],
    [0xd7, 0xd7, 0xd7],
    [0xd7, 0xd7, 0xff],
    [0xd7, 0xff, 0x00],
    [0xd7, 0xff, 0x5f],
    [0xd7, 0xff, 0x87],
    [0xd7, 0xff, 0xaf],
    [0xd7, 0xff, 0xd7],
    [0xd7, 0xff, 0xff],
    [0xff, 0x00, 0x00],
    [0xff, 0x00, 0x5f],
    [0xff, 0x00, 0x87],
    [0xff, 0x00, 0xaf],
    [0xff, 0x00, 0xd7],
    [0xff, 0x00, 0xff],
    [0xff, 0x5f, 0x00],
    [0xff, 0x5f, 0x5f],
    [0xff, 0x5f, 0x87],
    [0xff, 0x5f, 0xaf],
    [0xff, 0x5f, 0xd7],
    [0xff, 0x5f, 0xff],
    [0xff, 0x87, 0x00],
    [0xff, 0x87, 0x5f],
    [0xff, 0x87, 0x87],
    [0xff, 0x87, 0xaf],
    [0xff, 0x87, 0xd7],
    [0xff, 0x87, 0xff],
    [0xff, 0xaf, 0x00],
    [0xff, 0xaf, 0x5f],
    [0xff, 0xaf, 0x87],
    [0xff, 0xaf, 0xaf],
    [0xff, 0xaf, 0xd7],
    [0xff, 0xaf, 0xff],
    [0xff, 0xd7, 0x00],
    [0xff, 0xd7, 0x5f],
    [0xff, 0xd7, 0x87],
    [0xff, 0xd7, 0xaf],
    [0xff, 0xd7, 0xd7],
    [0xff, 0xd7, 0xff],
    [0xff, 0xff, 0x00],
    [0xff, 0xff, 0x5f],
    [0xff, 0xff, 0x87],
    [0xff, 0xff, 0xaf],
    [0xff, 0xff, 0xd7],
    [0xff, 0xff, 0xff],
    [0x08, 0x08, 0x08],
    [0x12, 0x12, 0x12],
    [0x1c, 0x1c, 0x1c],
    [0x26, 0x26, 0x26],
    [0x30, 0x30, 0x30],
    [0x3a, 0x3a, 0x3a],
    [0x44, 0x44, 0x44],
    [0x4e, 0x4e, 0x4e],
    [0x58, 0x58, 0x58],
    [0x62, 0x62, 0x62],
    [0x6c, 0x6c, 0x6c],
    [0x76, 0x76, 0x76],
    [0x80, 0x80, 0x80],
    [0x8a, 0x8a, 0x8a],
    [0x94, 0x94, 0x94],
    [0x9e, 0x9e, 0x9e],
    [0xa8, 0xa8, 0xa8],
    [0xb2, 0xb2, 0xb2],
    [0xbc, 0xbc, 0xbc],
    [0xc6, 0xc6, 0xc6],
    [0xd0, 0xd0, 0xd0],
    [0xda, 0xda, 0xda],
    [0xe4, 0xe4, 0xe4],
    [0xee, 0xee, 0xee],
];
