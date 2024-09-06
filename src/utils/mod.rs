use raqote::{
    Color,
    DrawOptions,
    DrawTarget,
    Gradient,
    GradientStop,
    IntRect,
    Path,
    PathBuilder,
    Point,
    SolidSource,
    Source,
    Spread,
    Transform,
    Vector,
};
use skrifa::{
    color::{
        Brush,
        ColorPainter,
        CompositeMode,
    },
    instance::Location,
    metrics::BoundingBox,
    outline::{
        DrawSettings,
        OutlinePen,
    },
    prelude::Size,
    raw::TableProvider,
    MetadataProvider,
};

use crate::Font;

pub(crate) mod lru;
pub(crate) mod plan_cache;
pub(crate) mod text_atlas;

pub(crate) struct Outline {
    path: PathBuilder,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            path: PathBuilder::new(),
        }
    }
}

impl Outline {
    pub(crate) fn finish(self) -> Path {
        self.path.finish()
    }
}

impl rustybuzz::ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(x1, y1, x, y)
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x, y)
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

pub(crate) struct Painter<'f, 'd, 'p> {
    font: &'f Font<'d>,
    target: &'f mut DrawTarget<&'p mut [u32]>,
    skew: Transform,
    scale: f32,
    y_offset: f32,
    x_offset: f32,
    transforms: Vec<skrifa::color::Transform>,
}

impl<'f, 'd, 'p> Painter<'f, 'd, 'p> {
    pub(crate) fn new(
        font: &'f Font<'d>,
        target: &'f mut DrawTarget<&'p mut [u32]>,
        skew: Transform,
        scale: f32,
        y_offset: f32,
        x_offset: f32,
    ) -> Self {
        Self {
            font,
            target,
            skew,
            scale,
            y_offset,
            x_offset,
            transforms: vec![],
        }
    }

    fn compute_transform(&self) -> Transform {
        self.transforms
            .iter()
            .fold(Transform::default(), |tfm, t| {
                tfm.then(&Transform::new(t.xx, t.yx, t.xy, t.yy, t.dx, t.dy))
            })
            .then_scale(self.scale, -self.scale)
            .then(&self.skew)
            .then_translate((self.x_offset, self.y_offset).into())
    }
}

impl<'f, 'd, 'p> ColorPainter for Painter<'f, 'd, 'p> {
    fn push_clip_glyph(&mut self, glyph_id: skrifa::GlyphId) {
        let mut outline = Outline::default();
        if let Some(path) = self
            .font
            .skrifa()
            .outline_glyphs()
            .get(glyph_id)
            .and_then(|glyph_outline| {
                glyph_outline
                    .draw(
                        DrawSettings::unhinted(Size::unscaled(), &Location::default()),
                        &mut outline,
                    )
                    .ok()
            })
            .map(|_| outline.finish())
        {
            self.target
                .push_clip(&path.transform(&self.compute_transform()));
        }
    }

    fn fill(&mut self, brush: Brush<'_>) {
        let paint = match brush {
            Brush::Solid {
                palette_index,
                alpha,
            } => {
                let color = self
                    .font
                    .skrifa()
                    .cpal()
                    .expect("Missing cpal table")
                    .color_records_array()
                    .and_then(Result::ok)
                    .expect("Missing cpal table")[palette_index as usize];
                Source::Solid(SolidSource::from_unpremultiplied_argb(
                    (color.alpha as f32 / 255. * alpha * 255.) as u8,
                    color.red,
                    color.green,
                    color.blue,
                ))
            }
            Brush::LinearGradient {
                p0,
                p1,
                color_stops,
                extend,
            } => {
                let colors = self
                    .font
                    .skrifa()
                    .cpal()
                    .expect("Missing cpal table")
                    .color_records_array()
                    .and_then(Result::ok)
                    .expect("Missing cpal table");

                // https://learn.microsoft.com/en-us/typography/opentype/spec/colr#linear-gradients
                let mut stops = color_stops
                    .iter()
                    .map(|stop| {
                        let color = colors[stop.palette_index as usize];
                        GradientStop {
                            position: stop.offset,
                            color: Color::new(
                                (color.alpha as f32 / 255. * stop.alpha * 255.) as u8,
                                color.red,
                                color.green,
                                color.blue,
                            ),
                        }
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.position.total_cmp(&r.position));

                let transform = self.compute_transform();
                let p0 = transform.transform_point(Point::new(p0.x, p0.y));
                let p1 = transform.transform_point(Point::new(p1.x, p1.y));

                if p0 == p1 {
                    return;
                }

                Source::new_linear_gradient(
                    Gradient { stops },
                    p0,
                    p1,
                    match extend {
                        skrifa::color::Extend::Pad => Spread::Pad,
                        skrifa::color::Extend::Repeat => Spread::Repeat,
                        skrifa::color::Extend::Reflect => Spread::Reflect,
                        _ => {
                            warn!("Malformed font");
                            Spread::Pad
                        }
                    },
                )
            }
            Brush::RadialGradient {
                c0,
                r0,
                c1,
                r1,
                color_stops,
                extend,
            } => {
                let colors = self
                    .font
                    .skrifa()
                    .cpal()
                    .expect("Missing cpal table")
                    .color_records_array()
                    .and_then(Result::ok)
                    .expect("Missing cpal table");

                let mut stops = color_stops
                    .iter()
                    .map(|stop| {
                        let color = colors[stop.palette_index as usize];
                        GradientStop {
                            position: stop.offset,
                            color: Color::new(
                                (color.alpha as f32 / 255. * stop.alpha * 255.) as u8,
                                color.red,
                                color.green,
                                color.blue,
                            ),
                        }
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.position.total_cmp(&r.position));

                let c0 = Point::new(c0.x, c0.y);
                let c1 = Point::new(c1.x, c1.y);

                if c0 == c1 && r0 == r1 {
                    return;
                }

                // TODO: This still produces incorrect output (compare e.g. the rocket from
                // SegoeUIEmoji to other renderers).
                Source::TwoCircleRadialGradient(
                    Gradient { stops },
                    match extend {
                        skrifa::color::Extend::Pad => Spread::Pad,
                        skrifa::color::Extend::Repeat => Spread::Repeat,
                        skrifa::color::Extend::Reflect => Spread::Reflect,
                        _ => {
                            warn!("Malformed font");
                            Spread::Pad
                        }
                    },
                    c0,
                    r0,
                    c1,
                    r1,
                    self.compute_transform().inverse().unwrap_or_default(),
                )
            }
            Brush::SweepGradient {
                c0,
                start_angle,
                end_angle,
                color_stops,
                extend,
            } => {
                let colors = self
                    .font
                    .skrifa()
                    .cpal()
                    .expect("Missing cpal table")
                    .color_records_array()
                    .and_then(Result::ok)
                    .expect("Missing cpal table");

                let mut stops = color_stops
                    .iter()
                    .map(|stop| {
                        let color = colors[stop.palette_index as usize];
                        GradientStop {
                            position: stop.offset,
                            color: Color::new(
                                (color.alpha as f32 / 255. * stop.alpha * 255.) as u8,
                                color.red,
                                color.green,
                                color.blue,
                            ),
                        }
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.position.total_cmp(&r.position));

                Source::SweepGradient(
                    Gradient { stops },
                    match extend {
                        skrifa::color::Extend::Pad => Spread::Pad,
                        skrifa::color::Extend::Repeat => Spread::Repeat,
                        skrifa::color::Extend::Reflect => Spread::Reflect,
                        _ => {
                            warn!("Malformed font");
                            Spread::Pad
                        }
                    },
                    start_angle,
                    end_angle,
                    self.compute_transform()
                        .inverse()
                        .unwrap_or_default()
                        .then_translate(Vector::new(-c0.x, -c0.y)),
                )
            }
        };

        let draw_options = DrawOptions {
            antialias: raqote::AntialiasMode::None,
            ..Default::default()
        };

        self.target.set_transform(&Transform::default());
        self.target.fill_rect(
            0.,
            0.,
            self.target.width() as f32,
            self.target.height() as f32,
            &paint,
            &draw_options,
        );
    }

    fn push_clip_box(&mut self, clipbox: BoundingBox) {
        let transform = self.compute_transform();

        let xy0 = transform.transform_point((clipbox.x_min, clipbox.y_min).into());
        let xy1 = transform.transform_point((clipbox.x_max, clipbox.y_max).into());
        let xy2 = transform.transform_point((clipbox.x_min, clipbox.y_max).into());
        let xy3 = transform.transform_point((clipbox.x_max, clipbox.y_min).into());
        let min_xy = xy0.min(xy1).min(xy2).min(xy3);
        let max_xy = xy0.max(xy1).max(xy2).max(xy3);

        self.target.push_clip_rect(IntRect {
            min: min_xy.to_i32(),
            max: max_xy.to_i32(),
        });
    }

    fn pop_clip(&mut self) {
        self.target.pop_clip();
    }

    fn push_layer(&mut self, mode: CompositeMode) {
        self.target.push_layer_with_blend(
            1.0,
            match mode {
                CompositeMode::Clear => raqote::BlendMode::Clear,
                CompositeMode::Src => raqote::BlendMode::Src,
                CompositeMode::Dest => raqote::BlendMode::Dst,
                CompositeMode::SrcOver => raqote::BlendMode::SrcOver,
                CompositeMode::DestOver => raqote::BlendMode::DstOver,
                CompositeMode::SrcIn => raqote::BlendMode::SrcIn,
                CompositeMode::DestIn => raqote::BlendMode::DstIn,
                CompositeMode::SrcOut => raqote::BlendMode::SrcOut,
                CompositeMode::DestOut => raqote::BlendMode::DstOut,
                CompositeMode::SrcAtop => raqote::BlendMode::SrcAtop,
                CompositeMode::DestAtop => raqote::BlendMode::DstAtop,
                CompositeMode::Xor => raqote::BlendMode::Xor,
                CompositeMode::Plus => raqote::BlendMode::Add,
                CompositeMode::Screen => raqote::BlendMode::Screen,
                CompositeMode::Overlay => raqote::BlendMode::Overlay,
                CompositeMode::Darken => raqote::BlendMode::Darken,
                CompositeMode::Lighten => raqote::BlendMode::Lighten,
                CompositeMode::ColorDodge => raqote::BlendMode::ColorDodge,
                CompositeMode::ColorBurn => raqote::BlendMode::ColorBurn,
                CompositeMode::HardLight => raqote::BlendMode::HardLight,
                CompositeMode::SoftLight => raqote::BlendMode::SoftLight,
                CompositeMode::Difference => raqote::BlendMode::Difference,
                CompositeMode::Exclusion => raqote::BlendMode::Exclusion,
                CompositeMode::Multiply => raqote::BlendMode::Multiply,
                CompositeMode::HslHue => raqote::BlendMode::Hue,
                CompositeMode::HslSaturation => raqote::BlendMode::Saturation,
                CompositeMode::HslColor => raqote::BlendMode::Color,
                CompositeMode::HslLuminosity => raqote::BlendMode::Luminosity,
                _ => {
                    warn!("Malformed font");
                    raqote::BlendMode::SrcOver
                }
            },
        );
    }

    fn pop_layer(&mut self) {
        self.target.pop_layer();
    }

    fn push_transform(&mut self, transform: skrifa::color::Transform) {
        self.transforms.push(transform);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }
}
