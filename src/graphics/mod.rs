use std::{
    f32::consts::PI,
    ops::Range,
};

use lyon_path::{
    geom::{
        self,
        CubicBezierSegment,
        LineSegment,
        QuadraticBezierSegment,
    },
    math::{
        Angle,
        Box2D,
        Point,
        Transform,
        Vector,
    },
    Path,
};
use palette::{
    blend::{
        Blend,
        Compose,
    },
    rgb::{
        PackedRgba,
        Rgba,
    },
    FromColor,
    IntoColor,
    Lcha,
    LinSrgba,
    Mix,
};

const TOLERANCE: f32 = 1. / 256.;
const SCALE: f32 = 8.;

const ALPHA_X_LUT: [f32; SCALE as usize] = [
    0.,
    1. / 7. * 1. / 8.,
    2. / 7. * 1. / 8.,
    3. / 7. * 1. / 8.,
    4. / 7. * 1. / 8.,
    5. / 7. * 1. / 8.,
    6. / 7. * 1. / 8.,
    1. / 8.,
];

type Lut = [Rgba; 256];

pub(crate) struct Image<'d> {
    data: &'d mut [Rgba],
    width: u32,
    height: u32,
}

impl<'d> Image<'d> {
    pub(crate) fn new(data: &'d mut [Rgba], width: u32, height: u32) -> Self {
        debug_assert_eq!((width as usize * height as usize), data.len(),);
        Self {
            data,
            width,
            height,
        }
    }

    fn get(&self, point: Point) -> Rgba {
        // Just nearest sampling for now
        let point = point.round().to_u32();
        self.data[(point.y * self.width + point.x) as usize]
    }

    #[cfg(test)]
    #[allow(dead_code)] // used for debugging and updating goldens
    pub(crate) fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        Self::save_raw(self.data, self.width, self.height, path)
    }

    #[cfg(test)]
    pub(crate) fn save_raw(
        data: &[Rgba],
        width: u32,
        height: u32,
        path: impl AsRef<std::path::Path>,
    ) -> std::io::Result<()> {
        use std::fs::File;

        use palette::{
            rgb::PackedArgb,
            Srgba,
        };
        use png::Encoder;

        let file = File::create(path)?;
        let mut writer = Encoder::new(file, width, height);
        writer.set_color(png::ColorType::Rgba);

        let mut writer = writer.write_header()?;
        let data = data
            .iter()
            .map(|c| PackedArgb::pack(Srgba::<u8>::from_format(*c)).color)
            .collect::<Vec<u32>>();
        writer.write_image_data(bytemuck::cast_slice(&data))?;

        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn from_raw(bytes: &[u8]) -> Vec<Rgba> {
        use palette::{
            rgb::PackedArgb,
            Srgba,
        };

        let data: &[u32] = bytemuck::cast_slice(bytes);

        data.iter()
            .copied()
            .map(|d| Srgba::<u8>::from(PackedArgb::from(d).unpack()).into_format())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn load_from_memory(bytes: &[u8]) -> std::io::Result<(Vec<Rgba>, u32, u32)> {
        use std::io::Cursor;

        use png::Decoder;

        let decoder = Decoder::new(Cursor::new(bytes));
        let mut reader = decoder.read_info()?;

        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf)?;

        Ok((
            Self::from_raw(&buf[..info.buffer_size()]),
            info.width,
            info.height,
        ))
    }
}

#[derive(Default)]
struct ClipMask {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

impl ClipMask {
    fn new_unmasked(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![1.; width as usize * height as usize],
        }
    }

    fn re_use(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.data.clear();
        self.data
            .resize(self.width as usize * self.height as usize, 0.);
    }

    fn row_slice_mut(&mut self, y: usize) -> &mut [f32] {
        let y = (y * self.width as usize).min(self.data.len());
        &mut self.data[y..y + self.width as usize]
    }

    #[cfg(test)]
    fn to_color(&self) -> Vec<Rgba> {
        self.data
            .iter()
            .copied()
            .map(|a| Rgba::new(a, a, a, 1.))
            .collect()
    }

    #[cfg(test)]
    #[allow(dead_code)] // used for debugging and updating goldens
    fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let mut colors = self.to_color();
        let image = Image::new(&mut colors, self.width, self.height);
        image.save(path)
    }
}

impl std::fmt::Debug for ClipMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ClipMask {}x{}", self.width, self.height)?;
        for y in 0..self.height {
            let y = y as usize * self.width as usize;
            let row = &self.data[y..y + self.width as usize];
            for val in row.iter() {
                write!(f, "{:.2},", val)?;
            }
            writeln!(f)?;
        }

        Ok(())
    }
}

#[derive(Debug)]
struct Edge {
    dx: f32,
    x: f32,
    y_range: Range<usize>,
    positive_winding: bool,
}

impl Edge {
    fn new(mut line: LineSegment<f32>) -> Option<Self> {
        line.from *= SCALE;
        line.to *= SCALE;

        let positive_winding = line.from.y < line.to.y;
        if !positive_winding {
            std::mem::swap(&mut line.from, &mut line.to);
        }

        let top = line.from.y.round();
        let bottom = line.to.y.round();

        if top == bottom {
            return None;
        }

        let dx = (line.to.x - line.from.x) / (line.to.y - line.from.y);
        let offset_y = top - line.from.y;

        let x = line.from.x + dx * offset_y;

        Some(Self {
            dx,
            x,
            y_range: top as usize..bottom as usize,
            positive_winding,
        })
    }
}

#[derive(Debug, Default)]
struct Rasterizer {
    edges_by_y: Vec<Vec<Edge>>,
    current_edges: Vec<Edge>,

    height: usize,
}

impl Rasterizer {
    fn rasterize_path(&mut self, path: &Path, out: &mut ClipMask) {
        let y_bounds = 0f32..out.height as f32;

        for control in path.iter() {
            match control {
                lyon_path::Event::Begin { .. } => {}
                lyon_path::Event::End { close: false, .. } => {}
                lyon_path::Event::Line { from, to }
                | lyon_path::Event::End {
                    last: from,
                    first: to,
                    close: true,
                } => {
                    let Some(line) = LineSegment { from, to }.clipped_y(y_bounds.clone()) else {
                        continue;
                    };

                    let Some(edge) = Edge::new(line) else {
                        continue;
                    };

                    self.height = self.height.max(edge.y_range.end);

                    self.edges_by_y
                        .resize_with(self.height.max(self.edges_by_y.len()), Vec::new);
                    self.edges_by_y[edge.y_range.start].push(edge);
                }
                lyon_path::Event::Quadratic { from, ctrl, to } => {
                    let segment = QuadraticBezierSegment { from, ctrl, to };
                    let (_, maxy) = segment.fast_bounding_range_y();
                    self.edges_by_y.resize_with(
                        self.height
                            .max((maxy * SCALE) as usize + 1)
                            .max(self.edges_by_y.len()),
                        Vec::new,
                    );

                    segment.for_each_flattened(TOLERANCE, &mut |line| {
                        let Some(line) = line.clipped_y(y_bounds.clone()) else {
                            return;
                        };

                        let Some(edge) = Edge::new(line) else {
                            return;
                        };

                        self.height = self.height.max(edge.y_range.end);
                        self.edges_by_y[edge.y_range.start].push(edge);
                    });
                }
                lyon_path::Event::Cubic {
                    from,
                    ctrl1,
                    ctrl2,
                    to,
                } => {
                    let segment = CubicBezierSegment {
                        from,
                        ctrl1,
                        ctrl2,
                        to,
                    };
                    let (_, maxy) = segment.fast_bounding_range_y();
                    self.edges_by_y.resize_with(
                        self.height
                            .max((maxy * SCALE) as usize + 1)
                            .max(self.edges_by_y.len()),
                        Vec::new,
                    );

                    segment.for_each_flattened(TOLERANCE, &mut |line| {
                        let Some(line) = line.clipped_y(y_bounds.clone()) else {
                            return;
                        };

                        let Some(edge) = Edge::new(line) else {
                            return;
                        };

                        self.height = self.height.max(edge.y_range.end);
                        self.edges_by_y[edge.y_range.start].push(edge);
                    })
                }
            }
        }

        for y in 0..self.height {
            if y / SCALE as usize >= out.height as usize {
                break;
            }

            self.current_edges.retain(|edge| edge.y_range.end > y);
            for edge in self.current_edges.iter_mut() {
                edge.x += edge.dx;
            }
            self.current_edges.append(&mut self.edges_by_y[y]);
            self.current_edges.sort_by(|l, r| l.x.total_cmp(&r.x));

            let mut winding = 0;
            let mut left = 0;

            for edge in self.current_edges.iter() {
                if winding == 0 {
                    left = edge.x.round() as usize;
                }

                if (left / SCALE as usize) >= out.width as usize {
                    break;
                }

                winding += if edge.positive_winding { 1 } else { -1 };

                let right = edge.x.round();
                if right >= 0. && winding == 0 {
                    let l = left % SCALE as usize;
                    let la = ALPHA_X_LUT[SCALE as usize - (l + 1)];

                    let right = right as usize;
                    let mut ra = ALPHA_X_LUT[right % SCALE as usize];

                    let mut right = right / SCALE as usize;
                    if right >= out.width as usize {
                        right = out.width as usize - 1;
                        ra = ALPHA_X_LUT[ALPHA_X_LUT.len() - 1];
                    }
                    let left = left / SCALE as usize;

                    let row = out.row_slice_mut(y / SCALE as usize);
                    let row = &mut row[left..=right];

                    if row.len() == 1 {
                        row[0] = (row[0] + ra - la).min(1.);
                    } else {
                        row[0] = (row[0] + la).min(1.);
                        let right = row.len() - 1;
                        row[right] = (row[right] + ra).min(1.);

                        for dst in row[1..right].iter_mut() {
                            *dst = (*dst + 1. / SCALE).min(1.);
                        }
                    }
                }
            }
        }

        self.current_edges.clear();
        for row in &mut self.edges_by_y[(out.height as usize).min(self.height)..] {
            row.clear();
        }
        self.height = 0;
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) enum BlendMode {
    /// Clear with transparent
    Clear,
    /// Copy source
    Source,
    /// Copy dest
    Destination,
    /// Copy from source. Where source is transparent, preserve dest.
    #[default]
    SourceOver,
    /// Copy from destination. Where dest is transparent, preserve source.
    DestinationOver,
    /// Copy from source, only keeping pixels which overlap non-transparent
    /// pixels in dest.
    SourceIn,
    /// Copy from dest, only keeping pixels which overlap non-transparent pixels
    /// in source.
    DestinationIn,
    /// Copy from source, only keeping pixels which overlap transparent pixels
    /// in dest.
    SourceOut,
    /// Copy from dest, only keeping pixels which overlap transparent pixels in
    /// source.
    DestinationOut,
    /// Copy from source and dest. Only take source pixels which overlap
    /// non-transparent pixels in dest.
    SourceAtop,
    /// Copy from dest and source. Only take dest pixels which overlap
    /// non-transparent pixels in source.
    DestinationAtop,
    /// Copy from source and dest. Only take pixels where one or the other is
    /// blank.
    Xor,
    /// Saturating add source and dest.
    Plus,
    /// Invert source and dest, multiply, then invert the result.
    Screen,
    /// Where dest is light, lighten source. Where dest is dark, darken source.
    Overlay,
    /// Minimum of dest and source.
    Darken,
    /// Maximum of dest and source.
    Lighten,
    /// Dest divided by inverted source.
    ColorDodge,
    /// Dest divided by inverted source, then inverted.
    ColorBurn,
    /// The same as overlay, with dest and source reversed.
    HardLight,
    /// Like overlay, but don't produce pure white/black.
    SoftLight,
    /// Maximum of dest - source, source - dest. Saturate to 0 if both are
    /// negative.
    Difference,
    /// Sum dest and source, then subtract double the product.
    Exclusion,
    /// Multiply dest and source.
    Multiply,
    /// Preserve luma & chroma of dest, copy hue of source.
    Hue,
    /// Preserve luma & hue of dest, copy chroma of source.
    Saturation,
    /// Preserve luma of dest, copy hue & chroma of source.
    Color,
    /// Preserve hue & chroma of dest, copy luma of source.
    Luminosity,
}

impl BlendMode {
    fn blend_fn(&self) -> fn(dest: Rgba, source: Rgba) -> Rgba {
        match self {
            BlendMode::Clear => |_, _| Rgba::default(),
            BlendMode::Source => |_, src| src,
            BlendMode::Destination => |dst, _| dst,
            BlendMode::SourceOver => |dst, src| src.over(dst),
            BlendMode::DestinationOver => |dst, src| dst.over(src),
            BlendMode::SourceIn => |dst, src| src.inside(dst),
            BlendMode::DestinationIn => |dst, src| dst.inside(src),
            BlendMode::SourceOut => |dst, src| src.outside(dst),
            BlendMode::DestinationOut => |dst, src| dst.outside(src),
            BlendMode::SourceAtop => |dst, src| src.atop(dst),
            BlendMode::DestinationAtop => |dst, src| dst.atop(src),
            BlendMode::Xor => |dst, src| dst.xor(src),
            BlendMode::Plus => |dst, src| dst.plus(src),
            BlendMode::Screen => |dst, src| src.screen(dst),
            BlendMode::Overlay => |dst, src| src.overlay(dst),
            BlendMode::Darken => |dst, src| src.darken(dst),
            BlendMode::Lighten => |dst, src| src.lighten(dst),
            BlendMode::ColorDodge => |dst, src| src.dodge(dst),
            BlendMode::ColorBurn => |dst, src| src.burn(dst),
            BlendMode::HardLight => |dst, src| src.hard_light(dst),
            BlendMode::SoftLight => |dst, src| src.soft_light(dst),
            BlendMode::Difference => |dst, src| src.difference(dst),
            BlendMode::Exclusion => |dst, src| src.exclusion(dst),
            BlendMode::Multiply => |dst, src| src.multiply(dst),
            BlendMode::Hue => |dst, src| {
                let mut dst = Lcha::from_color(dst);
                let src = Lcha::from_color(src);
                dst.hue = src.hue;
                dst.into_color()
            },
            BlendMode::Saturation => |dst, src| {
                let mut dst = Lcha::from_color(dst);
                let src = Lcha::from_color(src);
                dst.chroma = src.chroma;
                dst.into_color()
            },
            BlendMode::Color => |dst, src| {
                let mut dst = Lcha::from_color(dst);
                let src = Lcha::from_color(src);
                dst.hue = src.hue;
                dst.chroma = src.chroma;
                dst.into_color()
            },
            BlendMode::Luminosity => |dst, src| {
                let mut dst = Lcha::from_color(dst);
                let src = Lcha::from_color(src);
                dst.l = src.l;
                dst.into_color()
            },
        }
    }
}

pub(crate) struct GradientStop {
    pub(crate) offset: f32,
    pub(crate) color: LinSrgba,
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) enum SampleMode {
    /// Keep sampling the max/min value when out of bounds.
    #[default]
    Pad,
    /// Sample mod bounds.
    Repeat,
    /// Sample bouncing back when reaching bounds.
    Reflect,
}

impl SampleMode {
    fn sample_fn(&self) -> fn(point: Point, width: u32, height: u32) -> Point {
        match self {
            SampleMode::Pad => |point, width, height| {
                point.clamp(Point::new(0., 0.), Point::new(width as f32, height as f32))
            },
            SampleMode::Repeat => {
                |point, width, height| Point::new(point.x % width as f32, point.y % height as f32)
            }
            SampleMode::Reflect => |point, width, height| {
                // The way this works is as follows - you can think of mirroring as sampling
                // along a number line like so:
                // -inf                             0                             +inf
                // ...[width<-0][0<-width][width<-0][0->width][width->0][0->width]...
                // As you can see it is symmetric around 0. This means we can take the abs of
                // the value and still end up at the same relative location in the pattern.
                // The next bit is the check for / width is even. As should hopefully be obvious
                // from the example above, the pattern flips every width, starting with 0..width
                // at 0. This means that if we are an even number of width steps from the start,
                // we just want to index directly into our array. If we are an odd number of
                // steps, we started counting down from width, so we want to subtract the
                // prospective index from the width.
                let x = point.x.abs() % width as f32;
                let y = point.y.abs() % height as f32;

                let x = if (point.x.abs() as u32 / width) % 2 == 0 {
                    x
                } else {
                    (width - 1) as f32 - x
                };

                let y = if (point.y.abs() as u32 / height) % 2 == 0 {
                    y
                } else {
                    (height - 1) as f32 - y
                };

                Point::new(x, y)
            },
        }
    }
}

pub(crate) enum Shader<'d> {
    Solid(Rgba),
    Image {
        image: Image<'d>,
        mode: SampleMode,
    },
    LinearGradient {
        stops: Vec<GradientStop>,
        start: Point,
        end: Point,
        mode: SampleMode,
    },
    ConicalGradient {
        stops: Vec<GradientStop>,
        c0: Point,
        r0: f32,
        c1: Point,
        r1: f32,
        mode: SampleMode,
    },
    SweepGradient {
        stops: Vec<GradientStop>,
        center: Point,
        start: Angle,
        end: Angle,
        mode: SampleMode,
    },
}

impl Shader<'_> {
    fn get_transform(&self) -> Transform {
        match self {
            Shader::Solid(_) => Transform::default(),
            Shader::Image { .. } => Transform::default(),
            Shader::LinearGradient { start, end, .. } => {
                if start == end {
                    // ??? There's no good way to express this transform, so good luck with that.
                    return Transform::scale(0., 0.);
                }
                // We want a transform so that all incomming points align along the x axis in
                // the range 0..1
                let dist = *start - *end;

                let length = dist.length();
                let dist = dist.normalize();

                // Translate the point to align with the start
                Transform::translation(-start.x, -start.y)
                    // Rotate all the points so that they line up
                    .then_rotate(-dist.angle_from_x_axis())
                    // Scale to be in range 0..1
                    .then_scale(1. / length, 1. / length)
            }
            Shader::ConicalGradient { .. } => Transform::default(),
            Shader::SweepGradient { center, .. } => {
                // We just want to align all our points to the center location. The math in the
                // gradient takes care of translating to a linear value on 0..1
                Transform::translation(-center.x, -center.y)
            }
        }
    }
}

#[derive(Default)]
struct Layer {
    data: Vec<Rgba>,
    width: u32,
    height: u32,
    blend: BlendMode,
}

impl Layer {
    fn re_use(&mut self, width: u32, height: u32, blend: BlendMode) {
        self.data
            .resize(width as usize * height as usize, Rgba::default());
        self.width = width;
        self.height = height;
        self.blend = blend;
    }

    fn image_view(&mut self) -> Image {
        Image::new(&mut self.data, self.width, self.height)
    }
}

pub(crate) struct Canvas {
    result: Vec<Rgba>,
    height: u32,
    width: u32,

    points: Vec<Point>,

    layers: Vec<Layer>,
    old_layers: Vec<Layer>,

    rasterizer: Rasterizer,

    masks: Vec<ClipMask>,
    old_masks: Vec<ClipMask>,

    last_mask: ClipMask,
}

impl Canvas {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        Self {
            result: vec![Rgba::default(); width as usize * height as usize],
            height,
            width,
            points: vec![Point::zero(); width as usize * height as usize],
            layers: vec![],
            old_layers: vec![],
            rasterizer: Rasterizer::default(),
            masks: vec![],
            old_masks: vec![],
            last_mask: ClipMask::new_unmasked(width, height),
        }
    }

    pub(crate) fn pack_result(&self) -> Vec<u32> {
        if !self.layers.is_empty() {
            warn!("Packing result image with pending layers");
        }

        let mut out = vec![0; self.result.len()];
        for (dst, color) in out.iter_mut().zip(self.result.iter()) {
            *dst = PackedRgba::from(color.into_format()).color;
        }

        out
    }

    pub(crate) fn reset(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;

        self.result.clear();
        self.result
            .resize(width as usize * height as usize, Rgba::default());

        self.points.clear();
        self.points
            .resize(width as usize * height as usize, Point::zero());

        self.layers.clear();
        self.masks.clear();
    }

    pub(crate) fn fill(&mut self, shader: &Shader, blend: BlendMode, transform: Transform) {
        let transform = transform.then(&shader.get_transform());

        for y in 0..self.height {
            for x in 0..self.width {
                let point = Point::new(x as f32, y as f32);
                if matches!(shader, Shader::Solid(_)) || transform.approx_eq(&Transform::identity())
                {
                    self.points[(y * self.width + x) as usize] = point;
                } else {
                    self.points[(y * self.width + x) as usize] = transform.transform_point(point);
                }
            }
        }

        let blend_fn = blend.blend_fn();
        let mask = self.masks.last().unwrap_or(&self.last_mask);
        let dest = self
            .layers
            .last_mut()
            .map(|layer| &mut layer.data)
            .unwrap_or(&mut self.result);

        match shader {
            Shader::Image { image, mode } => {
                let sample_fn = mode.sample_fn();
                fill_with_image(
                    image,
                    &self.points,
                    sample_fn,
                    blend_fn,
                    mask,
                    Image::new(dest, self.width, self.height),
                );
            }
            Shader::Solid(color) => {
                fill_with_color(
                    *color,
                    blend_fn,
                    mask,
                    Image::new(dest, self.width, self.height),
                );
            }
            Shader::LinearGradient { stops, mode, .. } => {
                fill_with_linear_gradient(
                    stops,
                    &self.points,
                    *mode,
                    blend_fn,
                    mask,
                    Image::new(dest, self.width, self.height),
                );
            }
            Shader::ConicalGradient {
                stops,
                c0,
                r0,
                c1,
                r1,
                mode,
            } => {
                fill_with_conical_gradient(
                    stops,
                    &self.points,
                    *mode,
                    blend_fn,
                    mask,
                    *c0,
                    *r0,
                    *c1,
                    *r1,
                    Image::new(dest, self.width, self.height),
                );
            }
            Shader::SweepGradient {
                stops,
                start,
                end,
                mode,
                ..
            } => {
                fill_with_sweep_gradient(
                    stops,
                    &self.points,
                    *mode,
                    blend_fn,
                    mask,
                    *start,
                    *end,
                    Image::new(dest, self.width, self.height),
                );
            }
        }
    }

    pub(crate) fn push_layer(&mut self, blend: BlendMode) {
        let mut new_layer = self.old_layers.pop().unwrap_or_default();
        new_layer.re_use(self.width, self.height, blend);
        self.layers.push(new_layer);
    }

    pub(crate) fn pop_layer(&mut self) {
        let Some(mut layer) = self.layers.pop() else {
            warn!("Pop layer called without any active layers");
            return;
        };

        let blend = layer.blend;
        self.fill(
            &Shader::Image {
                image: layer.image_view(),
                mode: SampleMode::Pad,
            },
            blend,
            Transform::default(),
        );
        self.old_layers.push(layer);
    }

    pub(crate) fn push_clip(&mut self, path: &Path) {
        let mut mask = self.old_masks.pop().unwrap_or_default();
        mask.re_use(self.width, self.height);

        self.rasterizer.rasterize_path(path, &mut mask);
        if let Some(last) = self.masks.last() {
            for (dst, src) in mask.data.iter_mut().zip(last.data.iter().copied()) {
                *dst *= src;
            }
        }
        self.masks.push(mask);
    }

    pub(crate) fn push_clip_rect(&mut self, rect: Box2D) {
        // Box2D::to_u32 panics if the point isn't in bounds for u32. We don't care
        // about truncating here, so we just do the cast directly.
        let mut rect = geom::Box2D::new(
            geom::Point::new(rect.min.x as u32, rect.min.y as u32),
            geom::Point::new(rect.max.x as u32, rect.max.y as u32),
        );
        rect.min = rect.min.min((self.width, self.height).into());
        rect.max = rect.max.min((self.width, self.height).into());

        let mut mask = self.old_masks.pop().unwrap_or_default();
        mask.re_use(self.width, self.height);

        for y in rect.min.y..rect.max.y {
            let row = mask.row_slice_mut(y as usize);
            let row = &mut row[rect.min.x as usize..rect.max.x as usize];
            row.fill(1.);
        }
        self.masks.push(mask);
    }

    pub(crate) fn pop_clip(&mut self) {
        if let Some(mask) = self.masks.pop() {
            self.old_masks.push(mask);
        } else {
            warn!("pop_clip called without an active mask");
        }
    }
}

fn build_gradient_lut(stops: &[GradientStop]) -> Lut {
    // This function lerps using **linear** rgb values.
    // https://learn.microsoft.com/en-us/typography/opentype/spec/cpal#interpolation-of-colors
    let mut lut = [Rgba::default(); 256];

    if stops.is_empty() {
        // There's nothing sane to do here, so just create a table that always returns
        // transparent.
        return lut;
    }

    let first = stops.first().unwrap();
    let last = stops.last().unwrap();

    // We assume our stops are aligned with 0..1
    debug_assert!(first.offset >= 0.0 && first.offset <= 1.);
    debug_assert!(last.offset >= 0.0 && last.offset <= 1.);

    // Pad the start of the lookup table with the first color. This is inclusive of
    // the first offset, because our windows below use the first element in the
    // window to check the previously set color, so we want to advance one into to
    // list of stops.
    for lut in lut.iter_mut().take((first.offset * 255.) as u8 as usize) {
        *lut = Rgba::from_linear(first.color);
    }

    for prev_cur in stops.windows(2) {
        let prev = &prev_cur[0];
        let cur = &prev_cur[1];

        if prev.offset == cur.offset {
            // https://learn.microsoft.com/en-us/typography/opentype/spec/colr#color-lines
            // "If there are multiple color stops defined for the same stop
            // offset, the first one given in the font must be used for
            // computing color values on the color line below that stop offset,
            // and the last one must be used for computing color values at or
            // above that stop offset"
            let idx = (prev.offset * 255.) as u8 as usize;
            lut[idx] = Rgba::from_linear(cur.color);
            continue;
        }

        let prev_idx = (prev.offset * 255.) as u8;
        let cur_idx = (cur.offset * 255.) as u8;

        if cur_idx == 255 {
            // We ignore idx == 255, because we'll always overwrite the last
            // slot in the table during the last step of building the lut.
            break;
        }

        // We assume that the input stops are in sorted order, so check that here.
        debug_assert!(prev_idx <= cur_idx, "Input stops not sorted");

        if prev_idx == cur_idx {
            // The stops are so close together they share a slot
            if prev_idx == 0 {
                // We have an exception where we always want the first color in
                // the table to be the first stop.
            } else {
                // The lut contains our previous color, we lerp between it and our current color
                // ratio of their distances.
                lut[prev_idx as usize] = Rgba::from_linear(
                    lut[prev_idx as usize]
                        .into_linear()
                        .mix(cur.color, 1.0 - (prev.offset / cur.offset)),
                );
            }
        } else {
            // Fill the lookup table values between prev and cur with the lerp'd value
            // This represents the fractional distance for each step prev..cur
            let fract = 1. / (cur_idx - prev_idx) as f32;
            let mut current_fract = fract;
            for idx in (prev_idx + 1)..cur_idx {
                lut[idx as usize] = Rgba::from_linear(prev.color.mix(cur.color, current_fract));
                current_fract += fract;
            }
            lut[cur_idx as usize] = Rgba::from_linear(cur.color);
        }
    }

    // Pad the remainder of the table using the last color
    for lut in lut.iter_mut().skip((last.offset * 255.) as u8 as usize) {
        *lut = Rgba::from_linear(last.color);
    }

    lut
}

fn fill_with_image(
    src: &Image<'_>,
    samples: &[Point],
    sample_fn: fn(Point, u32, u32) -> Point,
    blend_fn: fn(Rgba, Rgba) -> Rgba,
    mask: &ClipMask,
    dst: Image<'_>,
) {
    for ((dst, point), mask) in dst
        .data
        .iter_mut()
        .zip(samples.iter().copied())
        .zip(mask.data.iter().copied())
    {
        let mut sample = src.get(sample_fn(point, src.width, src.height));
        sample.alpha *= mask;
        *dst = blend_fn(*dst, sample);
    }
}

fn fill_with_color(color: Rgba, blend_fn: fn(Rgba, Rgba) -> Rgba, mask: &ClipMask, dst: Image<'_>) {
    for (dst, mask) in dst.data.iter_mut().zip(mask.data.iter().copied()) {
        let mut color = color;
        color.alpha *= mask;
        *dst = blend_fn(*dst, color);
    }
}

fn fill_with_linear_gradient(
    stops: &[GradientStop],
    samples: &[Point],
    mode: SampleMode,
    blend_fn: fn(Rgba, Rgba) -> Rgba,
    mask: &ClipMask,
    dst: Image<'_>,
) {
    let lut = build_gradient_lut(stops);
    let sampler = mode.sample_fn();

    for ((dst, mask), point) in dst
        .data
        .iter_mut()
        .zip(mask.data.iter().copied())
        .zip(samples.iter().copied())
    {
        // Remember that when we created our transform for the gradient shader, we
        // applied a rotation so that the gradient is always aligned along the x axis.
        let offset = sampler(point, 256, 1).x;
        let mut color = lut[offset as u8 as usize];
        color.alpha *= mask;
        *dst = blend_fn(*dst, color);
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_with_conical_gradient(
    stops: &[GradientStop],
    points: &[Point],
    mode: SampleMode,
    blend_fn: fn(Rgba, Rgba) -> Rgba,
    mask: &ClipMask,
    c0: Point,
    r0: f32,
    c1: Point,
    r1: f32,
    dst: Image<'_>,
) {
    // See pixman's implementation of radial gradients for an explanation of the
    // math behind this function. It's fairly complex, but their comment does a
    // good job of breaking down the derivation from the original formula, with an
    // appropriate reference to the specification it's drawn from. It would be
    // redundant to repeat it here.
    // https://gitlab.freedesktop.org/pixman/pixman/-/blob/0cb4fbe3241988a5d6b51d3551998ec6837c75a1/pixman/pixman-radial-gradient.c
    let lut = build_gradient_lut(stops);

    let sample_fn = mode.sample_fn();

    for ((dst, mask), mut point) in dst
        .data
        .iter_mut()
        .zip(mask.data.iter().copied())
        .zip(points.iter().copied())
    {
        // Reference point is the center of the pixel
        point += Vector::splat(0.5);

        // The rest of this follows the math from the derivation in the comment.
        let delta_circles = c1 - c0;
        let delta_radi = r1 - r0;
        let min_delta_radi = -1. * r0;

        let delta_point = point - c0;

        let a = delta_circles.x.powi(2) + delta_circles.y.powi(2) - delta_radi.powi(2);
        let b = delta_point.x * delta_circles.x + delta_point.y * delta_circles.y + r0 * delta_radi;
        let c = delta_point.x.powi(2) + delta_point.y.powi(2) - r0.powi(2);

        let mut color =
            if let Some(point) = get_conical_sample_point(a, b, c, delta_radi, min_delta_radi) {
                lut[sample_fn(Point::new(point, 0.), 256, 1).x as u8 as usize]
            } else {
                Rgba::default()
            };
        color.alpha *= mask;
        *dst = blend_fn(*dst, color);
    }
}

fn get_conical_sample_point(
    a: f32,
    b: f32,
    c: f32,
    delta_radi: f32,
    min_delta_radi: f32,
) -> Option<f32> {
    if a == 0. {
        if b == 0. {
            return None;
        }

        let t = 0.5 * c / b;
        if t * delta_radi >= min_delta_radi {
            return Some(t);
        }

        return None;
    }

    let discr = b * b + a * -c;
    if discr >= 0. {
        let sqrt_discr = discr.sqrt();
        let t0 = (b + sqrt_discr) / a;
        let t1 = (b - sqrt_discr) / a;

        if t0 * delta_radi >= min_delta_radi {
            return Some(t0);
        }

        if t1 * delta_radi >= min_delta_radi {
            return Some(t1);
        }
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn fill_with_sweep_gradient(
    stops: &[GradientStop],
    points: &[Point],
    mode: SampleMode,
    blend_fn: fn(Rgba, Rgba) -> Rgba,
    mask: &ClipMask,
    start: Angle,
    end: Angle,
    dst: Image<'_>,
) {
    let lut = build_gradient_lut(stops);
    let sample_fn = mode.sample_fn();

    let start = start.positive().radians;
    let end = end.positive().radians;

    let offset = -start;

    let start = start / (2. * PI);
    let end = end / (2. * PI);
    let scale = 1. / (end - start);

    for ((dst, mask), point) in dst
        .data
        .iter_mut()
        .zip(mask.data.iter().copied())
        .zip(points.iter().copied())
    {
        let angle = point.to_vector().angle_from_x_axis().positive();

        let t = (angle.radians * scale) + offset;
        let mut color = lut[sample_fn(Point::new(t, 0.), 256, 1).x as u8 as usize];
        color.alpha *= mask;
        *dst = blend_fn(*dst, color);
    }
}

#[cfg(test)]
mod tests {
    use lyon_path::{
        math::{
            Box2D,
            Point,
            Transform,
            Vector,
        },
        Path,
    };
    use palette::Srgba;

    use crate::{
        graphics::{
            Canvas,
            Image,
        },
        utils::Outline,
        Font,
    };

    #[test]
    fn clip_box() {
        let mut canvas = Canvas::new(5, 5);
        canvas.push_clip_rect(Box2D::new(Point::new(1., 1.), Point::new(4., 4.)));

        assert_eq!(
            canvas.masks.last().unwrap().data,
            vec![
                0., 0., 0., 0., 0., //
                0., 1., 1., 1., 0., //
                0., 1., 1., 1., 0., //
                0., 1., 1., 1., 0., //
                0., 0., 0., 0., 0., //
            ]
        )
    }

    #[test]
    fn clip_line() {
        let mut canvas = Canvas::new(5, 5);
        let mut path = Path::builder();
        path.add_rectangle(
            &Box2D::new(Point::new(1., 1.), Point::new(2., 4.)),
            lyon_path::Winding::Positive,
        );
        canvas.push_clip(&path.build());

        assert_eq!(
            canvas.masks.last().unwrap().data,
            vec![
                0., 0., 0., 0., 0., //
                0., 1., 0., 0., 0., //
                0., 1., 0., 0., 0., //
                0., 1., 0., 0., 0., //
                0., 0., 0., 0., 0., //
            ]
        )
    }

    #[test]
    fn clip_line_horizontal() {
        let mut canvas = Canvas::new(5, 5);
        let mut path = Path::builder();
        path.add_rectangle(
            &Box2D::new(Point::new(1., 1.), Point::new(4., 2.)),
            lyon_path::Winding::Positive,
        );
        canvas.push_clip(&path.build());

        assert_eq!(
            canvas.masks.last().unwrap().data,
            vec![
                0., 0., 0., 0., 0., //
                0., 1., 1., 1., 0., //
                0., 0., 0., 0., 0., //
                0., 0., 0., 0., 0., //
                0., 0., 0., 0., 0., //
            ]
        )
    }

    #[test]
    fn clip_line_2seg_horizontal() {
        let mut canvas = Canvas::new(5, 5);
        let mut path = Path::builder();
        path.begin(Point::splat(1.));
        path.line_to(Point::new(3., 1.));
        path.line_to(Point::new(4., 1.));
        path.line_to(Point::new(4., 2.));
        path.line_to(Point::new(1., 2.));
        path.close();
        canvas.push_clip(&path.build());

        assert_eq!(
            canvas.masks.last().unwrap().data,
            vec![
                0., 0., 0., 0., 0., //
                0., 1., 1., 1., 0., //
                0., 0., 0., 0., 0., //
                0., 0., 0., 0., 0., //
                0., 0., 0., 0., 0., //
            ]
        )
    }

    #[test]
    fn clip_circle() {
        let mut canvas = Canvas::new(32, 32);
        let mut path = Path::builder();
        path.add_circle(Point::splat(16.), 8., lyon_path::Winding::Positive);
        canvas.push_clip(&path.build());

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_circle.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            // This just gets rid of fp rounding issues.
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_2() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/CascadiaMono-Regular.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('2').expect("No glyph for 2");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for 2");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(0., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_2.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_0_l() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/CascadiaMono-Regular.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('0').expect("No glyph for 0");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for 0");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(-8., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_0_l.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_0_r() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/CascadiaMono-Regular.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('0').expect("No glyph for 0");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for 0");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(24., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_0_r.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_dash() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/CascadiaMono-Regular.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('─').expect("No glyph for ─");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for ─");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(0., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_dash.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_w() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/CascadiaMono-Regular.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('w').expect("No glyph for w");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for w");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(0., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_w.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }

    #[test]
    fn clip_wide_h() {
        let mut canvas = Canvas::new(32, 32);
        let font = Font::new(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/backend/fonts/Fairfax.ttf"
        )))
        .expect("Invalid font file");
        let mut outline = Outline::default();

        const HEIGHT: f32 = 24.;

        let glyph = font.font().glyph_index('Ｈ').expect("No glyph for Ｈ");
        font.font()
            .outline_glyph(glyph, &mut outline)
            .expect("Invalid glyph for Ｈ");

        let scale = HEIGHT / font.font().height() as f32;
        let transform = Transform::scale(scale, -scale)
            .then_translate(Vector::new(0., font.font().ascender() as f32 * scale));
        let path = outline.finish().transformed(&transform);

        canvas.push_clip(&path);

        let mask = canvas.masks.last().unwrap();
        let (data, width, height) =
            Image::load_from_memory(include_bytes!("goldens/clip_wide_h.png")).unwrap();

        assert_eq!(mask.width, width);
        assert_eq!(mask.height, height);
        for (m, d) in mask.to_color().iter().zip(data.iter()) {
            assert_eq!(Srgba::<u8>::from_format(*m), Srgba::<u8>::from_format(*d));
        }
    }
}
