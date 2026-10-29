//! The area map bundle codec, on a synthetic bundle.

mod support;

use rebof3_emi::area::map::{Cell, ListRecord, MapBundle, MapCell, MapObject, TileQuad, TileRect};

/// A bundle of two by two cells with one tile of each kind.
fn bundle() -> MapBundle {
    let mut rects: [Vec<TileRect>; 4] = Default::default();
    rects[0].push(TileRect { u: 1, v: 2, du: 3, dv: 4 });
    let mut quads: [Vec<TileQuad>; 4] = Default::default();
    quads[1].push(TileQuad { uvs: [[1, 2], [3, 4], [5, 6], [7, 8]] });
    MapBundle {
        width: 2,
        height: 2,
        colour: [0x1234, 0x5678],
        scroll_seat: 9,
        cells: vec![
            Cell { heights: [1, 2, 3, 4] },
            Cell { heights: [-1, -2, -3, -4] },
            Cell { heights: [0, 0, 0, 0] },
            Cell { heights: [5, 6, 7, 8] },
        ],
        codes: vec![1, 2, 3, 4],
        grid: vec![0, 1, 2, 3],
        records: vec![0x0000_0001, 0x0000_0002],
        rects,
        quads,
        list_a: vec![ListRecord { opcode: 1, head: [2, 3], operands: vec![4] }],
        list_b: vec![ListRecord { opcode: 5, head: [6, 7], operands: vec![] }],
        objects: vec![MapObject { tag: 0x1234, words: vec![0x5678] }],
        cells_24: vec![MapCell { y: 1, x: 2, records: vec![ListRecord { opcode: 9, head: [0, 0], operands: vec![] }] }],
        heights: Some(vec![1, 2, 3, 4]),
        reserved: [0, 0, 0, 0, 0],
    }
}

#[test]
fn a_map_bundle_writes_its_payload_and_reads_it_back() {
    let bundle = bundle();
    let bytes = bundle.encode();
    assert_eq!(bytes.len(), bundle.encoded_len());

    let read = MapBundle::decode(&bytes).expect("the bundle reads back");
    assert_eq!(read.width, 2);
    assert_eq!(read.height, 2);
    assert_eq!(read.colour, [0x1234, 0x5678]);
    assert_eq!(read.scroll_seat, 9);
    assert_eq!(read.cells, bundle.cells);
    assert_eq!(read.codes, bundle.codes);
    assert_eq!(read.grid, bundle.grid);
    assert_eq!(read.records, bundle.records);
    assert_eq!(read.rects, bundle.rects);
    assert_eq!(read.quads, bundle.quads);
    assert_eq!(read.list_a, bundle.list_a);
    assert_eq!(read.list_b, bundle.list_b);
    assert_eq!(read.objects, bundle.objects);
    assert_eq!(read.cells_24, bundle.cells_24);
    assert_eq!(read.heights, bundle.heights);
    assert_eq!(read.encode(), bytes, "a decode and encode cycle is byte-exact");
    let out = support::out_dir("a_map_bundle_writes_its_payload_and_reads_it_back");
    support::write(&out, "bundle.map", &bytes);
}
