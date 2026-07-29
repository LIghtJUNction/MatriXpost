use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use calamine::{Reader, open_workbook_auto};
use clap::Parser;
use matrixpost_core::{DispatchOutcome, MediaSource, Platform, ProviderDispatchReport};

use crate::{
    args::{Cli, Command},
    batch::{
        BatchRow, BatchRowOutcome, BatchRowState, classify_rows, direct_files, is_direct_video,
        normalize_cell, prepare, project_rows, resolve_file, revalidate_source, row_request,
    },
    query::parse_request,
};

fn batch_args(extra: &[&str]) -> crate::args::PublishArgs {
    let mut argv = vec![
        "matrixpost",
        "publish",
        "-p",
        "dy",
        "--dir",
        "media",
        "--config",
        "rows.xlsx",
    ];
    argv.extend_from_slice(extra);
    let cli = Cli::try_parse_from(argv).unwrap();
    let Command::Publish(args) = cli.command else {
        panic!("expected publish");
    };
    args
}

#[test]
fn batch_headers_are_normalized_and_blank_file_rows_are_ignored() {
    assert_eq!(
        normalize_cell("\u{feff}  A  title\tkept  \u{200b}"),
        "A  title\tkept"
    );
    let rows = project_rows(&[
        vec![
            " 文件名 ".into(),
            "标题".into(),
            "标签".into(),
            " 创作声明 ".into(),
        ],
        vec![
            " movie.mp4 ".into(),
            " A  title\tkept ".into(),
            "one,  two words".into(),
            "original  statement\tkept".into(),
        ],
        vec![" \t\n".into(), "ignored".into()],
    ])
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].row, 2);
    assert_eq!(rows[0].file_name, "movie.mp4");
    assert_eq!(rows[0].title.as_deref(), Some("A  title\tkept"));
    assert_eq!(rows[0].tags.as_deref(), Some("one,  two words"));
    assert_eq!(
        rows[0].creative_statement.as_deref(),
        Some("original  statement\tkept")
    );
}

#[test]
fn batch_rows_retain_their_original_worksheet_row_after_blank_filtering() {
    let rows = project_rows(&[
        vec!["文件名".into(), "标题".into()],
        vec!["  ".into(), "blank row two".into()],
        vec!["movie.mp4".into(), "row three".into()],
    ])
    .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].row, 3);
    assert_eq!(rows[0].file_name, "movie.mp4");
}

#[test]
fn publish_grammar_keeps_file_and_batch_forms_separate() {
    assert!(Cli::try_parse_from(["matrixpost", "publish", "-p", "dy"]).is_err());
    let file = Cli::try_parse_from([
        "matrixpost",
        "publish",
        "-p",
        "dy",
        "-f",
        "movie.mp4",
        "-t",
        "title",
    ])
    .unwrap();
    assert!(
        matches!(file.command, Command::Publish(args) if args.file.is_some() && args.dir.is_none())
    );
    assert!(
        Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "dy",
            "-f",
            "movie.mp4",
            "--dir",
            "media"
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "dy",
            "--file",
            "movie.mp4",
            "--config",
            "rows.xlsx",
        ])
        .is_err()
    );
    assert!(
        Cli::try_parse_from([
            "matrixpost",
            "publish",
            "-p",
            "dy",
            "--file",
            "movie.mp4",
            "--xlsx",
            "rows.xlsx",
        ])
        .is_err()
    );
    let batch = batch_args(&[]);
    assert!(
        batch.file.is_none()
            && batch.dir.is_some()
            && batch.config.is_some()
            && batch.title.is_none()
    );
    let xlsx_alias = Cli::try_parse_from([
        "matrixpost",
        "publish",
        "-p",
        "dy",
        "--dir",
        "media",
        "--xlsx",
        "rows.xlsx",
    ])
    .unwrap();
    assert!(
        matches!(xlsx_alias.command, Command::Publish(args) if args.config == Some("rows.xlsx".into()))
    );
    assert_eq!(
        prepare(batch_args(&["--title", "not allowed"])).unwrap_err(),
        "batch rows own title, tags, creative statement, and platform overrides"
    );
    let missing_config =
        Cli::try_parse_from(["matrixpost", "publish", "-p", "dy", "--dir", "media"]).unwrap();
    let Command::Publish(missing_config) = missing_config.command else {
        panic!("expected publish");
    };
    assert_eq!(
        prepare(missing_config).unwrap_err(),
        "--dir requires --config (or --xlsx)"
    );
    let remote_dir = Cli::try_parse_from([
        "matrixpost",
        "publish",
        "-p",
        "dy",
        "--dir",
        "https://example.invalid/media",
        "--config",
        "rows.xlsx",
    ])
    .unwrap();
    let Command::Publish(remote_dir) = remote_dir.command else {
        panic!("expected publish");
    };
    assert_eq!(
        prepare(remote_dir).unwrap_err(),
        "--dir must be a local directory, not a remote URL"
    );
    let single_without_title =
        Cli::try_parse_from(["matrixpost", "publish", "-p", "dy", "--file", "movie.mp4"]).unwrap();
    let Command::Publish(single_without_title) = single_without_title.command else {
        panic!("expected publish");
    };
    assert_eq!(
        parse_request(single_without_title).unwrap_err(),
        "--title is required with --file"
    );
}

#[test]
fn exact_casefold_stem_and_unsafe_batch_resolution_are_deterministic() {
    let root = std::env::temp_dir().join(format!("matrixpost-batch-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let video = root.join("Video One.mp4");
    let same_stem = root.join("other.MP4");
    let other_kind = root.join("other.mov");
    fs::write(&video, []).unwrap();
    fs::write(&same_stem, []).unwrap();
    fs::write(&other_kind, []).unwrap();
    let candidates = vec![video.clone(), same_stem, other_kind];
    assert_eq!(
        resolve_file("video one.mp4", &candidates).unwrap(),
        fs::canonicalize(&video).unwrap()
    );
    assert!(
        resolve_file("other", &candidates)
            .unwrap_err()
            .contains("multiple")
    );
    assert!(resolve_file("../Video One.mp4", &candidates).is_err());
    assert!(resolve_file("/etc/passwd", &candidates).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn direct_candidates_are_limited_to_documented_video_extensions() {
    assert!(is_direct_video(PathBuf::from("clip.MP4").as_path()));
    assert!(is_direct_video(PathBuf::from("clip.m2ts").as_path()));
    assert!(!is_direct_video(PathBuf::from("rows.xlsx").as_path()));
    assert!(!is_direct_video(PathBuf::from("notes.txt").as_path()));

    let root = std::env::temp_dir().join(format!("matrixpost-video-filter-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let video = root.join("clip.mp4");
    fs::write(&video, []).unwrap();
    fs::write(root.join("rows.xlsx"), []).unwrap();
    fs::write(root.join("notes.txt"), []).unwrap();
    let candidates = direct_files(&fs::canonicalize(&root).unwrap()).unwrap();
    assert_eq!(candidates, vec![fs::canonicalize(&video).unwrap()]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn revalidation_requires_a_regular_video_beneath_the_original_directory() {
    let root = std::env::temp_dir().join(format!("matrixpost-revalidate-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let video = root.join("clip.mp4");
    fs::write(&video, []).unwrap();
    let root = fs::canonicalize(&root).unwrap();
    let video = fs::canonicalize(video).unwrap();
    assert!(revalidate_source(&root, &video).is_ok());
    fs::remove_file(&video).unwrap();
    fs::write(&video, []).unwrap();
    fs::rename(&video, root.join("clip.txt")).unwrap();
    assert!(revalidate_source(&root, &video).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn revalidation_skips_a_source_replaced_with_a_symlink() {
    use std::os::unix::fs::symlink;

    let root = std::env::temp_dir().join(format!("matrixpost-symlink-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let video = root.join("clip.mp4");
    let target = root.join("replacement.mp4");
    fs::write(&video, []).unwrap();
    fs::write(&target, []).unwrap();
    let root = fs::canonicalize(&root).unwrap();
    let video = fs::canonicalize(video).unwrap();
    fs::remove_file(&video).unwrap();
    symlink(&target, &video).unwrap();
    assert_eq!(
        revalidate_source(&root, &video).unwrap_err(),
        "media file is no longer a regular non-symlink file"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn row_requests_use_local_sources_and_platform_specific_tags_and_statements() {
    let args = batch_args(&["--draft", "--sph-product-id", "product"]);
    let request = row_request(
        &args,
        Platform::Douyin,
        PathBuf::from("/tmp/movie.mp4"),
        &BatchRow {
            row: 2,
            file_name: "movie.mp4".into(),
            title: Some("row title".into()),
            tags: Some("one,#two".into()),
            creative_statement: Some("original".into()),
        },
    )
    .unwrap();
    assert!(matches!(request.source, MediaSource::LocalFile(_)));
    assert_eq!(request.title, "row title");
    assert_eq!(request.bt2.as_deref(), Some("row title"));
    assert_eq!(request.task_name.as_deref(), Some("movie"));
    assert_eq!(request.tags, vec!["#one", "#two"]);
    assert_eq!(
        request.overrides[0].creative_statement.as_deref(),
        Some("original")
    );
    assert!(request.draft);
    assert_eq!(request.wechat_link.product_id.as_deref(), Some("product"));
    let bilibili = row_request(
        &args,
        Platform::Bilibili,
        PathBuf::from("/tmp/movie.mp4"),
        &BatchRow {
            row: 2,
            file_name: "movie.mp4".into(),
            title: None,
            tags: Some("one".into()),
            creative_statement: None,
        },
    )
    .unwrap();
    assert_eq!(bilibili.tags, vec!["one"]);
}

fn report(outcome: DispatchOutcome) -> ProviderDispatchReport {
    ProviderDispatchReport {
        outcomes: [(Platform::Douyin, outcome)].into_iter().collect(),
    }
}

fn outcome(state: BatchRowState) -> BatchRowOutcome {
    BatchRowOutcome {
        row: 2,
        file_name: "movie.mp4".into(),
        state,
    }
}

#[test]
fn batch_exit_classification_covers_queued_unavailable_and_mixed_rows() {
    let queued = outcome(BatchRowState::Queued {
        providers: report(DispatchOutcome::Queued {
            job_id: "job".into(),
        }),
    });
    let unavailable = outcome(BatchRowState::Unavailable {
        providers: report(DispatchOutcome::Unavailable {
            reason: "offline".into(),
        }),
    });
    let skipped = outcome(BatchRowState::Skipped {
        reason: "missing".into(),
    });
    assert_eq!(classify_rows(std::slice::from_ref(&queued)), 0);
    assert_eq!(classify_rows(&[unavailable]), 3);
    assert_eq!(classify_rows(&[queued, skipped]), 4);
}

#[test]
fn xlsx_first_sheet_projects_unicode_cells_and_physical_rows() {
    let path = std::env::temp_dir().join(format!(
        "matrixpost-batch-{}-{}.xlsx",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before Unix epoch")
            .as_nanos()
    ));
    fs::write(&path, minimal_xlsx()).expect("write workbook fixture");

    let result = {
        let mut workbook = open_workbook_auto(&path).expect("open workbook with calamine");
        let first_sheet = workbook
            .sheet_names()
            .first()
            .expect("fixture first sheet")
            .to_owned();
        assert_eq!(first_sheet, "Batch");
        let cells = workbook
            .worksheet_range(&first_sheet)
            .expect("read first sheet")
            .rows()
            .map(|row| row.iter().map(ToString::to_string).collect())
            .collect::<Vec<Vec<String>>>();
        project_rows(&cells).expect("project workbook rows")
    };

    fs::remove_file(&path).expect("remove workbook fixture");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].row, 3);
    assert_eq!(result[0].file_name, "movie.mp4");
    assert_eq!(result[0].title.as_deref(), Some("A  title\tkept"));
    assert_eq!(result[0].tags.as_deref(), Some("one, two"));
    assert_eq!(result[0].creative_statement.as_deref(), Some("original"));
}

/// A source-embedded, stored ZIP/XLSX fixture: no external generator or binary asset is needed.
fn minimal_xlsx() -> Vec<u8> {
    let worksheet = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>﻿ 文 件 名 </t></is></c><c r="B1" t="inlineStr"><is><t> 标 题 </t></is></c><c r="C1" t="inlineStr"><is><t>标签</t></is></c><c r="D1" t="inlineStr"><is><t>创作声明</t></is></c></row>"#,
        r#"<row r="2"><c r="A2" t="inlineStr"><is><t xml:space="preserve">  </t></is></c><c r="B2" t="inlineStr"><is><t>ignored</t></is></c></row>"#,
        r#"<row r="3"><c r="A3" t="inlineStr"><is><t>​movie.mp4‌</t></is></c><c r="B3" t="inlineStr"><is><t xml:space="preserve"> A  title&#9;kept </t></is></c><c r="C3" t="inlineStr"><is><t>one, two</t></is></c><c r="D3" t="inlineStr"><is><t> original </t></is></c></row>"#,
        r#"</sheetData></worksheet>"#,
    );
    zip_stored(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Batch" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", worksheet),
    ])
}

fn zip_stored(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut central_directory = Vec::new();
    for (name, content) in entries {
        let offset = u32::try_from(output.len()).expect("fixture stays below ZIP32 limit");
        let name = name.as_bytes();
        let content = content.as_bytes();
        let crc = crc32(content);
        output.extend_from_slice(&0x0403_4b50_u32.to_le_bytes());
        output.extend_from_slice(&20_u16.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(&crc.to_le_bytes());
        output.extend_from_slice(
            &u32::try_from(content.len())
                .expect("fixture content fits ZIP32")
                .to_le_bytes(),
        );
        output.extend_from_slice(
            &u32::try_from(content.len())
                .expect("fixture content fits ZIP32")
                .to_le_bytes(),
        );
        output.extend_from_slice(
            &u16::try_from(name.len())
                .expect("fixture name fits ZIP16")
                .to_le_bytes(),
        );
        output.extend_from_slice(&0_u16.to_le_bytes());
        output.extend_from_slice(name);
        output.extend_from_slice(content);

        central_directory.extend_from_slice(&0x0201_4b50_u32.to_le_bytes());
        central_directory.extend_from_slice(&20_u16.to_le_bytes());
        central_directory.extend_from_slice(&20_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&crc.to_le_bytes());
        central_directory.extend_from_slice(
            &u32::try_from(content.len())
                .expect("fixture content fits ZIP32")
                .to_le_bytes(),
        );
        central_directory.extend_from_slice(
            &u32::try_from(content.len())
                .expect("fixture content fits ZIP32")
                .to_le_bytes(),
        );
        central_directory.extend_from_slice(
            &u16::try_from(name.len())
                .expect("fixture name fits ZIP16")
                .to_le_bytes(),
        );
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u16.to_le_bytes());
        central_directory.extend_from_slice(&0_u32.to_le_bytes());
        central_directory.extend_from_slice(&offset.to_le_bytes());
        central_directory.extend_from_slice(name);
    }
    let central_offset = u32::try_from(output.len()).expect("fixture stays below ZIP32 limit");
    output.extend_from_slice(&central_directory);
    output.extend_from_slice(&0x0605_4b50_u32.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output.extend_from_slice(
        &u16::try_from(entries.len())
            .expect("fixture count fits ZIP16")
            .to_le_bytes(),
    );
    output.extend_from_slice(
        &u16::try_from(entries.len())
            .expect("fixture count fits ZIP16")
            .to_le_bytes(),
    );
    output.extend_from_slice(
        &u32::try_from(central_directory.len())
            .expect("fixture directory fits ZIP32")
            .to_le_bytes(),
    );
    output.extend_from_slice(&central_offset.to_le_bytes());
    output.extend_from_slice(&0_u16.to_le_bytes());
    output
}

fn crc32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(!0_u32, |crc, byte| {
        (0..8).fold(crc ^ u32::from(*byte), |value, _| {
            (value >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(value & 1))
        })
    }) ^ !0_u32
}
