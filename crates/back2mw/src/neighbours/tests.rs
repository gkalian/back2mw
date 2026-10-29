use super::*;

#[test]
fn game_files_reads_only_its_section() {
    let ini = "[General]\nGameFile9=nope.esp\n\n[Game Files]\nGameFile0=Morrowind.esm\n\
               ; GameFile1=commented.esp\ngamefile2 = TR_Mainland.esm \nGameFile3=\n\
               [Archives]\nArchive 0=Tribunal.bsa\n";
    assert_eq!(game_files(ini), ["Morrowind.esm", "TR_Mainland.esm"]);
}

#[test]
fn ring_walks_the_square_once() {
    for r in 0..5 {
        let pts = ring((3, -2), r);
        let set: HashSet<_> = pts.iter().copied().collect();
        assert_eq!(set.len(), pts.len(), "no repeats at r = {r}");
        assert_eq!(pts.len(), if r == 0 { 1 } else { 8 * r as usize });
        assert!(
            pts.iter()
                .all(|&(x, y)| (x - 3).abs().max((y + 2).abs()) == r)
        );
    }
    // Straight east first.
    assert_eq!(ring((0, 0), 2)[0], (2, 0));
}

fn square(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    (x0..=x1)
        .flat_map(|x| (y0..=y1).map(move |y| (x, y)))
        .collect()
}

#[test]
fn free_offset_keeps_the_preferred_one_when_nothing_is_in_the_way() {
    let world = square(-2, -2, 2, 2);
    let o = free_offset(&world, &[], (40, 0), 3, 50);
    assert_eq!(o, Some((40, 0)));
}

#[test]
fn free_offset_steps_around_a_neighbour_with_the_gap() {
    let world = square(-2, -2, 2, 2);
    // A neighbour right where the world would go: x 36…44.
    let tr = Occupant {
        plugin: "TR.esm".into(),
        cells: square(36, -5, 44, 5),
    };
    let o = free_offset(&world, std::slice::from_ref(&tr), (40, 0), 3, 50).unwrap();
    assert!(collisions(&world, std::slice::from_ref(&tr), o, 3).is_empty());
    // Nearest by the larger axis distance: east of the neighbour, 44 + 3 gap + 2 half-width + 1.
    assert_eq!(o, (50, 0));
    assert_eq!(
        collisions(&world, &[tr], (40, 0), 0),
        [("TR.esm".to_owned(), 25)]
    );
}

#[test]
fn free_offset_avoids_vanilla() {
    let world = square(0, 0, 3, 3);
    let o = free_offset(&world, &[], (0, 0), 2, 60).unwrap();
    let (vx0, vy0, vx1, vy1) = crate::pipeline::VANILLA_EXTERIOR;
    for (x, y) in world {
        let (x, y) = (x + o.0, y + o.1);
        assert!(x < vx0 - 2 || x > vx1 + 2 || y < vy0 - 2 || y > vy1 + 2);
    }
}

#[test]
fn free_offset_gives_up_beyond_reach() {
    let world = square(0, 0, 1, 1);
    assert_eq!(free_offset(&world, &[], (0, 0), 0, 3), None);
    assert_eq!(free_offset(&[], &[], (0, 0), 0, 3), None);
}
