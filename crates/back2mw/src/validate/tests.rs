use super::*;

#[test]
fn clean_output_has_no_failures() {
    assert!(parse_niftest_output("").is_empty());
    assert!(parse_niftest_output("Reading Files\nDone\n").is_empty());
}

#[test]
fn a_read_failure_gives_the_path_and_the_reason() {
    let text = "Failed to read 'nt/broken.nif':\n\
                NIFFile Error: Invalid NIF header: garbage when reading nt/broken.nif\n";
    assert_eq!(
        parse_niftest_output(text),
        [NiftestFailure {
            path: "nt/broken.nif".to_owned(),
            reason: "NIFFile Error: Invalid NIF header: garbage when reading nt/broken.nif"
                .to_owned(),
        }]
    );
}

#[test]
fn a_missing_file_is_a_failure_too() {
    let text = "Failed to read 'nt/nothere.nif':\n\
                Failed to open 'nt/nothere.nif' for reading: 2\n";
    let f = parse_niftest_output(text);
    assert_eq!(f.len(), 1, "one mention of a file is one failure: {f:?}");
    assert_eq!(f[0].path, "nt/nothere.nif");
    assert_eq!(
        f[0].reason, "",
        "the second line is also «Failed», not a reason"
    );
}

#[test]
fn a_bare_open_failure_counts() {
    let f = parse_niftest_output("Failed to open 'x.nif' for reading: 2\n");
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].path, "x.nif");
}

#[test]
fn several_files_and_windows_line_endings() {
    let text =
        "Failed to read 'a.nif':\r\nreason a\r\nnoise\r\nFailed to read 'b.nif':\r\nreason b\r\n";
    let f = parse_niftest_output(text);
    assert_eq!(
        f.iter()
            .map(|x| (x.path.as_str(), x.reason.as_str()))
            .collect::<Vec<_>>(),
        [("a.nif", "reason a"), ("b.nif", "reason b")]
    );
}

#[test]
fn a_path_with_spaces_and_quotes_inside_is_taken_up_to_the_first_closing_quote() {
    let f = parse_niftest_output("Failed to read 'C:\\a b\\c.nif':\nx\n");
    assert_eq!(f[0].path, "C:\\a b\\c.nif");
}

#[test]
fn lines_that_only_mention_the_words_are_not_failures() {
    assert!(parse_niftest_output("Skipped: Failed to read is a phrase\n").is_empty());
    assert!(parse_niftest_output("Failed to read without quotes\n").is_empty());
}

#[test]
fn the_same_path_named_twice_counts_once() {
    let text = "Failed to read 'a.nif':\nr\nFailed to read 'a.nif':\nr\n";
    assert_eq!(parse_niftest_output(text).len(), 1);
}

fn paths(n: usize, len: usize) -> Vec<PathBuf> {
    (0..n)
        .map(|i| PathBuf::from(format!("{i:0>len$}")))
        .collect()
}

#[test]
fn chunks_cover_every_file_once_and_respect_the_budget() {
    let files = paths(50, 97); // 100 characters per path, with a margin
    let chunks = chunks_by_length(&files, 1000);
    assert!(chunks.len() > 1);
    let total: usize = chunks.iter().map(|c| c.len()).sum();
    assert_eq!(total, 50);
    for c in &chunks {
        let used: usize = c.iter().map(|p| p.as_os_str().len() + 3).sum();
        assert!(used <= 1000, "{used}");
    }
    let flat: Vec<&PathBuf> = chunks.iter().flat_map(|c| c.iter()).collect();
    assert_eq!(flat, files.iter().collect::<Vec<_>>(), "order is preserved");
}

#[test]
fn an_oversized_single_path_still_gets_its_own_chunk() {
    let files = paths(2, 500);
    let chunks = chunks_by_length(&files, 100);
    assert_eq!(chunks.iter().map(|c| c.len()).collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn no_files_no_chunks() {
    assert!(chunks_by_length(&[], 100).is_empty());
}

#[test]
fn a_tool_that_cannot_start_is_an_error_not_a_pass() {
    let missing = Path::new("definitely-not-a-real-tool-back2mw");
    assert!(matches!(
        check_plugin(missing, Path::new("x.esp")),
        Err(ValidateError::Spawn { .. })
    ));
    assert!(matches!(
        check_meshes(missing, &[PathBuf::from("x.nif")]),
        Err(ValidateError::Spawn { .. })
    ));
}
