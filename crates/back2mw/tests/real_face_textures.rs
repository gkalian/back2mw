//! The face detail map `Egt::detail` (T8.8) against Oblivion's ready-made maps
//! `textures\faces\oblivion.esm\<FormID>_0.dds` — 1,977 NPCs of `Oblivion.esm` have them in the
//! texture archive. Needs `BACK2MW_OBLIVION_DATA` — the `Data` directory with `Oblivion.esm` and
//! the BSAs.
//!
//! ```text
//! BACK2MW_OBLIVION_DATA='C:\path\to\Oblivion\Data' \
//!   cargo test --release -p back2mw --test real_face_textures -- --ignored --nocapture
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use mesh_convert::facegen::Egt;
use tes4_read::{BsaIndex, FromFields, Npc, Plugin, Race};

fn data_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("BACK2MW_OBLIVION_DATA").expect("BACK2MW_OBLIVION_DATA is not set"),
    )
}

/// Correlation coefficient of two series.
fn correlation(x: &[f64], y: &[f64]) -> f64 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "series have hundreds of thousands of values"
    )]
    let n = x.len() as f64;
    let (mx, my) = (x.iter().sum::<f64>() / n, y.iter().sum::<f64>() / n);
    let (mut sxy, mut sxx, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxy += (a - mx) * (b - my);
        sxx += (a - mx).powi(2);
        syy += (b - my).powi(2);
    }
    sxy / (sxx * syy).sqrt()
}

/// The NPC's `FGTS` coefficients plus the race's give a map matching the ready-made one up to
/// compression noise; the NPC's coefficients alone (control) are noticeably worse.
#[test]
#[ignore = "real Oblivion install: BACK2MW_OBLIVION_DATA"]
fn egt_detail_matches_the_shipped_face_maps() {
    let data = data_dir();
    let bytes = std::fs::read(data.join("Oblivion.esm")).unwrap();
    let plugin = Plugin::parse(&bytes).unwrap();
    let races: HashMap<u32, Race> = plugin
        .records_of(*b"RACE")
        .map(|r| (r.header.form_id, Race::from_record(r).unwrap()))
        .collect();
    let open = |name: &str| {
        let mut r = BufReader::new(File::open(data.join(name)).unwrap());
        let idx = BsaIndex::read_from(&mut r).unwrap();
        (r, idx)
    };
    let (mut meshes, mesh_idx) = open("Oblivion - Meshes.bsa");
    let (mut textures, texture_idx) = open("Oblivion - Textures - Compressed.bsa");
    let mut egts: HashMap<String, Egt> = HashMap::new();
    let (mut rs, mut own_rs, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    for rec in plugin.records_of(*b"NPC_") {
        let path = format!(
            r"textures\faces\oblivion.esm\{:08x}_0.dds",
            rec.header.form_id
        );
        let Some(entry) = texture_idx.find(&path) else {
            continue;
        };
        let npc = Npc::from_record(rec).unwrap();
        let race = &races[&npc.race];
        let head = race.head_parts.iter().find(|p| p.index == 0).unwrap();
        let egt_path = format!(
            r"meshes\{}",
            head.model.as_ref().unwrap().to_ascii_lowercase()
        )
        .replace(".nif", ".egt");
        let egt = egts.entry(egt_path.clone()).or_insert_with(|| {
            let e = mesh_idx.find(&egt_path).unwrap();
            Egt::parse(&mesh_idx.extract(&mut meshes, e).unwrap()).unwrap()
        });
        let (w, h, truth) =
            mesh_convert::icons::decode_rgba(&texture_idx.extract(&mut textures, entry).unwrap())
                .unwrap();
        if (w as usize, h as usize) != (egt.columns, egt.rows) {
            continue;
        }
        let sum: Vec<f32> = npc
            .face
            .texture_symmetric
            .iter()
            .zip(&race.face.texture_symmetric)
            .map(|(a, b)| a + b)
            .collect();
        let flat =
            |d: Vec<[u8; 3]>| -> Vec<f64> { d.iter().flat_map(|p| p.map(f64::from)).collect() };
        let ours = flat(egt.detail(&sum));
        let own = flat(egt.detail(&npc.face.texture_symmetric));
        let truth: Vec<f64> = truth
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]].map(f64::from))
            .collect();
        let r = correlation(&ours, &truth);
        if !r.is_finite() {
            continue;
        }
        rs.push(r);
        // Control only where the race has its own coefficients: for Argonians, Dark Elves and
        // others they are zero, and both ways match.
        if race.face.texture_symmetric.iter().any(|v| *v != 0.0) {
            own_rs.push(correlation(&own, &truth));
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "hundreds of thousands of pixels"
        )]
        errors.push(
            ours.iter()
                .zip(&truth)
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                / ours.len() as f64,
        );
    }
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let n = rs.len();
    let (r, own_r, err) = (median(&mut rs), median(&mut own_rs), median(&mut errors));
    println!(
        "NPC {n}: correlation {r:.3}, mean error {err:.2} levels; NPC alone without race ({} NPCs of races with coefficients) — {own_r:.3}",
        own_rs.len()
    );
    assert!(n > 1900, "maps with a matching .egt: {n}");
    assert!(r > 0.93 && err < 3.0, "r {r}, error {err}");
    assert!(
        own_r < r - 0.1,
        "control: without the race coefficients it is worse ({own_r})"
    );
}
