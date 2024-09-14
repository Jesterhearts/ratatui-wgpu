use lyon_path::{
    self,
    builder::NoAttributes,
    math::{
        Angle,
        Box2D,
        Point,
        Transform,
        Vector,
    },
    BuilderImpl,
    Path,
};
use palette::Srgba;
use rustybuzz::ttf_parser::colr::CompositeMode;

use crate::graphics::{
    BlendMode,
    Canvas,
    GradientStop,
    SampleMode,
    Shader,
};

pub(crate) mod lru;
pub(crate) mod plan_cache;
pub(crate) mod text_atlas;

pub(crate) struct Outline {
    path: NoAttributes<BuilderImpl>,
}

impl Default for Outline {
    fn default() -> Self {
        Self {
            path: Path::builder(),
        }
    }
}

impl Outline {
    pub(crate) fn finish(self) -> Path {
        self.path.build()
    }
}

impl rustybuzz::ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.begin(Point::new(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(Point::new(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path
            .quadratic_bezier_to(Point::new(x1, y1), Point::new(x, y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path
            .cubic_bezier_to(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y));
    }

    fn close(&mut self) {
        self.path.close();
    }
}

pub(crate) struct Painter<'f, 'd> {
    font: &'f rustybuzz::Face<'d>,
    target: &'f mut Canvas,
    skew: Transform,
    outline: Path,
    scale: f32,
    y_offset: f32,
    x_offset: f32,
    transforms: Vec<rustybuzz::ttf_parser::Transform>,
}

impl<'f, 'd> Painter<'f, 'd> {
    pub(crate) fn new(
        font: &'f rustybuzz::Face<'d>,
        target: &'f mut Canvas,
        skew: Transform,
        scale: f32,
        y_offset: f32,
        x_offset: f32,
    ) -> Self {
        Self {
            font,
            target,
            skew,
            outline: Path::new(),
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
                tfm.then(&Transform::new(t.a, t.b, t.c, t.d, t.e, t.f))
            })
            .then_scale(self.scale, -self.scale)
            .then(&self.skew)
            .then_translate((self.x_offset, self.y_offset).into())
    }
}

impl<'f, 'd, 'a> rustybuzz::ttf_parser::colr::Painter<'a> for Painter<'f, 'd> {
    fn outline_glyph(&mut self, glyph_id: rustybuzz::ttf_parser::GlyphId) {
        let mut outline = Outline::default();
        if self.font.outline_glyph(glyph_id, &mut outline).is_some() {
            self.outline = outline.finish().transformed(&self.compute_transform());
        }
    }

    /// The documentation for this function implies that you should use the
    /// ouline stored from `outline_glyph`, but _actually_ you should be filling
    /// the clipped path with whatever paint is provided.
    /// See skrifa's [`ColorPainter`](https://docs.rs/skrifa/latest/skrifa/color/trait.ColorPainter.html)
    /// for correct documentation.
    fn paint(&mut self, paint: rustybuzz::ttf_parser::colr::Paint<'a>) {
        let shader = match paint {
            rustybuzz::ttf_parser::colr::Paint::Solid(color) => Shader::Solid(
                Srgba::new(color.red, color.green, color.blue, color.alpha).into_format(),
            ),
            rustybuzz::ttf_parser::colr::Paint::LinearGradient(grad) => {
                // https://learn.microsoft.com/en-us/typography/opentype/spec/colr#linear-gradients
                let mut stops = grad
                    .stops(0, &[])
                    .map(|stop| GradientStop {
                        offset: stop.stop_offset,
                        color: Srgba::new(
                            stop.color.red,
                            stop.color.green,
                            stop.color.blue,
                            stop.color.alpha,
                        )
                        .into_linear(),
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.offset.total_cmp(&r.offset));

                let p0 = Point::new(grad.x0, grad.y0);
                let p1 = Point::new(grad.x1, grad.y1);
                let p2 = Point::new(grad.x2, grad.y2);

                if p0 == p1 {
                    return;
                }

                // Get the line perpendicular to p0p2
                let dist = p2 - p0;
                let perp = Vector::new(dist.y, -dist.x);

                // Project the vector p0p1 onto the perpendicular line passing through p0
                let dist = p1 - p0;
                let p3 = p0 + dist.project_onto_vector(perp);

                Shader::LinearGradient {
                    stops,
                    start: p0,
                    end: p3,
                    mode: match grad.extend {
                        rustybuzz::ttf_parser::colr::GradientExtend::Pad => SampleMode::Pad,
                        rustybuzz::ttf_parser::colr::GradientExtend::Repeat => SampleMode::Repeat,
                        rustybuzz::ttf_parser::colr::GradientExtend::Reflect => SampleMode::Reflect,
                    },
                }
            }
            rustybuzz::ttf_parser::colr::Paint::RadialGradient(grad) => {
                let mut stops = grad
                    .stops(0, &[])
                    .map(|stop| GradientStop {
                        offset: stop.stop_offset,
                        color: Srgba::new(
                            stop.color.red,
                            stop.color.green,
                            stop.color.blue,
                            stop.color.alpha,
                        )
                        .into_linear(),
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.offset.total_cmp(&r.offset));

                let c0 = Point::new(grad.x0, grad.y0);
                let c1 = Point::new(grad.x1, grad.y1);

                if c0 == c1 && grad.r0 == grad.r1 {
                    return;
                }

                Shader::ConicalGradient {
                    stops,
                    c0,
                    r0: grad.r0,
                    c1,
                    r1: grad.r1,
                    mode: match grad.extend {
                        rustybuzz::ttf_parser::colr::GradientExtend::Pad => SampleMode::Pad,
                        rustybuzz::ttf_parser::colr::GradientExtend::Repeat => SampleMode::Repeat,
                        rustybuzz::ttf_parser::colr::GradientExtend::Reflect => SampleMode::Reflect,
                    },
                }
            }
            rustybuzz::ttf_parser::colr::Paint::SweepGradient(grad) => {
                let mut stops = grad
                    .stops(0, &[])
                    .map(|stop| GradientStop {
                        offset: stop.stop_offset,
                        color: Srgba::new(
                            stop.color.red,
                            stop.color.green,
                            stop.color.blue,
                            stop.color.alpha,
                        )
                        .into_linear(),
                    })
                    .collect::<Vec<_>>();
                stops.sort_by(|l, r| l.offset.total_cmp(&r.offset));

                Shader::SweepGradient {
                    stops,
                    center: Point::new(grad.center_x, grad.center_y),
                    start: Angle::degrees(grad.start_angle),
                    end: Angle::degrees(grad.end_angle),
                    mode: match grad.extend {
                        rustybuzz::ttf_parser::colr::GradientExtend::Pad => SampleMode::Pad,
                        rustybuzz::ttf_parser::colr::GradientExtend::Repeat => SampleMode::Repeat,
                        rustybuzz::ttf_parser::colr::GradientExtend::Reflect => SampleMode::Reflect,
                    },
                }
            }
        };

        self.target.fill(
            &shader,
            BlendMode::default(),
            self.compute_transform().inverse().unwrap_or_default(),
        );
    }

    fn push_clip(&mut self) {
        self.target.push_clip(&self.outline);
    }

    fn push_clip_box(&mut self, clipbox: rustybuzz::ttf_parser::colr::ClipBox) {
        let transform = self.compute_transform();
        let clip = Box2D::new(
            Point::new(clipbox.x_min, clipbox.y_min),
            Point::new(clipbox.x_max, clipbox.y_max),
        );

        let clip = transform.outer_transformed_box(&clip);

        self.target.push_clip_rect(clip);
    }

    fn pop_clip(&mut self) {
        self.target.pop_clip();
    }

    fn push_layer(&mut self, mode: rustybuzz::ttf_parser::colr::CompositeMode) {
        self.target.push_layer(match mode {
            CompositeMode::Clear => BlendMode::Clear,
            CompositeMode::Source => BlendMode::Source,
            CompositeMode::Destination => BlendMode::Destination,
            CompositeMode::SourceOver => BlendMode::SourceOver,
            CompositeMode::DestinationOver => BlendMode::DestinationOver,
            CompositeMode::SourceIn => BlendMode::SourceIn,
            CompositeMode::DestinationIn => BlendMode::DestinationIn,
            CompositeMode::SourceOut => BlendMode::SourceOut,
            CompositeMode::DestinationOut => BlendMode::DestinationOut,
            CompositeMode::SourceAtop => BlendMode::SourceAtop,
            CompositeMode::DestinationAtop => BlendMode::DestinationAtop,
            CompositeMode::Xor => BlendMode::Xor,
            CompositeMode::Plus => BlendMode::Plus,
            CompositeMode::Screen => BlendMode::Screen,
            CompositeMode::Overlay => BlendMode::Overlay,
            CompositeMode::Darken => BlendMode::Darken,
            CompositeMode::Lighten => BlendMode::Lighten,
            CompositeMode::ColorDodge => BlendMode::ColorDodge,
            CompositeMode::ColorBurn => BlendMode::ColorBurn,
            CompositeMode::HardLight => BlendMode::HardLight,
            CompositeMode::SoftLight => BlendMode::SoftLight,
            CompositeMode::Difference => BlendMode::Difference,
            CompositeMode::Exclusion => BlendMode::Exclusion,
            CompositeMode::Multiply => BlendMode::Multiply,
            CompositeMode::Hue => BlendMode::Hue,
            CompositeMode::Saturation => BlendMode::Saturation,
            CompositeMode::Color => BlendMode::Color,
            CompositeMode::Luminosity => BlendMode::Luminosity,
        });
    }

    fn pop_layer(&mut self) {
        self.target.pop_layer();
    }

    fn push_transform(&mut self, transform: rustybuzz::ttf_parser::Transform) {
        self.transforms.push(transform);
    }

    fn pop_transform(&mut self) {
        self.transforms.pop();
    }
}
