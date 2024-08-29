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
    StrokeStyle,
    Transform,
};
use rustybuzz::ttf_parser::colr::CompositeMode;

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

pub(crate) struct Painter<'f, 'd, 'p> {
    font: &'f rustybuzz::Face<'d>,
    target: &'f mut DrawTarget<&'p mut [u32]>,
    skew: Transform,
    fake_bold: bool,
    outline: Path,
    scale: f32,
    y_offset: f32,
    x_offset: f32,
    transforms: Vec<rustybuzz::ttf_parser::Transform>,
    modes: Vec<CompositeMode>,
}

impl<'f, 'd, 'p> Painter<'f, 'd, 'p> {
    pub(crate) fn new(
        font: &'f rustybuzz::Face<'d>,
        target: &'f mut DrawTarget<&'p mut [u32]>,
        skew: Transform,
        fake_bold: bool,
        scale: f32,
        y_offset: f32,
        x_offset: f32,
    ) -> Self {
        Self {
            font,
            target,
            skew,
            fake_bold,
            outline: PathBuilder::new().finish(),
            scale,
            y_offset,
            x_offset,
            transforms: vec![],
            modes: vec![CompositeMode::SourceOver],
        }
    }

    fn compute_transform(&self) -> Transform {
        self.transforms
            .iter()
            .fold(Transform::default(), |tfm, t| {
                tfm.then(&Transform::new(t.a, t.b, t.c, t.d, t.e, t.f))
            })
            .then_scale(self.scale, -self.scale)
            .then(&self.skew)
            .then_translate((self.x_offset, self.y_offset * self.scale).into())
    }
}

impl<'f, 'd, 'p, 'a> rustybuzz::ttf_parser::colr::Painter<'a> for Painter<'f, 'd, 'p> {
    fn outline_glyph(&mut self, glyph_id: rustybuzz::ttf_parser::GlyphId) {
        let mut outline = Outline::default();
        if self.font.outline_glyph(glyph_id, &mut outline).is_some() {
            self.outline = outline.finish();
        }
    }

    fn paint(&mut self, paint: rustybuzz::ttf_parser::colr::Paint<'a>) {
        let paint = match paint {
            rustybuzz::ttf_parser::colr::Paint::Solid(color) => {
                Source::Solid(SolidSource::from_unpremultiplied_argb(
                    color.alpha,
                    color.red,
                    color.green,
                    color.blue,
                ))
            }
            rustybuzz::ttf_parser::colr::Paint::LinearGradient(grad) => {
                Source::new_linear_gradient(
                    Gradient {
                        stops: grad
                            .stops(0, &[])
                            .map(|stop| GradientStop {
                                position: stop.stop_offset,
                                color: Color::new(
                                    stop.color.alpha,
                                    stop.color.red,
                                    stop.color.green,
                                    stop.color.blue,
                                ),
                            })
                            .collect(),
                    },
                    Point::new(grad.x0, grad.y0),
                    Point::new(grad.x2, grad.y2),
                    match grad.extend {
                        rustybuzz::ttf_parser::colr::GradientExtend::Pad => raqote::Spread::Pad,
                        rustybuzz::ttf_parser::colr::GradientExtend::Repeat => {
                            raqote::Spread::Repeat
                        }
                        rustybuzz::ttf_parser::colr::GradientExtend::Reflect => {
                            raqote::Spread::Reflect
                        }
                    },
                )
            }
            rustybuzz::ttf_parser::colr::Paint::RadialGradient(grad) => {
                Source::new_radial_gradient(
                    Gradient {
                        stops: grad
                            .stops(0, &[])
                            .map(|stop| GradientStop {
                                position: stop.stop_offset,
                                color: Color::new(
                                    stop.color.alpha,
                                    stop.color.red,
                                    stop.color.green,
                                    stop.color.blue,
                                ),
                            })
                            .collect(),
                    },
                    Point::new(grad.x0, grad.y0),
                    grad.r1 - grad.r0,
                    match grad.extend {
                        rustybuzz::ttf_parser::colr::GradientExtend::Pad => raqote::Spread::Pad,
                        rustybuzz::ttf_parser::colr::GradientExtend::Repeat => {
                            raqote::Spread::Repeat
                        }
                        rustybuzz::ttf_parser::colr::GradientExtend::Reflect => {
                            raqote::Spread::Reflect
                        }
                    },
                )
            }
            rustybuzz::ttf_parser::colr::Paint::SweepGradient(grad) => Source::new_sweep_gradient(
                Gradient {
                    stops: grad
                        .stops(0, &[])
                        .map(|stop| GradientStop {
                            position: stop.stop_offset,
                            color: Color::new(
                                stop.color.alpha,
                                stop.color.red,
                                stop.color.green,
                                stop.color.blue,
                            ),
                        })
                        .collect(),
                },
                Point::new(grad.center_x, grad.center_y),
                grad.start_angle,
                grad.end_angle,
                match grad.extend {
                    rustybuzz::ttf_parser::colr::GradientExtend::Pad => raqote::Spread::Pad,
                    rustybuzz::ttf_parser::colr::GradientExtend::Repeat => raqote::Spread::Repeat,
                    rustybuzz::ttf_parser::colr::GradientExtend::Reflect => raqote::Spread::Reflect,
                },
            ),
        };

        self.target.set_transform(&self.compute_transform());

        let draw_options = DrawOptions {
            blend_mode: match self.modes.last().unwrap() {
                CompositeMode::Clear => raqote::BlendMode::Clear,
                CompositeMode::Source => raqote::BlendMode::Src,
                CompositeMode::Destination => raqote::BlendMode::Dst,
                CompositeMode::SourceOver => raqote::BlendMode::SrcOver,
                CompositeMode::DestinationOver => raqote::BlendMode::DstOver,
                CompositeMode::SourceIn => raqote::BlendMode::SrcIn,
                CompositeMode::DestinationIn => raqote::BlendMode::DstIn,
                CompositeMode::SourceOut => raqote::BlendMode::SrcOut,
                CompositeMode::DestinationOut => raqote::BlendMode::DstOut,
                CompositeMode::SourceAtop => raqote::BlendMode::SrcAtop,
                CompositeMode::DestinationAtop => raqote::BlendMode::DstAtop,
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
                CompositeMode::Hue => raqote::BlendMode::Hue,
                CompositeMode::Saturation => raqote::BlendMode::Saturation,
                CompositeMode::Color => raqote::BlendMode::Color,
                CompositeMode::Luminosity => raqote::BlendMode::Luminosity,
            },
            alpha: 1.0,
            antialias: raqote::AntialiasMode::Gray,
        };

        self.target.fill(&self.outline, &paint, &draw_options);
        if self.fake_bold {
            self.target.stroke(
                &self.outline,
                &paint,
                &StrokeStyle {
                    width: 1.5,
                    ..Default::default()
                },
                &draw_options,
            );
        }
    }

    fn push_clip(&mut self) {
        self.target.set_transform(&self.compute_transform());
        self.target.push_clip(&self.outline);
    }

    fn push_clip_box(&mut self, clipbox: rustybuzz::ttf_parser::colr::ClipBox) {
        let transform = self.compute_transform();
        let min = transform
            .transform_point((clipbox.x_min, clipbox.y_min).into())
            .round();
        let max = transform
            .transform_point((clipbox.x_max, clipbox.y_max).into())
            .round();
        self.target.push_clip_rect(IntRect {
            min: (min.x.min(max.x) as i32, min.y.min(max.y) as i32).into(),
            max: (max.x.max(min.x) as i32, max.y.max(min.y) as i32).into(),
        });
    }

    fn pop_clip(&mut self) {
        self.target.pop_clip();
    }

    fn push_layer(&mut self, mode: rustybuzz::ttf_parser::colr::CompositeMode) {
        self.modes.push(mode);
    }

    fn pop_layer(&mut self) {
        self.modes.pop();
    }

    fn push_transform(&mut self, transform: rustybuzz::ttf_parser::Transform) {
        self.transforms.push(transform);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }
}
