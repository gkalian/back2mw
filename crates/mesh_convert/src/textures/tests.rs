use super::*;

#[test]
fn name_is_flat_lowercase_and_prefixed() {
    let copy = target_of("textures\\Architecture\\Anvil\\AnvilWall01.dds").unwrap();
    assert_eq!(copy.target_name, "ob_architecture_anvil_anvilwall01.dds");
    assert_eq!(
        copy.source_path,
        "textures\\architecture\\anvil\\anvilwall01.dds"
    );
}

#[test]
fn a_path_without_the_textures_directory_is_taken_as_relative_to_it() {
    let copy = target_of("Clutter/Bowl01.dds").unwrap();
    assert_eq!(copy.target_name, "ob_clutter_bowl01.dds");
    assert_eq!(copy.source_path, "textures\\clutter\\bowl01.dds");
}

#[test]
fn separators_and_case_do_not_change_the_result() {
    let a = target_of("Textures\\A\\B.dds").unwrap();
    let b = target_of("textures/a//b.DDS").unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_bare_file_name_works() {
    assert_eq!(target_of("rock.dds").unwrap().target_name, "ob_rock.dds");
}

#[test]
fn the_extension_is_kept_from_the_source() {
    assert_eq!(
        target_of("textures\\a\\b.tga").unwrap().target_name,
        "ob_a_b.tga"
    );
}

#[test]
fn bad_paths_are_errors() {
    assert!(matches!(
        target_of("textures\\rocher_é.dds"),
        Err(TextureError::NotAscii { .. })
    ));
    assert!(matches!(
        target_of("textures\\"),
        Err(TextureError::Empty { .. })
    ));
    assert!(matches!(target_of(""), Err(TextureError::Empty { .. })));
    assert!(matches!(
        target_of("textures\\..\\x.dds"),
        Err(TextureError::ParentSegment { .. })
    ));
}

#[test]
fn normal_maps_are_never_taken() {
    for path in [
        "textures\\a\\wall_n.dds",
        "textures\\a\\WALL_N.DDS",
        "wall_n.dds",
    ] {
        assert!(
            matches!(target_of(path), Err(TextureError::NormalMap { .. })),
            "{path}"
        );
    }
    // A _n suffix inside the name is not a normal map.
    assert!(target_of("textures\\a\\wall_new.dds").is_ok());
    assert!(target_of("textures\\a_n\\wall.dds").is_ok());
}

#[test]
fn the_same_path_gets_the_same_name_again() {
    let mut reg = TextureRegistry::new();
    let a = reg.register("textures\\a\\b.dds").unwrap();
    let b = reg.register("Textures/A/B.dds").unwrap();
    assert_eq!(a, b);
    assert_eq!(reg.len(), 1);
}

#[test]
fn different_paths_with_one_flattened_name_collide() {
    let mut reg = TextureRegistry::new();
    reg.register("textures\\a_b\\c.dds").unwrap();
    assert_eq!(
        reg.register("textures\\a\\b_c.dds").unwrap_err(),
        TextureError::Collision {
            name: "ob_a_b_c.dds".to_owned(),
            first: "textures\\a_b\\c.dds".to_owned(),
            second: "textures\\a\\b_c.dds".to_owned(),
        }
    );
    assert_eq!(reg.len(), 1, "the collision overwrote nothing");
}

#[test]
fn the_collision_is_found_in_either_order() {
    let mut reg = TextureRegistry::new();
    reg.register("textures\\a\\b_c.dds").unwrap();
    assert!(matches!(
        reg.register("textures\\a_b\\c.dds"),
        Err(TextureError::Collision { .. })
    ));
}

#[test]
fn the_copy_list_does_not_depend_on_registration_order() {
    let paths = ["textures\\z.dds", "textures\\a\\b.dds", "textures\\m.dds"];
    let list = |order: &[usize]| {
        let mut reg = TextureRegistry::new();
        for &i in order {
            reg.register(paths[i]).unwrap();
        }
        reg.copies()
    };
    assert_eq!(list(&[0, 1, 2]), list(&[2, 0, 1]));
    assert_eq!(list(&[0, 1, 2])[0].target_name, "ob_a_b.dds");
}
