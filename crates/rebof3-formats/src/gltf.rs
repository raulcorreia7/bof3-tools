//! Binary glTF 2.0 (`.glb`) writer for triangle meshes.

use serde_json::{Value, json};

/// The triangles of a mesh that share one material.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Primitive {
    /// The index into the material names.
    pub material: usize,
    /// Vertex indices, three per triangle.
    pub indices: Vec<u32>,
}

/// A node without a mesh: a named place, what it records in `extras`, and its children.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub name: String,
    /// The node's place in the mesh's coordinates.
    pub translation: Option<[f32; 3]>,
    /// What the node records, as glTF `extras`; `Null` writes none.
    pub extras: Value,
    pub children: Vec<Node>,
}

/// `glTF`, the file magic.
const MAGIC: u32 = 0x4654_6C67;
const VERSION: u32 = 2;
/// `JSON` and `BIN\0`, the chunk types.
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
/// glTF component types and buffer targets.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;

/// Writes a `.glb` with one mesh named `name`.
pub fn glb(
    name: &str,
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    primitives: &[Primitive],
    materials: &[&str],
) -> Option<Vec<u8>> {
    glb_with(name, positions, uvs, None, primitives, materials, &[])
}

/// Writes a `.glb` like [`glb`], with an RGBA vertex colour per vertex (`COLOR_0`, floats 0 to 1) when `colours`
pub fn glb_with(
    name: &str,
    positions: &[[f32; 3]],
    uvs: &[[f32; 2]],
    colours: Option<&[[f32; 4]]>,
    primitives: &[Primitive],
    materials: &[&str],
    nodes: &[Node],
) -> Option<Vec<u8>> {
    if positions.is_empty() || primitives.iter().all(|primitive| primitive.indices.is_empty()) {
        return None;
    }
    let mut bin = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();

    let (min, max) = bounds(positions);
    let bytes = positions.iter().flatten().flat_map(|value| value.to_le_bytes()).collect::<Vec<u8>>();
    push_section(&mut bin, &mut views, &bytes, ARRAY_BUFFER);
    accessors.push(json!({
        "bufferView": 0, "componentType": FLOAT, "count": positions.len(), "type": "VEC3", "min": min, "max": max,
    }));

    let bytes = uvs.iter().flatten().flat_map(|value| value.to_le_bytes()).collect::<Vec<u8>>();
    push_section(&mut bin, &mut views, &bytes, ARRAY_BUFFER);
    accessors.push(json!({
        "bufferView": 1, "componentType": FLOAT, "count": uvs.len(), "type": "VEC2",
    }));

    let mut attributes = json!({"POSITION": 0, "TEXCOORD_0": 1});
    if let Some(colours) = colours {
        let bytes = colours.iter().flatten().flat_map(|value| value.to_le_bytes()).collect::<Vec<u8>>();
        push_section(&mut bin, &mut views, &bytes, ARRAY_BUFFER);
        attributes["COLOR_0"] = json!(accessors.len());
        accessors.push(json!({
            "bufferView": views.len() - 1, "componentType": FLOAT, "count": colours.len(), "type": "VEC4",
        }));
    }

    let mut draws = Vec::new();
    for primitive in primitives.iter().filter(|primitive| !primitive.indices.is_empty()) {
        let bytes = primitive.indices.iter().flat_map(|index| index.to_le_bytes()).collect::<Vec<u8>>();
        push_section(&mut bin, &mut views, &bytes, ELEMENT_ARRAY_BUFFER);
        let accessor = accessors.len();
        accessors.push(json!({
            "bufferView": views.len() - 1,
            "componentType": UNSIGNED_INT,
            "count": primitive.indices.len(),
            "type": "SCALAR",
        }));
        draws.push(json!({
            "attributes": attributes,
            "indices": accessor,
            "material": primitive.material,
        }));
    }

    let materials: Vec<Value> = materials
        .iter()
        .map(|material| {
            json!({
                "name": material,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 1.0,
                },
            })
        })
        .collect();
    let mut all = vec![json!({"name": name, "mesh": 0})];
    let roots: Vec<usize> = std::iter::once(0).chain(nodes.iter().map(|node| flatten(node, &mut all))).collect();
    let document = json!({
        "asset": {"version": "2.0", "generator": "rebof3-assets"},
        "scene": 0,
        "scenes": [{"nodes": roots}],
        "nodes": all,
        "meshes": [{"name": name, "primitives": draws}],
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}],
    });
    Some(pack(&document, &bin))
}

/// Appends `node` and its children to `all`, children after their parent, and returns its index.
fn flatten(node: &Node, all: &mut Vec<Value>) -> usize {
    let index = all.len();
    all.push(json!({"name": node.name}));
    if let Some(translation) = node.translation {
        all[index]["translation"] = json!(translation);
    }
    if !node.extras.is_null() {
        all[index]["extras"] = node.extras.clone();
    }
    let children: Vec<usize> = node.children.iter().map(|child| flatten(child, all)).collect();
    if !children.is_empty() {
        all[index]["children"] = json!(children);
    }
    index
}

/// Appends one section to the binary chunk and records its buffer view.
fn push_section(bin: &mut Vec<u8>, views: &mut Vec<Value>, bytes: &[u8], target: u32) {
    views.push(json!({
        "buffer": 0, "byteOffset": bin.len(), "byteLength": bytes.len(), "target": target,
    }));
    bin.extend_from_slice(bytes);
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
}

/// The per-axis minimum and maximum of the positions, which glTF requires for `POSITION`.
fn bounds(positions: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for position in positions {
        for (axis, value) in position.iter().enumerate() {
            min[axis] = min[axis].min(*value);
            max[axis] = max[axis].max(*value);
        }
    }
    (min, max)
}

/// Lays out the header, the JSON chunk (padded with spaces) and the binary chunk (padded with zeros).
fn pack(document: &Value, bin: &[u8]) -> Vec<u8> {
    let mut json = serde_json::to_vec(document).expect("a JSON value always serializes");
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
    out.extend_from_slice(bin);
    out
}

/// The POSITION vertices of a binary glTF.
pub fn read_positions(glb: &[u8]) -> Result<Vec<[f32; 3]>, String> {
    let (document, bin) = chunks(glb)?;
    let accessor = document["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
        .as_u64()
        .and_then(|index| document["accessors"].get(index as usize))
        .ok_or("the first primitive has no POSITION accessor")?;
    if accessor["componentType"].as_u64() != Some(u64::from(FLOAT)) || accessor["type"] != "VEC3" {
        return Err("the POSITION accessor is not VEC3 floats".into());
    }
    let count = accessor["count"].as_u64().ok_or("the POSITION accessor has no count")? as usize;
    let view = accessor["bufferView"]
        .as_u64()
        .and_then(|index| document["bufferViews"].get(index as usize))
        .ok_or("the POSITION accessor has no buffer view")?;
    let start = view["byteOffset"].as_u64().unwrap_or(0) + accessor["byteOffset"].as_u64().unwrap_or(0);
    let start = start as usize;
    let end = count.checked_mul(12).and_then(|bytes| start.checked_add(bytes));
    let data = end.and_then(|end| bin.get(start..end)).ok_or("the POSITION data runs past the binary chunk")?;
    Ok(data
        .as_chunks::<12>()
        .0
        .iter()
        .map(|vertex| {
            let float = |at: usize| f32::from_le_bytes([vertex[at], vertex[at + 1], vertex[at + 2], vertex[at + 3]]);
            [float(0), float(4), float(8)]
        })
        .collect())
}

/// A node's name and its translation, when it has one.
pub type NamedPlace = (String, Option<[f32; 3]>);

/// The named nodes of a binary glTF, in file order.
pub fn read_nodes(glb: &[u8]) -> Result<Vec<NamedPlace>, String> {
    let (document, _) = chunks(glb)?;
    let nodes = document["nodes"].as_array().ok_or("the glTF has no nodes")?;
    Ok(nodes
        .iter()
        .filter_map(|node| {
            let name = node["name"].as_str()?.to_string();
            let translation = node["translation"].as_array().and_then(|values| {
                let values: Vec<f32> =
                    values.iter().filter_map(|value| value.as_f64().map(|value| value as f32)).collect();
                <[f32; 3]>::try_from(values).ok()
            });
            Some((name, translation))
        })
        .collect())
}

/// The JSON document and the binary chunk of a `.glb`.
fn chunks(glb: &[u8]) -> Result<(Value, &[u8]), String> {
    let word =
        |at: usize| glb.get(at..at + 4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
    if word(0) != Some(MAGIC) || word(4) != Some(VERSION) || word(16) != Some(CHUNK_JSON) {
        return Err("the file is not a glTF 2.0 binary with its JSON chunk first".into());
    }
    let json_length = word(12).ok_or("the glTF JSON chunk has no length")? as usize;
    let json = glb.get(20..20 + json_length).ok_or("the glTF JSON chunk runs past the file")?;
    let bin_at = 20 + json_length;
    let bin_length = word(bin_at).ok_or("the glTF binary chunk is missing")? as usize;
    if word(bin_at + 4) != Some(CHUNK_BIN) {
        return Err("the glTF binary chunk is missing".into());
    }
    let bin = glb.get(bin_at + 8..bin_at + 8 + bin_length).ok_or("the glTF binary chunk runs past the file")?;

    let document: Value = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    Ok((document, bin))
}
