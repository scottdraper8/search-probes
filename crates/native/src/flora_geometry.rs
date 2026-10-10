//! Canonical surface samples from the game's discoverable flora meshes.
//!
//! Samples preserve the correlation between UVs and vertex colour. The bounded,
//! deterministic area sampling estimates the authored surface in local mesh
//! space; it does not estimate camera-dependent coverage or scene lighting.
use crate::bioluminescence::{Assets as Source, Mbin};
use std::{collections::BTreeMap, path::Path, sync::Arc};

type Result<T> = std::result::Result<T, &'static str>;
const SAMPLE_COUNT: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub struct VertexSample {
    pub uv: [f64; 4],
    pub colour: [f64; 4],
    pub weight: f64,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub material: String,
    pub samples: Arc<[VertexSample]>,
}

pub struct Assets {
    source: Source,
}

fn path(name: &str) -> Result<String> {
    let name = name.replace('\\', "/").to_ascii_lowercase();
    if name.len() > 4096 || name.starts_with('/') || name.split('/').any(|part| part == "..") {
        return Err("invalid flora geometry path");
    }
    Ok(name)
}

#[derive(Default)]
struct Draw {
    material: String,
    hash: u64,
    start: usize,
    count: usize,
    lod: usize,
}

impl Assets {
    pub fn new(root: &Path) -> Self {
        Self {
            source: Source::new(root),
        }
    }

    pub fn meshes(&mut self, model: &str) -> Result<Vec<Mesh>> {
        let model = path(model)?;
        self.scene(&model, 0, &mut BTreeMap::new())
    }

    fn scene(
        &mut self,
        name: &str,
        depth: usize,
        cache: &mut BTreeMap<String, Vec<Mesh>>,
    ) -> Result<Vec<Mesh>> {
        if let Some(meshes) = cache.get(name) {
            return Ok(meshes.clone());
        }
        if cache.len() >= 1024 {
            return Err("flora scene reference count exceeds bound");
        }
        if depth > 32 {
            return Err("flora geometry scene reference depth exceeded");
        }
        let bytes = self
            .source
            .read("NMSARC.EntitySceneMBIN.pak", name, 16 * 1024 * 1024)?;
        let mbin = Mbin(&bytes);
        mbin.header(0x3db87e47)?;
        let mut geometry = None;
        let mut entities = Vec::new();
        let mut references = Vec::new();
        let mut draws = Vec::new();
        let mut nodes = vec![(0x20, 0)];
        let mut visited = 0;
        while let Some((node, level)) = nodes.pop() {
            visited += 1;
            if visited > 65536 || level > 128 {
                return Err("flora geometry scene node bound exceeded");
            }
            let mesh = mbin.bytes(node + 0x40, 16)?.starts_with(b"MESH\0");
            let mut draw = Draw::default();
            for attr in mbin.array(node, 0x20, 4096)?.step_by(0x20) {
                let key = mbin.bytes(attr, 16)?;
                let key = &key[..key.iter().position(|&byte| byte == 0).unwrap_or(16)];
                let value = mbin.string(attr + 0x10)?;
                if value.is_empty() {
                    continue;
                }
                match key {
                    b"GEOMETRY" if node == 0x20 => geometry = Some(path(value)?),
                    b"ATTACHMENT" => entities.push(path(value)?),
                    b"SCENEGRAPH" => references.push(path(value)?),
                    b"MATERIAL" if mesh => draw.material = path(value)?,
                    b"HASH" if mesh => {
                        draw.hash = value.parse().map_err(|_| "invalid flora mesh hash")?
                    }
                    b"BATCHSTARTGRAPH" if mesh => {
                        draw.start = value
                            .parse()
                            .map_err(|_| "invalid flora mesh index start")?
                    }
                    b"BATCHCOUNT" if mesh => {
                        draw.count = value
                            .parse()
                            .map_err(|_| "invalid flora mesh index count")?
                    }
                    b"LODLEVEL" if mesh => {
                        draw.lod = value
                            .parse()
                            .map_err(|_| "invalid flora mesh detail level")?
                    }
                    _ => {}
                }
            }
            if mesh && !draw.material.is_empty() && draw.count > 0 {
                draws.push(draw);
            }
            nodes.extend(
                mbin.array(node + 0x10, 0x80, 65536)?
                    .step_by(0x80)
                    .map(|node| (node, level + 1)),
            );
        }
        let mut discoverable = false;
        for entity in entities {
            discoverable |= self.source.flora(&entity)?;
        }
        let mut meshes = Vec::new();
        if discoverable && !draws.is_empty() {
            let geometry = geometry.ok_or("missing flora scene geometry")?;
            let filename = format!("{geometry}.pc");
            let bytes =
                self.source
                    .read("NMSARC.MeshPlanetBIOMES.pak", &filename, 16 * 1024 * 1024)?;
            let data_name = geometry
                .strip_suffix(".geometry.mbin")
                .ok_or("unsupported flora geometry filename")?;
            let data = self.source.read(
                "NMSARC.MeshPlanetBIOMES.pak",
                &format!("{data_name}.geometry.data.mbin.pc"),
                64 * 1024 * 1024,
            )?;
            let layouts = Geometry::parse(&bytes, &data)?;
            let lod = draws.iter().map(|draw| draw.lod).min().unwrap();
            for draw in draws.into_iter().filter(|draw| draw.lod == lod) {
                let samples = layouts.samples(&draw)?;
                if !samples.is_empty() {
                    meshes.push(Mesh {
                        material: draw.material,
                        samples: samples.into(),
                    });
                }
            }
        }
        // Discovery belongs to the referenced scene itself. Inheriting it would
        // incorrectly classify attached effects, such as a hive's bees, as flora.
        for reference in references {
            meshes.extend(self.scene(&reference, depth + 1, cache)?);
        }
        cache.insert(name.to_owned(), meshes.clone());
        Ok(meshes)
    }
}

struct Layout {
    stride: usize,
    position: Option<usize>,
    uv: Option<usize>,
    colour: Option<usize>,
}
impl Layout {
    fn parse(mbin: &Mbin<'_>, base: usize) -> Result<Self> {
        let stride = mbin.word(base + 0x1c)? as usize;
        if stride == 0 || stride > 256 {
            return Err("invalid flora vertex stride");
        }
        let mut layout = Self {
            stride,
            position: None,
            uv: None,
            colour: None,
        };
        for element in mbin.array(base, 12, 32)?.step_by(12) {
            if mbin.word(element)? != 0 {
                return Err("instanced flora vertex layout is unsupported");
            }
            let kind = mbin.word(element + 4)?;
            let fields = mbin.bytes(element + 8, 4)?;
            let offset = fields[1] as usize;
            let semantic = fields[2];
            let size = fields[3] as usize;
            let bytes = match semantic {
                0 | 1 if kind == 5131 && size == 4 => 8,
                4 if kind == 5121 && size == 4 => 4,
                11 if kind == 5125 && size == 1 => 4,
                _ => return Err("unsupported flora vertex element"),
            };
            if offset + bytes > stride {
                return Err("flora vertex element outside stride");
            }
            let target = match semantic {
                0 => Some(&mut layout.position),
                1 => Some(&mut layout.uv),
                4 => Some(&mut layout.colour),
                _ => None,
            };
            if let Some(target) = target {
                if target.replace(offset).is_some() {
                    return Err("duplicate flora vertex element");
                }
            }
        }
        Ok(layout)
    }
}

struct Stream<'a> {
    vertices: &'a [u8],
    positions: &'a [u8],
    indices: &'a [u8],
    index_size: usize,
}
struct Geometry<'a> {
    positions: Layout,
    vertices: Layout,
    streams: BTreeMap<u64, Stream<'a>>,
}
impl<'a> Geometry<'a> {
    fn parse(bytes: &[u8], data: &'a [u8]) -> Result<Self> {
        let mbin = Mbin(bytes);
        let stream = Mbin(data);
        if mbin.bytes(0, 8)? != [0xdd; 8]
            || mbin.word(12)? != 0x819c3220
            || stream.bytes(0, 8)? != [0xdd; 8]
            || stream.word(12)? != 0x40025754
        {
            return Err("unexpected flora geometry template");
        }
        let positions = Layout::parse(&mbin, 0x20)?;
        let vertices = Layout::parse(&mbin, 0x40)?;
        if positions.position.is_none() || positions.uv.is_none() {
            return Err("missing flora position or texture coordinates");
        }
        let mut streams = BTreeMap::new();
        for record in stream.array(0x20, 0x48, 65536)?.step_by(0x48) {
            let bytes = &data[stream.array(record + 0x10, 1, 64 * 1024 * 1024)?];
            let position_bytes = &data[stream.array(record + 0x20, 1, 64 * 1024 * 1024)?];
            let index_size = stream.word(record + 0x38)? as usize;
            let vertex_size = stream.word(record + 0x3c)? as usize;
            let position_size = stream.word(record + 0x40)? as usize;
            if vertex_size.checked_add(index_size) != Some(bytes.len())
                || position_bytes.len() != position_size
                || vertex_size % vertices.stride != 0
                || position_size % positions.stride != 0
                || vertex_size / vertices.stride != position_size / positions.stride
                || index_size
                    % if vertex_size / vertices.stride <= 65536 {
                        2
                    } else {
                        4
                    }
                    != 0
            {
                return Err("invalid flora mesh stream dimensions");
            }
            let hash = u64::from_le_bytes(stream.bytes(record + 0x30, 8)?.try_into().unwrap());
            if streams
                .insert(
                    hash,
                    Stream {
                        vertices: &bytes[..vertex_size],
                        positions: position_bytes,
                        indices: &bytes[vertex_size..],
                        index_size: if vertex_size / vertices.stride <= 65536 {
                            2
                        } else {
                            4
                        },
                    },
                )
                .is_some()
            {
                return Err("duplicate flora geometry stream hash");
            }
        }
        Ok(Self {
            positions,
            vertices,
            streams,
        })
    }

    fn samples(&self, draw: &Draw) -> Result<Vec<VertexSample>> {
        let stream = self
            .streams
            .get(&draw.hash)
            .ok_or("missing flora mesh stream")?;
        if draw.count % 3 != 0 {
            return Err("flora mesh is not a triangle list");
        }
        let start = draw
            .start
            .checked_mul(stream.index_size)
            .ok_or("flora draw extent overflow")?;
        let end = draw
            .start
            .checked_add(draw.count)
            .and_then(|end| end.checked_mul(stream.index_size))
            .ok_or("flora draw extent overflow")?;
        let indices = stream
            .indices
            .get(start..end)
            .ok_or("flora draw outside index stream")?;
        let mut triangles = Vec::new();
        let mut area = 0.;
        for triangle in indices.chunks_exact(3 * stream.index_size) {
            let mut points = [[0.; 3]; 3];
            let mut samples = [VertexSample {
                uv: [0.; 4],
                colour: [1.; 4],
                weight: 0.,
            }; 3];
            for i in 0..3 {
                let part = &triangle[i * stream.index_size..(i + 1) * stream.index_size];
                let index = if stream.index_size == 2 {
                    u16::from_le_bytes(part.try_into().unwrap()) as usize
                } else {
                    u32::from_le_bytes(part.try_into().unwrap()) as usize
                };
                let positions = stream
                    .positions
                    .get(index * self.positions.stride..(index + 1) * self.positions.stride)
                    .ok_or("flora vertex index outside position stream")?;
                let vertex = stream
                    .vertices
                    .get(index * self.vertices.stride..(index + 1) * self.vertices.stride)
                    .ok_or("flora vertex index outside colour stream")?;
                let p = half4(positions, self.positions.position.unwrap())?;
                points[i] = [p[0], p[1], p[2]];
                samples[i].uv = uv4(positions, self.positions.uv.unwrap())?;
                if let Some(offset) = self.vertices.colour {
                    samples[i].colour = std::array::from_fn(|c| vertex[offset + c] as f64 / 255.);
                }
            }
            let triangle_area = triangle_area(points);
            if !triangle_area.is_finite() {
                return Err("invalid flora triangle area");
            }
            if triangle_area > 0. {
                area += triangle_area;
                triangles.push((area, samples));
            }
        }
        if triangles.is_empty() {
            return Ok(Vec::new());
        }
        if !area.is_finite() {
            return Err("invalid flora mesh surface area");
        }
        Ok(sample_triangles(&triangles, area))
    }
}

fn half4(bytes: &[u8], offset: usize) -> Result<[f64; 4]> {
    let values = uv4(bytes, offset)?;
    if values.iter().any(|v| !v.is_finite()) {
        return Err("nonfinite flora vertex");
    }
    Ok(values)
}

fn uv4(bytes: &[u8], offset: usize) -> Result<[f64; 4]> {
    let mut values = [0.; 4];
    for (i, value) in values.iter_mut().enumerate() {
        let half = u16::from_le_bytes(
            bytes
                .get(offset + i * 2..offset + i * 2 + 2)
                .ok_or("truncated flora vertex")?
                .try_into()
                .unwrap(),
        );
        let sign = if half & 0x8000 != 0 { -1. } else { 1. };
        let exponent = (half >> 10) & 31;
        let mantissa = (half & 1023) as f64;
        *value = match exponent {
            0 => sign * mantissa * 2f64.powi(-24),
            31 if mantissa == 0. => sign * f64::INFINITY,
            31 => f64::NAN,
            _ => sign * (1. + mantissa / 1024.) * 2f64.powi(exponent as i32 - 15),
        };
    }
    if values[..2].iter().any(|v| !v.is_finite()) {
        return Err("nonfinite flora texture coordinates");
    }
    Ok(values)
}

fn triangle_area(p: [[f64; 3]; 3]) -> f64 {
    let a: [f64; 3] = std::array::from_fn(|i| p[1][i] - p[0][i]);
    let b: [f64; 3] = std::array::from_fn(|i| p[2][i] - p[0][i]);
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    cross.iter().map(|v| v * v).sum::<f64>().sqrt() * 0.5
}

fn radical_inverse(mut value: usize, base: usize) -> f64 {
    let mut result = 0.;
    let mut factor = 1. / base as f64;
    while value > 0 {
        result += (value % base) as f64 * factor;
        value /= base;
        factor /= base as f64;
    }
    result
}

fn sample_triangles(triangles: &[(f64, [VertexSample; 3])], area: f64) -> Vec<VertexSample> {
    let mut output = Vec::with_capacity(SAMPLE_COUNT);
    let mut triangle = 0;
    for i in 0..SAMPLE_COUNT {
        let target = (i as f64 + 0.5) * area / SAMPLE_COUNT as f64;
        while triangle + 1 < triangles.len() && triangles[triangle].0 < target {
            triangle += 1;
        }
        let sqrt = radical_inverse(i + 1, 2).sqrt();
        let second = radical_inverse(i + 1, 3);
        let barycentric = [1. - sqrt, sqrt * (1. - second), sqrt * second];
        let points = &triangles[triangle].1;
        output.push(VertexSample {
            uv: std::array::from_fn(|c| (0..3).map(|v| points[v].uv[c] * barycentric[v]).sum()),
            colour: std::array::from_fn(|c| {
                (0..3).map(|v| points[v].colour[c] * barycentric[v]).sum()
            }),
            weight: area / SAMPLE_COUNT as f64,
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn surface_samples_preserve_uv_colour_correlation_and_area() {
        let points = [
            VertexSample {
                uv: [0.; 4],
                colour: [0.; 4],
                weight: 0.,
            },
            VertexSample {
                uv: [1.; 4],
                colour: [1.; 4],
                weight: 0.,
            },
            VertexSample {
                uv: [0.; 4],
                colour: [0.; 4],
                weight: 0.,
            },
        ];
        let samples = sample_triangles(&[(2., points)], 2.);
        assert_eq!(samples.len(), SAMPLE_COUNT);
        assert!((samples.iter().map(|p| p.weight).sum::<f64>() - 2.).abs() < 1e-12);
        assert!(
            samples
                .iter()
                .all(|p| p.uv == p.colour && p.uv[0] >= 0. && p.uv[0] <= 1.)
        );
    }
    #[test]
    fn half_coordinates_retain_sign_subnormals_and_reject_nonfinite() {
        let bytes: Vec<u8> = [0x3c00u16, 0xbc00, 1, 0x0000]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(half4(&bytes, 0).unwrap(), [1., -1., 2f64.powi(-24), 0.]);
        assert_eq!(
            half4(&[0, 0x7c, 0, 0, 0, 0, 0, 0], 0),
            Err("nonfinite flora texture coordinates")
        );
    }

    #[test]
    fn unused_secondary_coordinates_preserve_authored_infinities() {
        let bytes: Vec<u8> = [0x3c00u16, 0x0000, 0xfc00, 0x7c00]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(
            uv4(&bytes, 0).unwrap(),
            [1., 0., f64::NEG_INFINITY, f64::INFINITY]
        );
        assert_eq!(half4(&bytes, 0), Err("nonfinite flora vertex"));
    }

    #[test]
    fn area_stratification_allocates_samples_to_surface_area() {
        let first = [VertexSample {
            uv: [0.; 4],
            colour: [1.; 4],
            weight: 0.,
        }; 3];
        let second = [VertexSample {
            uv: [1.; 4],
            colour: [1.; 4],
            weight: 0.,
        }; 3];
        let samples = sample_triangles(&[(1., first), (4., second)], 4.);
        assert_eq!(
            samples.iter().filter(|sample| sample.uv[0] < 0.5).count(),
            256
        );
        assert!((samples.iter().map(|sample| sample.weight).sum::<f64>() - 4.).abs() < 1e-12);
    }
}
