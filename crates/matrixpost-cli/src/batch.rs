//! Safe, local-only projection of MatrixMedia's directory/XLSX video batches.
//!
//! The workbook is only an input manifest. Each row is resolved against direct,
//! non-symlink files in one canonical directory before any local runner is called.

use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::ExitCode,
    str::FromStr,
};

use calamine::{Reader, open_workbook_auto};
use matrixpost_core::{
    AccountSelection, DispatchOutcome, MediaSource, Platform, PlatformOverride,
    ProviderDispatchReport, ProviderRegistry, PublishRequest, WechatLink,
};
use serde::Serialize;

use crate::{args::PublishArgs, output::emit};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct BatchRow {
    /// One-based worksheet row, retained even when blank filename rows are filtered.
    pub(crate) row: usize,
    pub(crate) file_name: String,
    pub(crate) title: Option<String>,
    pub(crate) tags: Option<String>,
    pub(crate) creative_statement: Option<String>,
}

#[derive(Debug)]
pub(crate) enum BatchItem {
    Ready {
        row: usize,
        file_name: String,
        request: Box<PublishRequest>,
    },
    Skipped {
        row: usize,
        file_name: String,
        reason: String,
    },
}

#[derive(Debug)]
pub(crate) struct BatchPlan {
    directory: PathBuf,
    items: Vec<BatchItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum BatchRowState {
    Queued { providers: ProviderDispatchReport },
    Unavailable { providers: ProviderDispatchReport },
    Rejected { reason: String },
    Skipped { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct BatchRowOutcome {
    pub(crate) row: usize,
    pub(crate) file_name: String,
    #[serde(flatten)]
    pub(crate) state: BatchRowState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct BatchSummary {
    rows: Vec<BatchRowOutcome>,
    queued: usize,
    unavailable: usize,
    rejected: usize,
    skipped: usize,
}

/// Cleans a workbook cell without losing meaningful internal word boundaries.
pub(crate) fn normalize_cell(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '\u{feff}' | '\u{200b}' | '\u{200c}' | '\u{200d}'))
        .collect::<String>()
        .trim()
        .to_owned()
}

fn normalize_header(value: &str) -> String {
    normalize_cell(value)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn column_index(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        let normalized = normalize_header(header).to_ascii_lowercase();
        names.iter().any(|name| normalized == *name)
    })
}

/// Maps the first worksheet into meaningful rows. Empty filename rows are ignored.
pub(crate) fn project_rows(rows: &[Vec<String>]) -> Result<Vec<BatchRow>, String> {
    let Some(headers) = rows.first() else {
        return Err("batch workbook has no readable rows".into());
    };
    let file = column_index(headers, &["文件名", "filename", "file"])
        .ok_or_else(|| "batch workbook needs a 文件名, filename, or file column".to_owned())?;
    let title = column_index(headers, &["标题", "title"]);
    let tags = column_index(headers, &["标签", "tags"]);
    let creative_statement = column_index(headers, &["创作声明", "creativestatement", "cs"]);
    Ok(rows
        .iter()
        .enumerate()
        .skip(1)
        .filter_map(|(index, row)| {
            let value = |column: Option<usize>| {
                column
                    .and_then(|index| row.get(index))
                    .map(|value| normalize_cell(value))
                    .filter(|value| !value.is_empty())
            };
            let file_name = value(Some(file))?;
            Some(BatchRow {
                row: index + 1,
                file_name,
                title: value(title),
                tags: value(tags),
                creative_statement: value(creative_statement),
            })
        })
        .collect())
}

fn read_rows(config: &Path) -> Result<Vec<BatchRow>, String> {
    let mut workbook = open_workbook_auto(config)
        .map_err(|error| format!("cannot read batch workbook: {error}"))?;
    let sheet = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "batch workbook has no sheets".to_owned())?;
    let range = workbook
        .worksheet_range(&sheet)
        .map_err(|error| format!("cannot read first batch sheet: {error}"))?;
    let cells = range
        .rows()
        .map(|row| row.iter().map(ToString::to_string).collect())
        .collect::<Vec<Vec<String>>>();
    project_rows(&cells)
}

fn canonical_directory(value: &Path) -> Result<PathBuf, String> {
    if matches!(url::Url::parse(&value.to_string_lossy()), Ok(url) if matches!(url.scheme(), "http" | "https"))
    {
        return Err("--dir must be a local directory, not a remote URL".into());
    }
    let metadata =
        fs::symlink_metadata(value).map_err(|error| format!("cannot inspect --dir: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("--dir must name an existing non-symlink directory".into());
    }
    fs::canonicalize(value).map_err(|error| format!("cannot canonicalize --dir: {error}"))
}

fn canonical_config(value: &Path) -> Result<PathBuf, String> {
    let metadata =
        fs::symlink_metadata(value).map_err(|error| format!("cannot inspect --config: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("--config must name a regular non-symlink .xlsx file".into());
    }
    if !value
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        return Err("--config must name a .xlsx file".into());
    }
    fs::canonicalize(value).map_err(|error| format!("cannot canonicalize --config: {error}"))
}

pub(crate) fn direct_files(directory: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    for entry in
        fs::read_dir(directory).map_err(|error| format!("cannot enumerate --dir: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot read a --dir entry: {error}"))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("cannot inspect a --dir entry: {error}"))?;
        if metadata.is_file() && !metadata.file_type().is_symlink() && is_direct_video(&path) {
            let canonical = fs::canonicalize(&path)
                .map_err(|error| format!("cannot canonicalize a --dir file: {error}"))?;
            if !canonical.starts_with(directory) {
                return Err("a --dir file resolves outside the canonical directory".into());
            }
            let canonical_metadata = fs::symlink_metadata(&canonical)
                .map_err(|error| format!("cannot re-check a --dir file: {error}"))?;
            if !canonical_metadata.is_file() || canonical_metadata.file_type().is_symlink() {
                return Err("a --dir file changed while it was being inspected".into());
            }
            files.push(canonical);
        }
    }
    files.sort();
    Ok(files)
}

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "m4v", "mkv", "avi", "wmv", "flv", "webm", "mpeg", "mpg", "3gp", "3g2", "ts",
    "m2ts",
];

pub(crate) fn is_direct_video(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            VIDEO_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

/// Re-checks a planned source immediately before it reaches a local runner.
pub(crate) fn revalidate_source(directory: &Path, source: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| format!("cannot revalidate media file: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("media file is no longer a regular non-symlink file".into());
    }
    if !is_direct_video(source) {
        return Err("media file no longer has an allowed video extension".into());
    }
    let canonical = fs::canonicalize(source)
        .map_err(|error| format!("cannot canonicalize media file before dispatch: {error}"))?;
    if !canonical.starts_with(directory) {
        return Err("media file resolves outside the canonical batch directory".into());
    }
    let canonical_metadata = fs::symlink_metadata(&canonical)
        .map_err(|error| format!("cannot re-check canonical media file: {error}"))?;
    if canonical_metadata.file_type().is_symlink() || !canonical_metadata.is_file() {
        return Err("media file changed while it was being revalidated".into());
    }
    Ok(())
}

fn safe_file_name(value: &str) -> bool {
    let mut components = Path::new(value).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

pub(crate) fn resolve_file(value: &str, candidates: &[PathBuf]) -> Result<PathBuf, String> {
    if !safe_file_name(value) {
        return Err("file name must be a single relative file name".into());
    }
    let wanted = normalize_cell(value).to_lowercase();
    let exact = candidates
        .iter()
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                normalize_cell(&name.to_string_lossy()).to_lowercase() == wanted
            })
        })
        .collect::<Vec<_>>();
    let matches = if exact.is_empty() {
        candidates
            .iter()
            .filter(|path| {
                path.file_stem().is_some_and(|stem| {
                    normalize_cell(&stem.to_string_lossy()).to_lowercase() == wanted
                })
            })
            .collect()
    } else {
        exact
    };
    match matches.as_slice() {
        [file] => Ok((*file).clone()),
        [] => Err("no direct regular file matches this row".into()),
        _ => Err("multiple direct regular files match this row".into()),
    }
}

fn normalized_tags(value: Option<&str>, platform: Platform) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(normalize_cell)
        .filter(|tag| !tag.is_empty())
        .map(|tag| {
            if matches!(
                platform,
                Platform::WechatChannels | Platform::Douyin | Platform::Kuaishou
            ) && !tag.starts_with('#')
            {
                format!("#{tag}")
            } else {
                tag
            }
        })
        .collect()
}

fn batch_platform(args: &PublishArgs) -> Result<Platform, String> {
    if args.file.is_some() {
        return Err("--file cannot be combined with --dir".into());
    }
    if args.dir.is_none() || args.config.is_none() {
        return Err("--dir requires --config (or --xlsx)".into());
    }
    if args.title.is_some()
        || !args.tags.is_empty()
        || args.creative_statement.is_some()
        || !args.platform_overrides.is_empty()
    {
        return Err(
            "batch rows own title, tags, creative statement, and platform overrides".into(),
        );
    }
    let targets = args
        .platforms
        .iter()
        .map(|value| Platform::from_str(value).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    match targets.as_slice() {
        [platform] => Ok(*platform),
        _ => Err("batch publishing requires exactly one --platform".into()),
    }
}

pub(crate) fn row_request(
    args: &PublishArgs,
    platform: Platform,
    file: PathBuf,
    row: &BatchRow,
) -> Result<PublishRequest, String> {
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(normalize_cell)
        .filter(|stem| !stem.is_empty())
        .ok_or_else(|| "media file name has no usable UTF-8 stem".to_owned())?;
    let title = row.title.clone().unwrap_or_else(|| stem.clone());
    let overrides = row
        .creative_statement
        .as_ref()
        .map_or_else(Vec::new, |statement| {
            vec![PlatformOverride {
                platform,
                title: None,
                short_title: None,
                tags: None,
                creative_statement: Some(statement.clone()),
                account: None,
                wechat_link: None,
            }]
        });
    let request = PublishRequest {
        source: MediaSource::LocalFile(file),
        title: title.clone(),
        short_title: args.short_title.clone(),
        tags: normalized_tags(row.tags.as_deref(), platform),
        address: args.address.clone(),
        draft: args.draft,
        bt2: Some(title),
        scheduled_at: args.publish_at.clone(),
        task_name: Some(stem),
        account: AccountSelection {
            phone: args.phone.clone(),
            partition: args.partition.clone(),
        },
        wechat_link: WechatLink {
            product_id: args.sph_product_id.clone(),
            link_type: args.sph_link_type.clone(),
            link_value: args.sph_link_value.clone(),
        },
        overrides,
        targets: vec![platform],
    };
    request.validate().map_err(|error| error.to_string())?;
    Ok(request)
}

pub(crate) fn prepare(args: PublishArgs) -> Result<BatchPlan, String> {
    let platform = batch_platform(&args)?;
    let directory = canonical_directory(args.dir.as_deref().expect("checked dir"))?;
    let config = canonical_config(args.config.as_deref().expect("checked config"))?;
    let rows = read_rows(&config)?;
    if rows.is_empty() {
        return Err("batch workbook has no effective file rows".into());
    }
    let candidates = direct_files(&directory)?;
    let items = rows
        .into_iter()
        .map(|row| {
            let row_number = row.row;
            match resolve_file(&row.file_name, &candidates)
                .and_then(|file| row_request(&args, platform, file, &row))
            {
                Ok(request) => BatchItem::Ready {
                    row: row_number,
                    file_name: row.file_name,
                    request: Box::new(request),
                },
                Err(reason) => BatchItem::Skipped {
                    row: row_number,
                    file_name: row.file_name,
                    reason,
                },
            }
        })
        .collect();
    Ok(BatchPlan { directory, items })
}

fn row_outcome(
    row: usize,
    file_name: String,
    report: Result<ProviderDispatchReport, String>,
) -> BatchRowOutcome {
    let state = match report {
        Err(reason) => BatchRowState::Rejected { reason },
        Ok(report)
            if report
                .outcomes
                .values()
                .all(|outcome| matches!(outcome, DispatchOutcome::Queued { .. })) =>
        {
            BatchRowState::Queued { providers: report }
        }
        Ok(report)
            if report
                .outcomes
                .values()
                .all(|outcome| matches!(outcome, DispatchOutcome::Unavailable { .. })) =>
        {
            BatchRowState::Unavailable { providers: report }
        }
        Ok(report) => BatchRowState::Rejected {
            reason: format!("provider dispatch was incomplete: {report:?}"),
        },
    };
    BatchRowOutcome {
        row,
        file_name,
        state,
    }
}

fn classify(rows: &[BatchRowOutcome]) -> u8 {
    let skipped = rows
        .iter()
        .any(|row| matches!(row.state, BatchRowState::Skipped { .. }));
    let attempted = rows
        .iter()
        .filter(|row| !matches!(row.state, BatchRowState::Skipped { .. }))
        .collect::<Vec<_>>();
    if !skipped
        && !attempted.is_empty()
        && attempted
            .iter()
            .all(|row| matches!(row.state, BatchRowState::Queued { .. }))
    {
        0
    } else if !skipped
        && !attempted.is_empty()
        && attempted
            .iter()
            .all(|row| matches!(row.state, BatchRowState::Unavailable { .. }))
    {
        3
    } else {
        4
    }
}

fn summary(rows: Vec<BatchRowOutcome>) -> BatchSummary {
    let mut result = BatchSummary {
        queued: 0,
        unavailable: 0,
        rejected: 0,
        skipped: 0,
        rows,
    };
    for row in &result.rows {
        match row.state {
            BatchRowState::Queued { .. } => result.queued += 1,
            BatchRowState::Unavailable { .. } => result.unavailable += 1,
            BatchRowState::Rejected { .. } => result.rejected += 1,
            BatchRowState::Skipped { .. } => result.skipped += 1,
        }
    }
    result
}

pub(crate) fn dispatch(registry: &ProviderRegistry, plan: BatchPlan) -> ExitCode {
    let mut outcomes = Vec::with_capacity(plan.items.len());
    for item in plan.items {
        let outcome = match item {
            BatchItem::Ready {
                row,
                file_name,
                request,
            } => match &request.source {
                MediaSource::LocalFile(source) => {
                    match revalidate_source(&plan.directory, source) {
                        Ok(()) => row_outcome(
                            row,
                            file_name,
                            registry
                                .dispatch_all(&request)
                                .map_err(|error| error.to_string()),
                        ),
                        Err(reason) => BatchRowOutcome {
                            row,
                            file_name,
                            state: BatchRowState::Skipped { reason },
                        },
                    }
                }
                MediaSource::RemoteUrl(_) => BatchRowOutcome {
                    row,
                    file_name,
                    state: BatchRowState::Skipped {
                        reason: "batch source is unexpectedly remote".into(),
                    },
                },
            },
            BatchItem::Skipped {
                row,
                file_name,
                reason,
            } => BatchRowOutcome {
                row,
                file_name,
                state: BatchRowState::Skipped { reason },
            },
        };
        // Progress is intentionally JSONL. It records only local runner outcomes.
        println!(
            "{}",
            serde_json::json!({ "event": "batch_progress", "row": &outcome })
        );
        outcomes.push(outcome);
    }
    let code = classify(&outcomes);
    let message = match code {
        0 => Some(
            "all rows were queued by local runners; remote platform processing is not confirmed",
        ),
        3 => Some("every attempted row was unavailable; no publishing was attempted"),
        _ => Some("batch completed with skipped, rejected, or mixed local runner outcomes"),
    };
    emit(code, summary(outcomes), message)
}

#[cfg(test)]
pub(crate) fn classify_rows(rows: &[BatchRowOutcome]) -> u8 {
    classify(rows)
}
