use ratatui::{
    buffer::Cell,
    style::Modifier,
};
use swash::{
    text::cluster::{
        CharCluster,
        Status,
    },
    CacheKey,
    FontRef,
    Metrics,
};

/// A Font which can be used for rendering.
#[derive(Clone)]
pub struct Font<'a> {
    font: FontRef<'a>,
    metrics: Metrics,
}

impl<'a> Font<'a> {
    /// Create a new Font from data. Returns [`None`] if the font cannot
    /// be parsed.
    pub fn new(data: &'a [u8]) -> Option<Self> {
        FontRef::from_index(data, 0).map(|font| Self {
            font,
            metrics: font.metrics(&[]),
        })
    }
}

impl Font<'_> {
    pub(crate) fn metrics(&self) -> Metrics {
        self.metrics
    }

    pub(crate) fn key(&self) -> CacheKey {
        self.font.key
    }

    pub(crate) fn font(&self) -> FontRef {
        self.font
    }

    pub(crate) fn char_width(&self, height_px: u32) -> u32 {
        let scale = height_px as f32 / (self.metrics.ascent + self.metrics.descent);
        self.font
            .glyph_metrics(&[])
            .linear_scale(scale)
            .advance_width(self.font.charmap().map('m')) as u32
    }
}

/// A collection of fonts to use for rendering. Supports font fallback.
///
/// It is recommended, but not required, that all fonts have the same/very
/// similar aspect ratio, or you may get unexpected results during rendering due
/// to fallback.
pub struct Fonts<'a> {
    char_width: u32,
    char_height: u32,

    last_resort: Font<'a>,

    regular: Vec<Font<'a>>,
    bold: Vec<Font<'a>>,
    italic: Vec<Font<'a>>,
    bold_italic: Vec<Font<'a>>,
}

impl<'a> Fonts<'a> {
    /// Create a new, empty set of fonts. The provided font will be used as a
    /// last-resort fallback if no other fonts can render a particular
    /// character. Rendering will attempt to fake bold/italic styles using this
    /// font where appropriate.
    ///
    /// The provided size_px will be the rendered height in pixels of all fonts
    /// in this collection.
    pub fn new(font: Font<'a>, size_px: u32) -> Self {
        Self {
            char_width: font.char_width(size_px),
            char_height: size_px,
            last_resort: font,
            regular: vec![],
            bold: vec![],
            italic: vec![],
            bold_italic: vec![],
        }
    }

    /// The height (in pixels) of all fonts.
    #[inline]
    pub fn height_px(&self) -> u32 {
        self.char_height
    }

    /// The maximum width (in pixels) across all fonts.
    #[inline]
    pub fn width_px(&self) -> u32 {
        self.char_width
    }

    /// Change the height of all fonts in this collection to the specified
    /// height in pixels.
    pub fn set_size_px(&mut self, height_px: u32) {
        self.char_height = height_px;

        self.char_width = std::iter::once(&self.last_resort)
            .chain(self.regular.iter())
            .chain(self.bold.iter())
            .chain(self.italic.iter())
            .chain(self.bold_italic.iter())
            .map(|font| font.char_width(height_px))
            .max()
            .unwrap_or_default();
    }

    /// Add a new collection of fonts for regular styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    pub fn add_regular_fonts(&mut self, fonts: impl IntoIterator<Item = Font<'a>>) {
        self.char_width =
            self.char_width
                .max(Self::add_fonts(&mut self.regular, fonts, self.char_height));
    }

    /// Add a new collection of fonts for bold styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    ///
    /// You do not have to provide these for bold text to be supported. If no
    /// bold fonts are supplied, rendering will fallback to the regular fonts
    /// with fake bolding.
    pub fn add_bold_fonts(&mut self, fonts: impl IntoIterator<Item = Font<'a>>) {
        self.char_width =
            self.char_width
                .max(Self::add_fonts(&mut self.bold, fonts, self.char_height));
    }

    /// Add a new collection of fonts for italic styled text. These fonts will
    /// come _after_ previously provided fonts in the fallback order.
    ///
    /// It is recommended, but not required, that you provide italic fonts if
    /// your application intends to make use of italics. If no italic fonts
    /// are supplied, rendering will fallback to the regular fonts with fake
    /// italics.
    pub fn add_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font<'a>>) {
        self.char_width =
            self.char_width
                .max(Self::add_fonts(&mut self.italic, fonts, self.char_height));
    }

    /// Add a new collection of fonts for bold italic styled text. These fonts
    /// will come _after_ previously provided fonts in the fallback order.
    ///
    /// You do not have to provide these for bold text to be supported. If no
    /// bold fonts are supplied, rendering will fallback to the italic fonts
    /// with fake bolding.
    pub fn add_bold_italic_fonts(&mut self, fonts: impl IntoIterator<Item = Font<'a>>) {
        self.char_width = self.char_width.max(Self::add_fonts(
            &mut self.bold_italic,
            fonts,
            self.char_height,
        ));
    }
}

impl<'a> Fonts<'a> {
    pub(crate) fn last_resort(&self) -> &Font {
        &self.last_resort
    }

    pub(crate) fn font_for_cell(
        &self,
        cluster: &mut CharCluster,
        cell: &Cell,
    ) -> (&Font, bool, bool) {
        if cell.modifier.contains(Modifier::BOLD | Modifier::ITALIC) {
            self.select_font(
                cluster,
                self.bold_italic
                    .iter()
                    .map(|f| (f, false, false))
                    .chain(self.italic.iter().map(|f| (f, true, false)))
                    .chain(self.bold.iter().map(|f| (f, false, true)))
                    .chain(self.regular.iter().map(|f| (f, true, true))),
                true,
                true,
            )
        } else if cell.modifier.contains(Modifier::BOLD) {
            self.select_font(
                cluster,
                self.bold
                    .iter()
                    .map(|f| (f, false, false))
                    .chain(self.regular.iter().map(|f| (f, true, false))),
                true,
                false,
            )
        } else if cell.modifier.contains(Modifier::ITALIC) {
            self.select_font(
                cluster,
                self.italic
                    .iter()
                    .map(|f| (f, false, false))
                    .chain(self.regular.iter().map(|f| (f, false, true))),
                false,
                true,
            )
        } else {
            self.select_font(
                cluster,
                self.regular.iter().map(|f| (f, false, false)),
                false,
                false,
            )
        }
    }

    fn select_font<'fonts>(
        &'fonts self,
        cluster: &mut CharCluster,
        fonts: impl IntoIterator<Item = (&'fonts Font<'a>, bool, bool)>,
        last_resort_fake_bold: bool,
        last_resort_fake_italic: bool,
    ) -> (&'fonts Font<'a>, bool, bool) {
        let mut font = None;
        for (candidate, fake_bold, fake_italic) in fonts.into_iter().chain(std::iter::once((
            &self.last_resort,
            last_resort_fake_bold,
            last_resort_fake_italic,
        ))) {
            let map = candidate.font.charmap();
            match cluster.map(|ch| map.map(ch)) {
                Status::Discard => continue,
                Status::Keep => font = Some((candidate, fake_bold, fake_italic)),
                Status::Complete => {
                    font = Some((candidate, fake_bold, fake_italic));
                    break;
                }
            }
        }

        *font.get_or_insert((
            &self.last_resort,
            last_resort_fake_bold,
            last_resort_fake_italic,
        ))
    }

    fn add_fonts(
        target: &mut Vec<Font<'a>>,
        fonts: impl IntoIterator<Item = Font<'a>>,
        char_height: u32,
    ) -> u32 {
        let len = target.len();
        target.extend(fonts);

        target[len..]
            .iter()
            .map(|font| font.char_width(char_height))
            .max()
            .unwrap_or_default()
    }
}
