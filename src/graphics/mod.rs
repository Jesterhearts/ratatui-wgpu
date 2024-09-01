use std::f32::consts::PI;

use lyon_path::{
    geom::{
        CubicBezierSegment,
        LineSegment,
        QuadraticBezierSegment,
    },
    math::{
        Angle,
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

type Lut = [Rgba; 256];

pub(crate) struct Image<'d> {
    data: &'d mut [Rgba],
    width: u32,
    height: u32,
}

impl<'d> Image<'d> {
    fn new(data: &'d mut [Rgba], width: u32, height: u32) -> Self {
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
}

struct ClipMask {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

impl ClipMask {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0.; width as usize * height as usize],
        }
    }

    fn new_unmasked(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![1.; width as usize * height as usize],
        }
    }

    fn row_slice_mut(&mut self, y: usize) -> &mut [f32] {
        let y = (y * self.width as usize).min(self.data.len());
        &mut self.data[y..y + self.width as usize]
    }
}

#[derive(Debug, Default)]
struct Rasterizer {
    edges_by_y: Vec<Vec<LineSegment<f32>>>,
    current_edges: Vec<LineSegment<f32>>,

    height: usize,
}

impl Rasterizer {
    fn rasterize_path(&mut self, path: &Path, out: &mut ClipMask) {
        for control in path.iter() {
            match control {
                lyon_path::Event::Begin { .. } => {}
                lyon_path::Event::Line { from, to } => {
                    let y = from.y.min(to.y) as usize;
                    self.height = self.height.max(from.y.max(to.y) as usize);

                    self.edges_by_y.resize_with(self.height, Vec::new);
                    self.edges_by_y[y].push(LineSegment { from, to });
                }
                lyon_path::Event::Quadratic { from, ctrl, to } => {
                    let segment = QuadraticBezierSegment { from, ctrl, to };
                    let (_, maxy) = segment.fast_bounding_range_y();
                    self.edges_by_y.resize_with(maxy as usize, Vec::new);
                    segment.for_each_flattened(0.01, &mut |line| {
                        let y = line.from.y.min(line.to.y) as usize;
                        self.height = self.height.max(line.from.y.max(line.to.y) as usize);
                        self.edges_by_y[y].push(*line);
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
                    self.edges_by_y.resize_with(maxy as usize, Vec::new);

                    segment.for_each_flattened(0.01, &mut |line| {
                        let y = line.from.y.min(line.to.y) as usize;
                        self.height = self.height.max(line.from.y.max(line.to.y) as usize);
                        self.edges_by_y[y].push(*line);
                    })
                }
                lyon_path::Event::End { last, first, .. } => {
                    let y = last.y.min(first.y) as usize;
                    self.height = self.height.max(last.y.max(first.y) as usize);
                    self.edges_by_y.resize_with(self.height, Vec::new);
                    self.edges_by_y[y].push(LineSegment {
                        from: last,
                        to: first,
                    });
                }
            }
        }

        for y in 0..self.height {
            if y >= out.height as usize {
                break;
            }

            self.current_edges
                .retain(|edge| edge.from.y.max(edge.to.y) as usize >= y);
            self.current_edges.append(&mut self.edges_by_y[y]);
            self.current_edges
                .sort_unstable_by(|l, r| l.from.x.min(r.to.x).total_cmp(&r.from.x.min(r.to.x)));

            let mut winding = 0;
            let mut edges = self.current_edges.iter().copied().peekable();
            while let Some(edge) = edges.peek().copied() {
                if edge.from.x.max(edge.to.x) >= 0. {
                    break;
                }
                edges.next();

                if edge.from.y != edge.to.y {
                    winding += if edge.from.y < edge.to.y { 1 } else { -1 };
                }
            }

            for edge in edges {
                if winding != 0 {
                    let width = out.width;
                    let min_x = edge.from.x.min(edge.to.x).max(0.);
                    let max_x = edge.from.x.max(edge.to.x).min((width - 1) as f32);

                    if min_x as u32 >= width {
                        break;
                    }

                    let row = out.row_slice_mut(y);
                    let segment = &mut row[min_x as usize..=max_x as usize];
                    if segment.is_empty() {
                        continue;
                    }

                    if segment.len() == 1 {
                        segment[0] = (segment[0] + (max_x - min_x)).clamp(0., 1.);
                        continue;
                    }

                    segment[0] = (segment[0] + min_x.fract()).min(1.);
                    let last = segment.len() - 1;
                    segment[last] = (segment[last] + max_x.fract()).min(11.);

                    let min_y = edge.from.y.min(edge.to.y);
                    let max_y = edge.from.y.max(edge.to.y);
                    let y_cov = if min_y as usize == y {
                        min_y.fract()
                    } else if max_y as usize == y {
                        max_y.fract()
                    } else {
                        1.
                    };
                    for point in &mut segment[1..last] {
                        *point = (*point + y_cov).min(1.);
                    }
                }

                if edge.from.y != edge.to.y {
                    winding += if edge.from.y < edge.to.y { 1 } else { -1 };
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
    offset: f32,
    color: LinSrgba,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SampleMode {
    /// Keep sampling the max/min value when out of bounds.
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
                center,
                start,
                end,
                mode,
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
        let mut mask = ClipMask::new(self.width, self.height);

        self.rasterizer.rasterize_path(path, &mut mask);
        if let Some(last) = self.masks.last() {
            for (dst, src) in mask.data.iter_mut().zip(last.data.iter().copied()) {
                *dst *= src;
            }
        }
        self.masks.push(mask);
    }

    pub(crate) fn pop_clip(&mut self) {
        self.masks.pop();
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
