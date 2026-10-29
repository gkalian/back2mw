use super::*;
use tes4_read::ExternalEdge;

fn grid(world: usize, cell: (i32, i32), points: &[[f32; 3]], edges: &[&[u16]]) -> SourcePathGrid {
    SourcePathGrid {
        world,
        cell,
        grid: SourceGrid {
            points: points.to_vec(),
            edges: edges.iter().map(|e| e.to_vec()).collect(),
            external: Vec::new(),
        },
    }
}

fn link(point: u16, target: [f32; 3]) -> ExternalEdge {
    ExternalEdge { point, target }
}

/// The grid of Morrowind cell `target` from those `grids` whose cells fall into it — as in
/// `convert_exterior`.
fn exterior(
    target: (i32, i32),
    grids: &[SourcePathGrid],
    report: &mut PathGridReport,
) -> Option<PathGraph> {
    let links = resolve_links(grids);
    let members: Vec<usize> = (0..grids.len())
        .filter(|&i| {
            let (x, y) = grids[i].cell;
            (x.div_euclid(2), y.div_euclid(2)) == target
        })
        .collect();
    exterior_graph(target, &members, grids, &links, report).unwrap()
}

#[test]
fn four_source_grids_merge_into_local_coordinates() {
    // Target cell (1, -1): world X 8192…16384, Y −8192…0. The south-western source is (2, −2), the
    // north-eastern (3, −1).
    let sw = grid(
        0,
        (2, -2),
        &[[8200.0, -8100.0, 5.0], [8300.4, -8000.6, -7.5]],
        &[&[1], &[0]],
    );
    let ne = grid(0, (3, -1), &[[16000.0, -10.0, 100.0]], &[&[]]);
    let mut report = PathGridReport::default();
    let g = exterior((1, -1), &[sw, ne], &mut report).unwrap();
    assert_eq!(g.points, [[8, 92, 5], [108, 191, -8], [7808, 8182, 100]]);
    assert_eq!(g.edges, [vec![1], vec![0], vec![]]);
    assert_eq!(
        (report.grids, report.points, report.points_outside_cell),
        (1, 3, 0)
    );
}

#[test]
fn links_inside_the_block_become_edges_and_outside_ones_are_lost() {
    let mut west = grid(
        0,
        (0, 0),
        &[[4000.0, 100.0, 0.0], [100.0, 100.0, 0.0]],
        &[&[], &[]],
    );
    west.grid.external = vec![
        // Into the eastern neighbor in the same Morrowind cell — a point 0.5 units from the
        // declared one.
        link(0, [4200.0, 100.5, 0.0]),
        // Into the Morrowind cell to the west — lost.
        link(1, [-50.0, 100.0, 0.0]),
        // A point exactly on the edge x = 0 belongs to the western neighbor: by coordinate it would
        // be assigned to this cell, but it is another grid — the link is lost.
        link(1, [0.0, 200.0, 0.0]),
        // Into the same cell, but there is no point there.
        link(1, [6000.0, 6000.0, 0.0]),
    ];
    let east = grid(0, (1, 0), &[[4200.0, 100.0, 0.0]], &[&[]]);
    let far_west = grid(
        0,
        (-1, 0),
        &[[-50.0, 100.0, 0.0], [0.0, 200.0, 0.0]],
        &[&[], &[]],
    );
    let mut report = PathGridReport::default();
    let g = exterior((0, 0), &[west, east, far_west], &mut report).unwrap();
    // The link is written at both ends, although in the source it was only at the western point.
    assert_eq!(g.edges, [vec![2], vec![], vec![0]]);
    assert_eq!(
        (
            report.links_joined,
            report.links_outside,
            report.links_unmatched
        ),
        (1, 2, 1)
    );
}

#[test]
fn links_do_not_cross_into_another_worldspace() {
    // The city (world 1) stands at the same coordinates as the island's point, but the island's
    // link leads only to island points.
    let mut island = grid(0, (0, 0), &[[4000.0, 100.0, 0.0]], &[&[]]);
    island.grid.external = vec![link(0, [4200.0, 100.0, 0.0])];
    let city = grid(1, (1, 0), &[[4200.0, 100.0, 0.0]], &[&[]]);
    let mut report = PathGridReport::default();
    let g = exterior((0, 0), &[island, city], &mut report).unwrap();
    assert_eq!(g.edges, [vec![], vec![]]);
    assert_eq!((report.links_joined, report.links_unmatched), (0, 1));
}

#[test]
fn edges_are_symmetric_sorted_and_without_repeats_or_loops() {
    let g = grid(
        0,
        (0, 0),
        &[[0.0; 3], [1.0; 3], [2.0; 3]],
        &[&[2, 1, 1, 0], &[], &[0]],
    );
    let mut report = PathGridReport::default();
    let out = interior_graph(&g.grid, &mut report).unwrap().unwrap();
    assert_eq!(out.edges, [vec![1, 2], vec![0], vec![0]]);
}

#[test]
fn neighbours_over_the_limit_are_cut_and_counted() {
    let n = MAX_NEIGHBOURS + 3;
    let points = vec![[0.0f32; 3]; n];
    let hub: Vec<u16> = (1..u16::try_from(n).unwrap()).collect();
    let mut edges: Vec<&[u16]> = vec![&hub];
    edges.extend(std::iter::repeat_n(&[][..], n - 1));
    let g = grid(0, (0, 0), &points, &edges);
    let mut report = PathGridReport::default();
    let out = interior_graph(&g.grid, &mut report).unwrap().unwrap();
    assert_eq!(out.edges[0].len(), MAX_NEIGHBOURS);
    assert_eq!(report.neighbours_cut, 2);
}

#[test]
fn interior_keeps_coordinates_and_drops_links() {
    let mut g = grid(0, (0, 0), &[[-1500.4, 20_000.6, -3.0]], &[&[]]);
    g.grid.external = vec![link(0, [0.0; 3])];
    let mut report = PathGridReport::default();
    let out = interior_graph(&g.grid, &mut report).unwrap().unwrap();
    assert_eq!(out.points, [[-1500, 20_001, -3]]);
    assert_eq!(report.links_outside, 1);
}

#[test]
fn points_beyond_their_cell_are_counted_not_moved() {
    let g = grid(
        0,
        (0, 0),
        &[[-10.0, 50.0, 0.0], [8200.0, 50.0, 0.0]],
        &[&[], &[]],
    );
    let mut report = PathGridReport::default();
    let out = exterior((0, 0), &[g], &mut report).unwrap();
    assert_eq!(out.points[0][0], -10);
    assert_eq!(report.points_outside_cell, 2);
}

#[test]
fn empty_grid_gives_no_record_and_huge_one_is_an_error() {
    let mut report = PathGridReport::default();
    let empty = grid(0, (0, 0), &[], &[]);
    assert_eq!(interior_graph(&empty.grid, &mut report), Ok(None));
    let huge = grid(0, (0, 0), &vec![[0.0; 3]; 70_000], &vec![&[][..]; 70_000]);
    assert_eq!(
        interior_graph(&huge.grid, &mut report),
        Err(TooManyPoints(70_000))
    );
    assert_eq!(report.grids, 0);
}
