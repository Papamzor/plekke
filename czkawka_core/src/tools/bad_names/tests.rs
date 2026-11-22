use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::common::tool_data::CommonData;
use crate::common::traits::Search;
use crate::tools::bad_names::core::check_and_generate_new_name;
use crate::tools::bad_names::{BadNames, BadNamesParameters, NameIssues};

/// Scans a directory holding only `file_name` and returns the proposed new names.
fn scan_new_names(file_name: &str, issues: NameIssues) -> Vec<String> {
    let temp_dir = tempfile::tempdir().unwrap();
    fs::write(temp_dir.path().join(file_name), "test").unwrap();

    let mut bad_names = BadNames::new(BadNamesParameters::new(issues));
    bad_names.get_cd_mut().directories.set_included_paths(vec![temp_dir.path().to_path_buf()]);
    bad_names.search(&Arc::new(AtomicBool::new(false)), None);

    bad_names.get_bad_names_files().iter().map(|entry| entry.new_name.clone()).collect()
}

/// Checks each (input, expected) pair: equal pairs must stay unchanged, others must be fixed to
/// `expected` and the fixed name must not be changed again (when `check_double_fix`).
fn assert_fixes(check_params: &NameIssues, test_cases: &[(&str, &str)], check_double_fix: bool) {
    let mut errors = Vec::new();
    for &(input, expected_output) in test_cases {
        match check_and_generate_new_name(Path::new(input), check_params) {
            Some(new_name) if input == expected_output => errors.push(format!("Input: '{input}' should not be modified but got: '{new_name}'")),
            None if input != expected_output => errors.push(format!("Input: '{input}' was not fixed")),
            Some(new_name) => {
                if new_name != expected_output {
                    errors.push(format!("Input: '{input}', Expected: '{expected_output}', Got: '{new_name}'"));
                }
                if check_double_fix && check_and_generate_new_name(Path::new(&new_name), check_params).is_some() {
                    errors.push(format!("Double fix should return None for: '{new_name}'"));
                }
            }
            None => {}
        }
    }
    assert!(errors.is_empty(), "Name fix tests failed:\n{}", errors.join("\n"));
}

#[test]
fn test_uppercase_extension_detection() {
    let issues = NameIssues {
        uppercase_extension: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("test.TXT", issues), ["test.txt"]);
}

#[test]
fn test_emoji_detection() {
    let issues = NameIssues {
        emoji_used: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("test😀.txt", issues), ["test.txt"]);
}

#[test]
fn test_space_at_start_end_stem_detection() {
    let issues = NameIssues {
        space_at_start_or_end: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names(" test .txt", issues), ["test.txt"]);
}

#[test]
fn test_space_at_start_end_extension_detection() {
    let issues = NameIssues {
        space_at_start_or_end: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("test. txt ", issues), ["test.txt"]);
}

#[test]
fn test_non_ascii_graphical_detection() {
    let issues = NameIssues {
        non_ascii_graphical: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("tëst.txt", issues), ["test.txt"]);
}

#[test]
fn test_restricted_charset_detection() {
    let issues = NameIssues {
        restricted_charset_allowed: Some(vec!['_', '-', ' ']),
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("test@file.txt", issues), ["testfile.txt"]);
}

#[test]
fn test_duplicated_non_alphanumeric() {
    let issues = NameIssues {
        remove_duplicated_non_alphanumeric: true,
        ..NameIssues::default()
    };
    assert_eq!(scan_new_names("test__file--name.txt", issues), ["test_file-name.txt"]);
}

#[test]
fn test_multiple_issues() {
    assert_eq!(scan_new_names(" tëst😀 .TXT ", NameIssues::all()), ["test.txt"]);
}

#[test]
fn test_uppercase_extension_unit() {
    let check_params = NameIssues {
        uppercase_extension: true,
        ..NameIssues::default()
    };
    let test_cases = [
        ("test.TXT", "test.txt"),
        ("file.Jpg", "file.jpg"),
        ("document.PDF", "document.pdf"),
        ("image.PnG", "image.png"),
        ("video.MP4", "video.mp4"),
        ("archive.ZIP", "archive.zip"),
        ("data.CSV", "data.csv"),
        ("presentation.PPTX", "presentation.pptx"),
        ("script.Py", "script.py"),
        ("code.Js", "code.js"),
        ("style.Css", "style.css"),
        ("page.Html", "page.html"),
        ("config.Json", "config.json"),
        ("readme.Md", "readme.md"),
        ("Makefile.Mk", "Makefile.mk"),
        ("abc.cde.TXT", "abc.cde.txt"),
        ("file.backup.PDF", "file.backup.pdf"),
        ("my.file.name.JPG", "my.file.name.jpg"),
        ("test.1.2.3.Zip", "test.1.2.3.zip"),
        ("document.v2.0.Doc", "document.v2.0.doc"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_emoji_removal_unit() {
    let check_params = NameIssues {
        emoji_used: true,
        ..NameIssues::default()
    };
    let test_cases = [
        ("test😀.txt", "test.txt"),
        ("file🎉🎊.doc", "file.doc"),
        ("image❤.png", "image.png"),
        ("video🔥.mp4", "video.mp4"),
        ("doc👍.pdf", "doc.pdf"),
        ("report😊😊😊.xlsx", "report.xlsx"),
        ("photo🌟.jpg", "photo.jpg"),
        ("music🎵🎶.mp3", "music.mp3"),
        ("readme📝.md", "readme.md"),
        ("party🎈🎉🎊🎁.txt", "party.txt"),
        ("love💕💖💗💘.doc", "love.doc"),
        ("fire🔥🔥🔥.log", "fire.log"),
        ("star⭐.txt", "star.txt"),
        ("food🍕🍔🍟.jpg", "food.jpg"),
        ("weather☀🌧⛈.csv", "weather.csv"),
        ("test😀.backup.txt", "test.backup.txt"),
        ("my.file🎉.doc", "my.file.doc"),
        ("archive.v1.2🔥.zip", "archive.v1.2.zip"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_space_at_start_end_unit() {
    let check_params = NameIssues {
        space_at_start_or_end: true,
        ..NameIssues::default()
    };
    let test_cases = [
        (" test.txt", "test.txt"),
        ("test .txt", "test.txt"),
        (" test .txt", "test.txt"),
        ("  test  .txt", "test.txt"),
        ("test. txt ", "test.txt"),
        ("   file   .doc", "file.doc"),
        ("image .png", "image.png"),
        (" video.mp4", "video.mp4"),
        ("document .pdf", "document.pdf"),
        (" report .xlsx", "report.xlsx"),
        ("     data     .csv", "data.csv"),
        ("photo . jpg ", "photo.jpg"),
        (" music .mp3", "music.mp3"),
        ("readme . md ", "readme.md"),
        ("  archive  . zip ", "archive.zip"),
        (" abc.cde.txt", "abc.cde.txt"),
        ("abc.cde .txt", "abc.cde.txt"),
        (" my.file.name .doc", "my.file.name.doc"),
        ("  test.1.2  . pdf ", "test.1.2.pdf"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_non_ascii_graphical_unit() {
    let check_params = NameIssues {
        non_ascii_graphical: true,
        ..NameIssues::default()
    };
    let test_cases = [
        ("tëst.txt", "test.txt"),
        ("café.pdf", "cafe.pdf"),
        ("Kraków.doc", "Krakow.doc"),
        ("Łódź.txt", "Lodz.txt"),
        ("naïve.doc", "naive.doc"),
        ("résumé.pdf", "resume.pdf"),
        ("São Paulo.txt", "Sao Paulo.txt"),
        ("Zürich.doc", "Zurich.doc"),
        ("Москва.txt", "Moskva.txt"),
        ("日本.txt", "Ri Ben.txt"),
        ("über.pdf", "uber.pdf"),
        ("señor.txt", "senor.txt"),
        ("Ærø.doc", "AEro.doc"),
        ("niño.txt", "nino.txt"),
        ("Björk.mp3", "Bjork.mp3"),
        ("François.doc", "Francois.doc"),
        ("Ñoño.txt", "Nono.txt"),
        ("Østergård.pdf", "Ostergard.pdf"),
        ("Łukasz.txt", "Lukasz.txt"),
        ("Müller.doc", "Muller.doc"),
        ("pièces", "pieces"),
        // (`•`->`*`, `¿`->`?`, unmapped->`[?]`); these must be stripped, not kept.
        ("a•b.txt", "ab.txt"),
        ("All Posts • Instagram(1).txt", "All Posts  Instagram(1).txt"),
        ("¿que.txt", "que.txt"),
        ("x\u{0378}y.txt", "xy.txt"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_restricted_charset_unit() {
    let check_params = NameIssues {
        restricted_charset_allowed: Some(vec!['_', '-', ' ']),
        ..NameIssues::default()
    };
    let test_cases = [
        ("test@file.txt", "testfile.txt"),
        ("my#doc.pdf", "mydoc.pdf"),
        ("file$name.doc", "filename.doc"),
        ("data%set.csv", "dataset.csv"),
        ("script&code.js", "scriptcode.js"),
        ("image*pic.png", "imagepic.png"),
        ("video(1).mp4", "video1.mp4"),
        ("photo[2].jpg", "photo2.jpg"),
        ("doc{test}.pdf", "doctest.pdf"),
        ("file|name.txt", "filename.txt"),
        ("test:file.doc", "testfile.doc"),
        ("name;value.csv", "namevalue.csv"),
        ("file'name.txt", "filename.txt"),
        ("test\"quote.doc", "testquote.doc"),
        ("data<less.xml", "dataless.xml"),
        ("file>more.txt", "filemore.txt"),
        ("question?.log", "question.log"),
        ("wild*.txt", "wild.txt"),
        ("comma,.csv", "comma.csv"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_duplicated_non_alphanumeric_unit() {
    let check_params = NameIssues {
        remove_duplicated_non_alphanumeric: true,
        ..NameIssues::default()
    };
    let test_cases = [
        ("test__file.txt", "test_file.txt"),
        ("my--doc.pdf", "my-doc.pdf"),
        ("file  name.doc", "file name.doc"),
        ("data...set.csv", "data.set.csv"),
        ("script___code.js", "script_code.js"),
        ("image---pic.png", "image-pic.png"),
        ("test____file----name.txt", "test_file-name.txt"),
        ("multiple   spaces.doc", "multiple spaces.doc"),
        ("under______score.log", "under_score.log"),
        ("dash-------line.txt", "dash-line.txt"),
        ("mixed__--__test.doc", "mixed_-_test.doc"),
        ("file,,,,name.csv", "file,name.csv"),
        ("test;;;;code.txt", "test;code.txt"),
        ("data::::value.xml", "data:value.xml"),
        ("triple___---...test.txt", "triple_-.test.txt"),
        ("many        spaces.doc", "many spaces.doc"),
        ("dots......dots.txt", "dots.dots.txt"),
        ("under_score.txt", "under_score.txt"),
        ("normal-file.txt", "normal-file.txt"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_combined_all_issues_unit() {
    let check_params = NameIssues {
        uppercase_extension: true,
        emoji_used: true,
        space_at_start_or_end: true,
        non_ascii_graphical: true,
        restricted_charset_allowed: Some(vec!['_', '-', ' ']),
        remove_duplicated_non_alphanumeric: true,
    };
    let test_cases = [
        (" tëst😀 .TXT ", "test.txt"),
        ("  café☕  .Pdf  ", "cafe.pdf"),
        (" über@file😊 .Txt ", "uberfile.txt"),
        ("test__😀__file.JPG", "test_file.jpg"),
        (" Kraków🎉 .Doc ", "Krakow.doc"),
        ("  résumé##  .PDF  ", "resume.pdf"),
        ("São Paulo  .TXT", "Sao Paulo.txt"),
        (" file___name😀😀.PNG ", "file_name.png"),
        ("test  @@  emoji🎉.MP4", "test emoji.mp4"),
        (" Łódź---file .CSV ", "Lodz-file.csv"),
        ("über__müller😊.XLSX", "uber_muller.xlsx"),
        (" data___set🔥 . JSON ", "data_set.json"),
        ("test  ##  ëmoji😀.Doc", "test emoji.doc"),
        (" François___Müller .PDF ", "Francois_Muller.pdf"),
        ("multi___issue___test😀😀 .TXT ", "multi_issue_test.txt"),
    ];
    assert_fixes(&check_params, &test_cases, true);
}

#[test]
fn test_no_issues_no_changes() {
    let test_cases = [
        "normal_file.txt",
        "test-file.doc",
        "MyDocument.pdf",
        "data_2024.csv",
        "image-001.jpg",
        "video_final.mp4",
        "report-2024-01.xlsx",
        "README.md",
        "config.json",
        "script.py",
    ];
    assert_fixes(&NameIssues::all(), &test_cases.map(|name| (name, name)), true);
}

#[test]
fn test_edge_cases_unit() {
    let test_cases = [
        // Stems stripped to nothing become the "empty" placeholder.
        ("😀.txt", "empty.txt"),
        ("   .TXT", "empty.txt"),
        ("😀😀😀.txt", "empty.txt"),
        ("___", "_"),
        ("---", "-"),
        ("...", "."),
        (" 😀 .TXT ", "empty.txt"),
        ("test.", "test"),
        (".test", ".test"),
    ];
    assert_fixes(&NameIssues::all(), &test_cases, false);
}
