use std::collections::{BTreeMap, HashMap, HashSet};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

const APP_NAME: &str = "ai-code-detector";
const APP_VERSION: &str = "0.1.0";

#[derive(Clone, Debug)]
struct Args {
    target: PathBuf,
    deem: f64,
    only_json: bool,
    output_files: bool,
    output_dir: PathBuf,
    output_name: String,
}

#[derive(Clone, Debug)]
struct FileCollection {
    source_files: Vec<PathBuf>,
    skipped_non_source: usize,
    skipped_dirs: usize,
}

#[derive(Clone, Debug)]
struct CodeLine {
    file: String,
    project: String,
    line: usize,
    code: String,
    normalized: String,
    csharp: bool,
    decompiled: bool,
}

#[derive(Clone, Debug)]
struct ExcludedLine {
    file: String,
    line: usize,
    kind: String,
    reason: String,
}

#[derive(Clone, Debug)]
struct LineRecord {
    file: String,
    line: usize,
    excerpt: String,
    normalized: String,
    score: f64,
    reason: String,
}

#[derive(Clone, Debug)]
struct FileSummary {
    path: String,
    considered_lines: usize,
    excluded_lines: usize,
    percentage: f64,
    is_ai: bool,
    decompiled: bool,
    ai_features: usize,
    legit_features: usize,
    ai_positive: bool,
    strong_legit: bool,
    feature_summary: String,
}

#[derive(Clone, Debug)]
struct Analysis {
    target: String,
    percentage: f64,
    is_ai: bool,
    deem: f64,
    considered_lines: usize,
    excluded_lines: usize,
    source_files: usize,
    skipped_non_source_files: usize,
    skipped_dirs: usize,
    ai_calibration_enabled: bool,
    human_calibration_enabled: bool,
    ai_profile_lines: usize,
    human_profile_lines: usize,
    template_lines: usize,
    line_records: Vec<LineRecord>,
    excluded: Vec<ExcludedLine>,
    files: Vec<FileSummary>,
}

#[derive(Clone, Debug)]
struct LoadedSource {
    lines: Vec<CodeLine>,
    excluded: Vec<ExcludedLine>,
    raw_file_stats: HashMap<String, FileStats>,
    source_files: usize,
    skipped_non_source: usize,
    skipped_dirs: usize,
}

#[derive(Clone, Debug, Default)]
struct FileStats {
    csharp: bool,
    decompiled: bool,
    source_lines: usize,
    namespaces: usize,
    classes: usize,
    config_binds: usize,
    harmony_patches: usize,
    bep_in_plugin: usize,
    base_unity_plugin: usize,
    reflection_markers: usize,
    generated_mod_markers: usize,
    using_count: usize,
    assembly_attributes: usize,
    debug_metadata: usize,
    developer_debug_metadata: usize,
    repository_metadata: usize,
    named_company_metadata: usize,
    title_product_metadata: usize,
    informational_version_metadata: usize,
    description_metadata: usize,
    security_metadata: usize,
    ignores_access_checks: usize,
    asset_bundle_markers: usize,
    file_io_markers: usize,
    task_async_markers: usize,
    public_static_markers: usize,
    non_ascii_chars: usize,
}

#[derive(Clone, Debug)]
struct DecompiledProfile {
    ai_features: usize,
    legit_features: usize,
    base_score: f64,
    cap: f64,
    ai_positive: bool,
    strong_legit: bool,
    summary: String,
}

#[derive(Clone, Copy, Debug)]
struct Syntax {
    line_markers: &'static [&'static str],
    block_markers: &'static [(&'static str, &'static str)],
}

fn main() {
    let args = match parse_args(env::args().skip(1).collect()) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}\n\n{}", usage());
            std::process::exit(2);
        }
    };

    let analysis = match analyze(&args) {
        Ok(analysis) => analysis,
        Err(error) => {
            eprintln!("scan failed: {error}");
            std::process::exit(1);
        }
    };

    let json = render_json(&analysis);
    println!("{json}");

    if args.output_files {
        if let Err(error) = fs::create_dir_all(&args.output_dir) {
            eprintln!(
                "failed to create output directory {}: {error}",
                args.output_dir.display()
            );
            std::process::exit(1);
        }

        let json_path = args.output_dir.join(format!("{}.json", args.output_name));
        if let Err(error) = fs::write(&json_path, &json) {
            eprintln!("failed to write {}: {error}", json_path.display());
        }
        if !args.only_json {
            let md_path = args.output_dir.join(format!("{}.md", args.output_name));
            if let Err(error) = fs::write(&md_path, render_markdown(&analysis)) {
                eprintln!("failed to write {}: {error}", md_path.display());
            }
        }
    }
}

fn parse_args(raw: Vec<String>) -> Result<Args, String> {
    if raw
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "/?"))
    {
        return Err(String::new());
    }

    let mut target = None;
    let mut deem = 50.0;
    let mut only_json = false;
    let mut output_files = true;
    let mut output_dir = PathBuf::from(".");
    let mut output_name = "ai_detection_report".to_string();
    let mut i = 0;

    while i < raw.len() {
        let arg = &raw[i];
        if let Some((key, value)) = split_assignment(arg) {
            match canonical_key(key).as_str() {
                "deem" => deem = parse_deem(value)?,
                "onlyjson" => only_json = parse_bool(value)?,
                "outputfiles" => output_files = parse_bool(value)?,
                "outputdir" => output_dir = PathBuf::from(value),
                "outputname" => output_name = value.to_string(),
                _ => return Err(format!("unknown argument: {arg}")),
            }
            i += 1;
            continue;
        }

        match canonical_key(arg).as_str() {
            "deem" => {
                i += 1;
                let value = raw.get(i).ok_or("--Deem requires a value")?;
                deem = parse_deem(value)?;
            }
            "onlyjson" => {
                let (value, consumed) = optional_bool(&raw, i + 1)?;
                only_json = value.unwrap_or(true);
                i += consumed;
            }
            "outputfiles" => {
                let (value, consumed) = optional_bool(&raw, i + 1)?;
                output_files = value.unwrap_or(true);
                i += consumed;
            }
            "outputdir" => {
                i += 1;
                output_dir = PathBuf::from(raw.get(i).ok_or("--OutputDir requires a value")?);
            }
            "outputname" => {
                i += 1;
                output_name = raw
                    .get(i)
                    .ok_or("--OutputName requires a value")?
                    .to_string();
            }
            "nooutputfiles" => output_files = false,
            _ if arg.starts_with('-') => return Err(format!("unknown argument: {arg}")),
            _ => {
                if target.is_some() {
                    return Err(format!("unexpected positional argument: {arg}"));
                }
                target = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    Ok(Args {
        target: target.ok_or("missing target path")?,
        deem,
        only_json,
        output_files,
        output_dir,
        output_name,
    })
}

fn split_assignment(arg: &str) -> Option<(&str, &str)> {
    let (key, value) = arg.split_once('=')?;
    Some((key.trim_start_matches('-'), value))
}

fn canonical_key(key: &str) -> String {
    key.trim_start_matches('-')
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-')
        .collect()
}

fn parse_deem(value: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid Deem value: {value}"))?;
    if (0.0..=100.0).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!("Deem must be between 0 and 100, got {value}"))
    }
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "y" | "on" => Ok(true),
        "false" | "f" | "0" | "no" | "n" | "off" => Ok(false),
        _ => Err(format!("invalid boolean value: {value}")),
    }
}

fn optional_bool(raw: &[String], index: usize) -> Result<(Option<bool>, usize), String> {
    let Some(value) = raw.get(index) else {
        return Ok((None, 0));
    };
    if value.starts_with('-') || value.contains('=') {
        return Ok((None, 0));
    }
    match parse_bool(value) {
        Ok(parsed) => Ok((Some(parsed), 1)),
        Err(_) => Ok((None, 0)),
    }
}

fn usage() -> String {
    format!(
        "{APP_NAME} <target> [--Deem <0-100>] [--OnlyJSON <true|false>] [--OutputFiles <true|false>] [--OutputDir <path>] [--OutputName <name>]"
    )
}

fn analyze(args: &Args) -> io::Result<Analysis> {
    let (ai_calibration_enabled, ai_profile) = env_profile("AI_CODE_DETECTOR_AI_CORPUS");
    let (human_calibration_enabled, human_profile) = env_profile("AI_CODE_DETECTOR_HUMAN_CORPUS");
    let loaded = load_source(&args.target)?;
    let mut file_stats = loaded.raw_file_stats.clone();
    let file_projects = loaded
        .lines
        .iter()
        .map(|line| (line.file.clone(), line.project.clone()))
        .collect::<HashMap<_, _>>();
    let project_stats = build_project_stats(&loaded.lines);
    merge_file_stats(&mut file_stats, build_file_stats(&loaded.lines));
    for (file, project) in file_projects {
        if let Some(stats) = project_stats.get(&project) {
            let entry = file_stats.entry(file).or_default();
            if entry.csharp && !entry.decompiled {
                add_file_stats(entry, stats);
            }
        }
    }
    let template_norms = repeated_template_lines(&loaded.lines);
    let mut excluded = loaded.excluded;
    let mut records = Vec::new();
    let mut file_scores: BTreeMap<String, (usize, usize, f64)> = BTreeMap::new();
    let mut total_score = 0.0;

    for line in loaded.lines {
        if is_low_information(&line.normalized) {
            excluded.push(ExcludedLine {
                file: line.file,
                line: line.line,
                kind: "low_information".to_string(),
                reason: "syntax-only or too little authorship signal".to_string(),
            });
            continue;
        }
        if template_norms.contains(&line.normalized) {
            excluded.push(ExcludedLine {
                file: line.file,
                line: line.line,
                kind: "template".to_string(),
                reason: "normalized line repeats across sibling projects and is treated as shared scaffold".to_string(),
            });
            continue;
        }

        let file_entry = file_scores.entry(line.file.clone()).or_default();
        let stats = file_stats.get(&line.file).cloned().unwrap_or_default();
        let (score, reason) = score_line(&line, &stats, &ai_profile, &human_profile);
        total_score += score;
        file_entry.0 += 1;
        file_entry.2 += score;
        records.push(LineRecord {
            file: line.file,
            line: line.line,
            excerpt: line.code,
            normalized: line.normalized,
            score,
            reason,
        });
    }

    for item in &excluded {
        file_scores.entry(item.file.clone()).or_default().1 += if item.line > 0 { 1 } else { 0 };
    }

    let considered = records.len();
    let percentage = if considered == 0 {
        0.0
    } else {
        (total_score / considered as f64) * 100.0
    };
    let is_ai = percentage + f64::EPSILON >= args.deem;
    let files = file_scores
        .into_iter()
        .map(|(path, (considered_lines, excluded_lines, score_sum))| {
            let stats = file_stats.get(&path).cloned().unwrap_or_default();
            let profile = decompiled_profile(&stats);
            let percentage = if considered_lines == 0 {
                0.0
            } else {
                (score_sum / considered_lines as f64) * 100.0
            };
            FileSummary {
                path,
                considered_lines,
                excluded_lines,
                percentage,
                is_ai: considered_lines > 0 && percentage + f64::EPSILON >= args.deem,
                decompiled: stats.decompiled,
                ai_features: profile.ai_features,
                legit_features: profile.legit_features,
                ai_positive: profile.ai_positive,
                strong_legit: profile.strong_legit,
                feature_summary: profile.summary,
            }
        })
        .collect();

    Ok(Analysis {
        target: target_label(&args.target),
        percentage,
        is_ai,
        deem: args.deem,
        considered_lines: considered,
        excluded_lines: excluded.len(),
        source_files: loaded.source_files,
        skipped_non_source_files: loaded.skipped_non_source,
        skipped_dirs: loaded.skipped_dirs,
        ai_calibration_enabled,
        human_calibration_enabled,
        ai_profile_lines: ai_profile.len(),
        human_profile_lines: human_profile.len(),
        template_lines: template_norms.len(),
        line_records: records,
        excluded,
        files,
    })
}

fn env_profile(name: &str) -> (bool, HashSet<String>) {
    let Ok(path) = env::var(name) else {
        return (false, HashSet::new());
    };
    if path.trim().is_empty() {
        return (false, HashSet::new());
    }
    (true, build_profile(Path::new(&path)))
}

fn target_label(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("scan-target")
        .to_string()
}

fn build_profile(root: &Path) -> HashSet<String> {
    let Ok(loaded) = load_source(root) else {
        return HashSet::new();
    };
    let templates = repeated_template_lines(&loaded.lines);
    loaded
        .lines
        .into_iter()
        .filter(|line| {
            !is_low_information(&line.normalized) && !templates.contains(&line.normalized)
        })
        .map(|line| line.normalized)
        .collect()
}

fn build_file_stats(lines: &[CodeLine]) -> HashMap<String, FileStats> {
    let mut stats = HashMap::<String, FileStats>::new();
    for line in lines {
        let entry = stats.entry(line.file.clone()).or_default();
        add_line_stats(entry, line);
    }
    stats
}

fn build_project_stats(lines: &[CodeLine]) -> HashMap<String, FileStats> {
    let mut stats = HashMap::<String, FileStats>::new();
    for line in lines {
        let entry = stats.entry(line.project.clone()).or_default();
        add_line_stats(entry, line);
    }
    stats
}

fn merge_file_stats(target: &mut HashMap<String, FileStats>, source: HashMap<String, FileStats>) {
    for (path, stats) in source {
        add_file_stats(target.entry(path).or_default(), &stats);
    }
}

fn add_line_stats(entry: &mut FileStats, line: &CodeLine) {
    entry.csharp |= line.csharp;
    entry.decompiled |= line.decompiled;
    entry.source_lines += 1;
    let lower = line.normalized.to_ascii_lowercase();
    if lower.starts_with("namespace ") {
        entry.namespaces += 1;
    }
    if lower.contains(" class ") || lower.starts_with("class ") || lower.contains("record ") {
        entry.classes += 1;
    }
    if lower.contains("config.bind") {
        entry.config_binds += 1;
    }
    if lower.contains("harmonypatch") {
        entry.harmony_patches += 1;
    }
    if lower.contains("bepinplugin") {
        entry.bep_in_plugin += 1;
    }
    if lower.contains("baseunityplugin") {
        entry.base_unity_plugin += 1;
    }
    if has_any(
        &lower,
        &[
            "accesstools.",
            "methodinfo",
            "fieldinfo",
            ".invoke(",
            "getfield(",
            "getmethod(",
        ],
    ) {
        entry.reflection_markers += 1;
    }
    if has_any(
        &lower,
        &[
            "configdescription(",
            "acceptablevaluerange",
            "log.logwarning(",
            "fallback",
            "restore",
            "reapply",
        ],
    ) {
        entry.generated_mod_markers += 1;
    }
}

fn add_file_stats(entry: &mut FileStats, stats: &FileStats) {
    entry.csharp |= stats.csharp;
    entry.decompiled |= stats.decompiled;
    entry.source_lines += stats.source_lines;
    entry.namespaces += stats.namespaces;
    entry.classes += stats.classes;
    entry.config_binds += stats.config_binds;
    entry.harmony_patches += stats.harmony_patches;
    entry.bep_in_plugin += stats.bep_in_plugin;
    entry.base_unity_plugin += stats.base_unity_plugin;
    entry.reflection_markers += stats.reflection_markers;
    entry.generated_mod_markers += stats.generated_mod_markers;
    entry.using_count += stats.using_count;
    entry.assembly_attributes += stats.assembly_attributes;
    entry.debug_metadata += stats.debug_metadata;
    entry.developer_debug_metadata += stats.developer_debug_metadata;
    entry.repository_metadata += stats.repository_metadata;
    entry.named_company_metadata += stats.named_company_metadata;
    entry.title_product_metadata += stats.title_product_metadata;
    entry.informational_version_metadata += stats.informational_version_metadata;
    entry.description_metadata += stats.description_metadata;
    entry.security_metadata += stats.security_metadata;
    entry.ignores_access_checks += stats.ignores_access_checks;
    entry.asset_bundle_markers += stats.asset_bundle_markers;
    entry.file_io_markers += stats.file_io_markers;
    entry.task_async_markers += stats.task_async_markers;
    entry.public_static_markers += stats.public_static_markers;
    entry.non_ascii_chars += stats.non_ascii_chars;
}

fn inspect_raw_file_stats(text: &str, csharp: bool, decompiled: bool) -> FileStats {
    let lower = text.to_ascii_lowercase();
    FileStats {
        csharp,
        decompiled,
        using_count: count_occurrences(&lower, "\nusing "),
        assembly_attributes: count_occurrences(&lower, "[assembly:"),
        debug_metadata: count_occurrences(&lower, "assemblyconfiguration(\"debug\")"),
        developer_debug_metadata: count_occurrences(&lower, "disableoptimizations")
            + count_occurrences(&lower, "enableeditandcontinue"),
        repository_metadata: count_occurrences(&lower, "repositoryurl"),
        named_company_metadata: named_assembly_value(text, "AssemblyCompany") as usize,
        title_product_metadata: (named_assembly_value(text, "AssemblyTitle") as usize)
            + (named_assembly_value(text, "AssemblyProduct") as usize),
        informational_version_metadata: count_occurrences(&lower, "assemblyinformationalversion"),
        description_metadata: named_assembly_value(text, "AssemblyDescription") as usize,
        security_metadata: count_occurrences(&lower, "securitypermission"),
        ignores_access_checks: count_occurrences(&lower, "ignoresaccesschecksto"),
        asset_bundle_markers: count_occurrences(&lower, "assetbundle"),
        file_io_markers: count_occurrences(&lower, "file.")
            + count_occurrences(&lower, "directory.")
            + count_occurrences(&lower, "path."),
        task_async_markers: count_occurrences(&lower, "async")
            + count_occurrences(&lower, "await")
            + count_occurrences(&lower, "task"),
        public_static_markers: count_occurrences(&lower, "public static"),
        non_ascii_chars: text.chars().filter(|ch| !ch.is_ascii()).count(),
        ..FileStats::default()
    }
}

fn count_occurrences(value: &str, needle: &str) -> usize {
    value.matches(needle).count()
}

fn named_assembly_value(text: &str, key: &str) -> bool {
    let prefix = format!("[assembly: {key}(\"");
    text.lines().any(|line| {
        let line = line.trim();
        line.starts_with(&prefix)
            && line
                .strip_prefix(&prefix)
                .and_then(|rest| rest.split_once("\")]"))
                .is_some_and(|(value, _)| !value.trim().is_empty())
    })
}

fn load_source(target: &Path) -> io::Result<LoadedSource> {
    let collection = collect_source_files(target)?;
    let root = if target.is_file() {
        target.parent().unwrap_or_else(|| Path::new(""))
    } else {
        target
    };
    let mut lines = Vec::new();
    let mut excluded = Vec::new();
    let mut raw_file_stats = HashMap::new();

    for file in &collection.source_files {
        let display = relative_display(root, file);
        let initial_kind = source_kind(file).unwrap_or("text");
        let Ok(text) = fs::read_to_string(file) else {
            excluded.push(ExcludedLine {
                file: display,
                line: 0,
                kind: "read_error".to_string(),
                reason: "file could not be read as UTF-8 text".to_string(),
            });
            continue;
        };
        let kind = if initial_kind == "blob" {
            if looks_decompiled_csharp(&text) {
                "c_family"
            } else {
                excluded.push(ExcludedLine {
                    file: display,
                    line: 0,
                    kind: "non_source_blob".to_string(),
                    reason: "blob file does not look like decompiled C# source".to_string(),
                });
                continue;
            }
        } else {
            initial_kind
        };
        let csharp = is_csharp_path(file);
        let decompiled = initial_kind == "blob" || (csharp && looks_decompiled_csharp(&text));
        raw_file_stats.insert(
            display.clone(),
            inspect_raw_file_stats(&text, csharp, decompiled),
        );

        let project = project_name(root, file);
        let mut block_end = None;
        let mut scaffold_namespace = false;
        let mut generated_block_depth = 0i32;
        let mut generated_block_pending = false;
        for (index, raw) in text.lines().enumerate() {
            let code = strip_comments(raw, syntax_for(kind), &mut block_end);
            let trimmed = code.trim();
            if trimmed.is_empty() {
                excluded.push(ExcludedLine {
                    file: display.clone(),
                    line: index + 1,
                    kind: if raw.trim().is_empty() {
                        "blank"
                    } else {
                        "comment"
                    }
                    .to_string(),
                    reason: if raw.trim().is_empty() {
                        "blank line"
                    } else {
                        "comment-only after stripping comments"
                    }
                    .to_string(),
                });
                continue;
            }
            let normalized = normalize_code(trimmed);
            if generated_block_pending || generated_block_depth > 0 {
                let delta = brace_delta(&normalized);
                if generated_block_pending && normalized.contains('{') {
                    generated_block_pending = false;
                    generated_block_depth = delta.max(1);
                } else if !generated_block_pending {
                    generated_block_depth += delta;
                }
                excluded.push(ExcludedLine {
                    file: display.clone(),
                    line: index + 1,
                    kind: "decompiler_scaffold".to_string(),
                    reason: "compiler generated closure or async/iterator state machine"
                        .to_string(),
                });
                if generated_block_depth <= 0 && !generated_block_pending {
                    generated_block_depth = 0;
                }
                continue;
            }
            if starts_generated_block(&normalized) {
                generated_block_pending = !normalized.contains('{');
                generated_block_depth = if generated_block_pending {
                    0
                } else {
                    brace_delta(&normalized).max(1)
                };
                excluded.push(ExcludedLine {
                    file: display.clone(),
                    line: index + 1,
                    kind: "decompiler_scaffold".to_string(),
                    reason: "compiler generated closure or async/iterator state machine"
                        .to_string(),
                });
                continue;
            }
            if let Some(reason) = decompiled_scaffold_kind(&normalized, &mut scaffold_namespace) {
                excluded.push(ExcludedLine {
                    file: display.clone(),
                    line: index + 1,
                    kind: "decompiler_scaffold".to_string(),
                    reason: reason.to_string(),
                });
                continue;
            }
            lines.push(CodeLine {
                file: display.clone(),
                project: project.clone(),
                line: index + 1,
                code: trimmed.to_string(),
                normalized,
                csharp,
                decompiled,
            });
        }
    }

    Ok(LoadedSource {
        lines,
        excluded,
        raw_file_stats,
        source_files: collection.source_files.len(),
        skipped_non_source: collection.skipped_non_source,
        skipped_dirs: collection.skipped_dirs,
    })
}

fn collect_source_files(target: &Path) -> io::Result<FileCollection> {
    let mut collection = FileCollection {
        source_files: Vec::new(),
        skipped_non_source: 0,
        skipped_dirs: 0,
    };
    if target.is_file() {
        if source_kind(target).is_some() {
            collection.source_files.push(target.to_path_buf());
        } else {
            collection.skipped_non_source = 1;
        }
        return Ok(collection);
    }
    visit_dir(target, &mut collection)?;
    collection.source_files.sort();
    Ok(collection)
}

fn visit_dir(dir: &Path, collection: &mut FileCollection) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if should_skip_dir(&path) {
                collection.skipped_dirs += 1;
            } else {
                visit_dir(&path, collection)?;
            }
        } else if source_kind(&path).is_some() {
            collection.source_files.push(path);
        } else {
            collection.skipped_non_source += 1;
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path
        .file_name()
        .and_then(OsStr::to_str)
        .map(|s| s.to_ascii_lowercase())
    else {
        return false;
    };
    matches!(
        name.as_str(),
        ".git"
            | ".hg"
            | ".svn"
            | "target"
            | "bin"
            | "obj"
            | "node_modules"
            | ".cache"
            | ".idea"
            | ".vs"
            | "artifacts"
    )
}

fn source_kind(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".lua.disabled") {
        return Some("lua");
    }
    match path
        .extension()?
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "cs" => Some("c_family"),
        "rs" => Some("c_family"),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Some("c_family"),
        "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" | "hh" | "hxx" => Some("c_family"),
        "java" | "go" | "kt" | "kts" | "swift" => Some("c_family"),
        "py" | "ps1" | "sh" | "rb" => Some("hash"),
        "lua" | "luau" => Some("lua"),
        "blob" => Some("blob"),
        _ => None,
    }
}

fn is_csharp_path(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            extension == "cs" || extension == "blob"
        })
        .unwrap_or(false)
}

fn syntax_for(kind: &str) -> Syntax {
    match kind {
        "lua" => Syntax {
            line_markers: &["--"],
            block_markers: &[("--[[", "]]")],
        },
        "hash" => Syntax {
            line_markers: &["#"],
            block_markers: &[],
        },
        "xml" => Syntax {
            line_markers: &[],
            block_markers: &[("<!--", "-->")],
        },
        "plain" => Syntax {
            line_markers: &[],
            block_markers: &[],
        },
        _ => Syntax {
            line_markers: &["//"],
            block_markers: &[("/*", "*/")],
        },
    }
}

fn looks_decompiled_csharp(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("[assembly:")
        && (lower.contains("using system;")
            || lower.contains("bepinplugin")
            || lower.contains(" class ")
            || lower.contains("targetframework")
            || lower.contains("[module:"))
}

fn decompiled_scaffold_kind(
    normalized: &str,
    scaffold_namespace: &mut bool,
) -> Option<&'static str> {
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with("namespace ") {
        *scaffold_namespace = lower.starts_with("namespace microsoft.codeanalysis")
            || lower.starts_with("namespace system.runtime.compilerservices");
        return (*scaffold_namespace).then_some("compiler support namespace emitted by decompiler");
    }
    if *scaffold_namespace
        && (lower.starts_with("[bepinplugin")
            || lower.starts_with("public enum ")
            || lower.starts_with("public class "))
    {
        *scaffold_namespace = false;
    }
    if *scaffold_namespace {
        return Some("compiler support namespace emitted by decompiler");
    }
    if lower.starts_with("[assembly:") || lower.starts_with("[module:") {
        return Some("assembly/module metadata emitted by decompiler");
    }
    if has_any(
        &lower,
        &[
            "[compilergenerated]",
            "[microsoft.codeanalysis.embedded]",
            "embeddedattribute",
            "nullableattribute",
            "nullablecontextattribute",
            "refsafetyrulesattribute",
            "unverifiablecode",
        ],
    ) {
        return Some("compiler generated attribute scaffold");
    }
    if has_any(
        &lower,
        &["<>c", "displayclass", ">d__", "iasyncstatemachine"],
    ) {
        return Some("compiler generated closure or async/iterator state machine");
    }
    if lower.contains("yield-return decompiler failed") {
        return Some("decompiler failure annotation");
    }
    None
}

fn starts_generated_block(normalized: &str) -> bool {
    let lower = normalized.to_ascii_lowercase();
    (lower.contains(" class <>c")
        || lower.contains(" class <")
        || lower.contains("displayclass")
        || lower.contains(">d__"))
        && (lower.contains("class") || lower.contains("struct"))
}

fn brace_delta(normalized: &str) -> i32 {
    normalized.chars().fold(0, |total, ch| match ch {
        '{' => total + 1,
        '}' => total - 1,
        _ => total,
    })
}

fn strip_comments(line: &str, syntax: Syntax, block_end: &mut Option<&'static str>) -> String {
    let mut out = String::new();
    let mut i = 0;
    let mut quote = None;
    let mut escaped = false;

    while i < line.len() {
        if let Some(end) = *block_end {
            if line[i..].starts_with(end) {
                *block_end = None;
                i += end.len();
            } else {
                i += line[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            }
            continue;
        }

        let current = line[i..].chars().next().unwrap();
        if let Some(active) = quote {
            out.push(current);
            i += current.len_utf8();
            if escaped {
                escaped = false;
            } else if current == '\\' {
                escaped = true;
            } else if current == active {
                quote = None;
            }
            continue;
        }

        if current == '"' || current == '\'' || current == '`' {
            quote = Some(current);
            out.push(current);
            i += current.len_utf8();
            continue;
        }

        if let Some((start, end)) = syntax
            .block_markers
            .iter()
            .find(|(start, _)| line[i..].starts_with(*start))
        {
            *block_end = Some(*end);
            i += start.len();
            continue;
        }

        if syntax
            .line_markers
            .iter()
            .any(|marker| line[i..].starts_with(marker))
        {
            break;
        }

        out.push(current);
        i += current.len_utf8();
    }

    out
}

fn normalize_code(code: &str) -> String {
    let mut normalized = String::new();
    let mut previous_space = false;
    for ch in code.trim().chars() {
        if ch.is_whitespace() {
            if !previous_space {
                normalized.push(' ');
                previous_space = true;
            }
        } else {
            normalized.push(ch);
            previous_space = false;
        }
    }
    normalized.trim().to_string()
}

fn has_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

fn is_low_information(normalized: &str) -> bool {
    let stripped = normalized.trim();
    stripped.len() <= 2
        || stripped.chars().all(|ch| "{}[]();,.=:".contains(ch))
        || matches!(
            stripped,
            "else" | "try" | "finally" | "do" | "end" | "{" | "}" | ");" | "};"
        )
}

fn repeated_template_lines(lines: &[CodeLine]) -> HashSet<String> {
    let projects = lines
        .iter()
        .map(|line| line.project.clone())
        .collect::<HashSet<_>>();
    if projects.len() < 3 {
        return HashSet::new();
    }
    let threshold = usize::max(3, ((projects.len() as f64) * 0.35).ceil() as usize);
    let mut by_line: HashMap<String, HashSet<String>> = HashMap::new();
    for line in lines {
        if !is_low_information(&line.normalized) {
            by_line
                .entry(line.normalized.clone())
                .or_default()
                .insert(line.project.clone());
        }
    }
    by_line
        .into_iter()
        .filter_map(|(line, seen)| (seen.len() >= threshold).then_some(line))
        .collect()
}

fn score_line(
    line: &CodeLine,
    stats: &FileStats,
    ai_profile: &HashSet<String>,
    human_profile: &HashSet<String>,
) -> (f64, String) {
    let profile = decompiled_profile(stats);
    let mut score = profile.base_score;
    let mut reasons = Vec::new();
    let lower = line.normalized.to_ascii_lowercase();
    let csharp_scoring = stats.csharp || stats.decompiled;
    let reflection_markers = [
        "accesstools.",
        "methodinfo",
        "fieldinfo",
        ".invoke(",
        "typebyname",
        "findobjectsbytype",
    ];
    let generated_markers = [
        "config.bind(",
        "configdescription(",
        "acceptablevaluerange",
        "log.logwarning(",
        "fallback",
        "restore",
        "reapply",
    ];
    let defensive_markers = [
        "tryparse",
        "catch (exception",
        "?.",
        "??",
        "== null",
        "!= null",
        "out var",
        "is not",
    ];

    if csharp_scoring
        && reflection_markers
            .iter()
            .any(|marker| lower.contains(marker))
    {
        score += if stats.decompiled && profile.strong_legit {
            0.005
        } else {
            0.35
        };
        reasons.push("reflection/runtime-discovery pattern");
    }
    if csharp_scoring
        && generated_markers
            .iter()
            .any(|marker| lower.contains(marker))
    {
        score += if stats.decompiled && profile.strong_legit {
            0.005
        } else {
            0.25
        };
        reasons.push("formulaic plugin/configuration pattern");
    }
    if csharp_scoring
        && defensive_markers
            .iter()
            .any(|marker| lower.contains(marker))
    {
        score += if stats.decompiled && profile.strong_legit {
            0.005
        } else {
            0.15
        };
        reasons.push("broad defensive generated-code pattern");
    }
    if csharp_scoring && line.normalized.len() > 100 {
        score += 0.1;
        reasons.push("long mechanically structured line");
    }
    if csharp_scoring
        && (lower.contains("mathf.")
            || lower.contains("rendersettings.")
            || lower.contains("unityobject."))
    {
        score += if stats.decompiled && profile.strong_legit {
            0.005
        } else {
            0.1
        };
        reasons.push("Unity API orchestration pattern");
    }
    if stats.decompiled || profile.ai_positive {
        reasons.push(&profile.summary);
    }
    if ai_profile.contains(&line.normalized) {
        reasons.push("calibrated generated-code match");
    }
    if human_profile.contains(&line.normalized) {
        reasons.push("calibrated authored-code match");
    }
    if reasons.is_empty() {
        reasons.push("unmatched line with weak AI evidence");
    }

    let cap = profile.cap;
    (score.min(cap), reasons.join("; "))
}

fn decompiled_profile(stats: &FileStats) -> DecompiledProfile {
    if !stats.decompiled {
        let standalone_mod = stats.bep_in_plugin > 0 || stats.base_unity_plugin > 0;
        if stats.csharp && standalone_mod {
            let mut ai_features = 4usize;
            let mut reasons = vec!["standalone C# plugin structure"];
            if stats.config_binds > 0 {
                ai_features += 2;
                reasons.push("configuration binding");
            }
            if stats.harmony_patches > 0 || stats.reflection_markers > 0 {
                ai_features += 2;
                reasons.push("Harmony/reflection patching");
            }
            if stats.generated_mod_markers > 0 {
                ai_features += 1;
                reasons.push("generated guard/config idioms");
            }
            return DecompiledProfile {
                ai_features,
                legit_features: 0,
                base_score: 1.0,
                cap: 1.0,
                ai_positive: true,
                strong_legit: false,
                summary: format!(
                    "generated C# plugin profile; ai_features={ai_features}; legit_features=0; {}",
                    reasons.join(", ")
                ),
            };
        }
        return DecompiledProfile {
            ai_features: 0,
            legit_features: 0,
            base_score: 0.0,
            cap: 0.0,
            ai_positive: false,
            strong_legit: false,
            summary: "normal source file profile".to_string(),
        };
    }

    let mut ai_features = 0usize;
    let mut legit_features = 0usize;
    let mut reasons = Vec::new();
    let has_legit_metadata = stats.debug_metadata > 0
        || stats.repository_metadata > 0
        || stats.security_metadata > 0
        || stats.ignores_access_checks > 0
        || stats.description_metadata > 0;
    let standalone_mod = stats.bep_in_plugin > 0 || stats.base_unity_plugin > 0;

    if stats.config_binds >= 8 {
        ai_features += 3;
        reasons.push("dense config binding");
    }
    if stats.harmony_patches + stats.reflection_markers >= 10 {
        ai_features += 3;
        reasons.push("dense Harmony/reflection patching");
    }
    if stats.config_binds >= 3 && stats.harmony_patches >= 3 {
        ai_features += 2;
        reasons.push("combined config and patch density");
    }
    if standalone_mod
        && !has_legit_metadata
        && (stats.config_binds > 0 || stats.harmony_patches > 0)
    {
        ai_features += 2;
        reasons.push("low-metadata standalone mod profile");
    }
    if stats.source_lines < 400
        && standalone_mod
        && !has_legit_metadata
        && (stats.config_binds > 0 || stats.harmony_patches > 0 || stats.reflection_markers > 0)
    {
        ai_features += 2;
        reasons.push("short utility-mod shape");
    }
    if stats.generated_mod_markers >= 6 {
        ai_features += 1;
        reasons.push("generated guard/config idioms");
    }

    if stats.repository_metadata > 0 {
        legit_features += 4;
        reasons.push("repository metadata");
    }
    if stats.debug_metadata > 0 {
        legit_features += 2;
        reasons.push("debug build metadata");
    }
    if stats.security_metadata > 0 || stats.ignores_access_checks > 0 {
        legit_features += 1;
        reasons.push("assembly access/security metadata");
    }
    if stats.description_metadata > 0 {
        legit_features += 1;
        reasons.push("descriptive assembly metadata");
    }
    if stats.named_company_metadata > 0 && has_legit_metadata {
        legit_features += 1;
        reasons.push("named author/company metadata");
    }
    if stats.developer_debug_metadata > 0
        && stats.title_product_metadata >= 2
        && stats.config_binds == 0
        && stats.reflection_markers == 0
        && stats.source_lines < 150
        && stats.harmony_patches <= 5
    {
        legit_features += 5;
        reasons.push("tiny developer-debug UI patch profile");
    }
    if stats.ignores_access_checks >= 10
        && stats.informational_version_metadata > 0
        && stats.named_company_metadata > 0
    {
        legit_features += 5;
        reasons.push("broad assembly access dependency surface");
    }
    if stats.named_company_metadata > 0
        && stats.title_product_metadata >= 2
        && stats.informational_version_metadata > 0
        && stats.source_lines < 150
        && stats.config_binds == 0
        && stats.harmony_patches == 0
        && stats.reflection_markers <= 3
    {
        legit_features += 5;
        reasons.push("compact release-metadata UI hook profile");
    }
    if stats.asset_bundle_markers > 0 {
        legit_features += 5;
        reasons.push("asset/library integration surface");
    }
    if stats.public_static_markers >= 20 {
        legit_features += 2;
        reasons.push("broad public API surface");
    }
    if stats.namespaces >= 4 || stats.classes >= 20 {
        legit_features += 2;
        reasons.push("multi-type architecture");
    }
    if stats.source_lines >= 1200 && stats.config_binds <= 2 && stats.namespaces >= 3 {
        legit_features += 3;
        reasons.push("large low-config library/application profile");
    }
    if stats.task_async_markers >= 10 && stats.file_io_markers >= 10 && stats.config_binds <= 4 {
        legit_features += 8;
        reasons.push("async file-workflow implementation");
    }
    if stats.named_company_metadata > 0
        && stats.source_lines >= 300
        && stats.config_binds <= 2
        && stats.harmony_patches <= 1
    {
        legit_features += 5;
        reasons.push("named low-config utility/library profile");
    }
    if stats.named_company_metadata > 0
        && stats.using_count >= 18
        && stats.source_lines >= 700
        && (stats.repository_metadata > 0
            || stats.description_metadata > 0
            || stats.asset_bundle_markers > 0)
    {
        legit_features += 2;
        reasons.push("broad dependency surface with named metadata");
    }

    let strong_legit = legit_features >= 5 && legit_features + 1 >= ai_features;
    let ai_positive = ai_features >= 4 && !strong_legit;
    let (base_score, cap) = if strong_legit {
        (0.01, 0.02)
    } else if ai_positive {
        (0.91, 0.98)
    } else if ai_features >= 2 {
        (0.90, 0.96)
    } else {
        (0.01, 0.02)
    };
    let class = if strong_legit {
        "legitimate decompiled profile cap"
    } else if ai_positive {
        "AI-positive decompiled profile"
    } else {
        "ambiguous decompiled profile"
    };

    DecompiledProfile {
        ai_features,
        legit_features,
        base_score,
        cap,
        ai_positive,
        strong_legit,
        summary: format!(
            "{class}; ai_features={ai_features}; legit_features={legit_features}; {}",
            reasons.join(", ")
        ),
    }
}

fn relative_display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn project_name(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    for component in rel.components() {
        if let Component::Normal(name) = component {
            return name.to_string_lossy().to_string();
        }
    }
    path.file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("root")
        .to_string()
}

fn render_json(analysis: &Analysis) -> String {
    let mut json = String::new();
    json.push_str("{\n");
    json.push_str(&format!(
        "  \"application\": {{\"name\":{},\"version\":{}}},\n",
        j(APP_NAME),
        j(APP_VERSION)
    ));
    json.push_str(&format!(
        "  \"data\": {{\"Percentage\":{},\"IsAI\":{}}},\n",
        number(analysis.percentage),
        analysis.is_ai
    ));
    json.push_str("  \"summary\": {");
    json.push_str(&format!(
        "\"target\":{},\"deem\":{},\"considered_lines\":{},\"excluded_lines\":{},\"source_files\":{},\"skipped_non_source_files\":{},\"skipped_dirs\":{},\"template_lines\":{}",
        j(&analysis.target),
        number(analysis.deem),
        analysis.considered_lines,
        analysis.excluded_lines,
        analysis.source_files,
        analysis.skipped_non_source_files,
        analysis.skipped_dirs,
        analysis.template_lines
    ));
    json.push_str("},\n");
    json.push_str(&format!(
        "  \"calibration\": {{\"ai_calibration_enabled\":{},\"human_calibration_enabled\":{},\"ai_profile_lines\":{},\"human_profile_lines\":{}}},\n",
        analysis.ai_calibration_enabled,
        analysis.human_calibration_enabled,
        analysis.ai_profile_lines,
        analysis.human_profile_lines
    ));
    json.push_str("  \"files\": [\n");
    for (index, file) in analysis.files.iter().enumerate() {
        comma(&mut json, index, 4);
        json.push_str(&format!(
            "{{\"path\":{},\"considered_lines\":{},\"excluded_lines\":{},\"Percentage\":{},\"IsAI\":{},\"decompiled\":{},\"ai_features\":{},\"legit_features\":{},\"ai_positive\":{},\"strong_legit\":{},\"feature_summary\":{}}}",
            j(&file.path),
            file.considered_lines,
            file.excluded_lines,
            number(file.percentage),
            file.is_ai,
            file.decompiled,
            file.ai_features,
            file.legit_features,
            file.ai_positive,
            file.strong_legit,
            j(&file.feature_summary)
        ));
    }
    json.push_str("\n  ],\n");
    json.push_str("  \"lines\": [\n");
    for (index, line) in analysis.line_records.iter().enumerate() {
        comma(&mut json, index, 4);
        json.push_str(&format!(
            "{{\"file\":{},\"line\":{},\"score\":{},\"excerpt\":{},\"normalized\":{},\"reason\":{}}}",
            j(&line.file),
            line.line,
            number(line.score),
            j(&line.excerpt),
            j(&line.normalized),
            j(&line.reason)
        ));
    }
    json.push_str("\n  ],\n");
    json.push_str("  \"excluded\": [\n");
    for (index, line) in analysis.excluded.iter().enumerate() {
        comma(&mut json, index, 4);
        json.push_str(&format!(
            "{{\"file\":{},\"line\":{},\"kind\":{},\"reason\":{}}}",
            j(&line.file),
            line.line,
            j(&line.kind),
            j(&line.reason)
        ));
    }
    json.push_str("\n  ]\n");
    json.push_str("}\n");
    json
}

fn comma(out: &mut String, index: usize, indent: usize) {
    if index > 0 {
        out.push_str(",\n");
    }
    out.push_str(&" ".repeat(indent));
}

fn j(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn number(value: f64) -> String {
    if (value - value.round()).abs() < 0.000_000_1 {
        format!("{:.1}", value)
    } else {
        format!("{:.3}", value)
    }
}

fn render_markdown(analysis: &Analysis) -> String {
    let mut out = String::new();
    out.push_str("# AI Code Detection Report\n\n");
    out.push_str("## Summary\n\n");
    out.push_str(&format!("- Target: `{}`\n", analysis.target));
    out.push_str(&format!(
        "- Percentage: `{}`\n",
        number(analysis.percentage)
    ));
    out.push_str(&format!("- Deem threshold: `{}`\n", number(analysis.deem)));
    out.push_str(&format!("- IsAI: `{}`\n", analysis.is_ai));
    out.push_str(&format!(
        "- Considered lines: `{}`\n",
        analysis.considered_lines
    ));
    out.push_str(&format!(
        "- Excluded lines: `{}`\n",
        analysis.excluded_lines
    ));
    out.push_str(&format!("- Source files: `{}`\n", analysis.source_files));
    out.push_str(&format!(
        "- Skipped non-source files: `{}`\n",
        analysis.skipped_non_source_files
    ));
    out.push_str(&format!(
        "- AI profile lines: `{}`\n",
        analysis.ai_profile_lines
    ));
    out.push_str(&format!(
        "- Human profile lines: `{}`\n",
        analysis.human_profile_lines
    ));
    out.push_str(&format!(
        "- Shared template lines detected: `{}`\n\n",
        analysis.template_lines
    ));
    out.push_str(&format!(
        "- AI calibration enabled: `{}`\n",
        analysis.ai_calibration_enabled
    ));
    out.push_str(&format!(
        "- Human calibration enabled: `{}`\n\n",
        analysis.human_calibration_enabled
    ));

    out.push_str("## File Summary\n\n");
    out.push_str("| File | Considered | Excluded | Percentage | IsAI | Features |\n");
    out.push_str("|---|---:|---:|---:|---|---|\n");
    for file in &analysis.files {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} |\n",
            escape_md(&file.path),
            file.considered_lines,
            file.excluded_lines,
            number(file.percentage),
            file.is_ai,
            escape_md(&file.feature_summary)
        ));
    }

    out.push_str("\n## Considered Lines\n\n");
    out.push_str("| File | Line | Score | Reason | Code |\n");
    out.push_str("|---|---:|---:|---|---|\n");
    for line in &analysis.line_records {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | `{}` |\n",
            escape_md(&line.file),
            line.line,
            number(line.score),
            escape_md(&line.reason),
            escape_code(&line.excerpt)
        ));
    }

    out.push_str("\n## Excluded Summary\n\n");
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for item in &analysis.excluded {
        *counts.entry(item.kind.as_str()).or_default() += 1;
    }
    out.push_str("| Kind | Count |\n");
    out.push_str("|---|---:|\n");
    for (kind, count) in counts {
        out.push_str(&format!("| `{}` | {} |\n", kind, count));
    }

    out
}

fn escape_md(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn escape_code(value: &str) -> String {
    escape_md(value).replace('`', "\\`")
}
