use anyhow;
use clap::{self, Parser};
use spatch::diff_parser::{DiffParser, Patch};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
};

#[derive(Clone, Debug)]
enum FileProcessing {
    ExtractPatch,
    ExtractFile,
}

#[derive(Clone, Debug)]
enum FilterType {
    Regex(regex::Regex),
    Glob(globset::Glob),
    OnlyNew(FileProcessing),
    OnlyRemoved(FileProcessing),
    None,
}

#[derive(Clone, Debug, clap::Args)]
#[group(multiple = false)]
struct AddedRemovedGroup {
    #[arg(long, short = 'n', help = "Only extract patches for newly added files")]
    #[arg(default_value_t = false)]
    #[arg(group = "added_removed")]
    only_new: bool,

    #[arg(long, short = 'r', help = "Only extract patches for removed files")]
    #[arg(default_value_t = false)]
    #[arg(group = "added_removed")]
    only_removed: bool,
}

#[derive(Clone, Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, short, help = "Output directory for split patches")]
    output_dir: Option<PathBuf>,

    #[clap(flatten)]
    added_removed: AddedRemovedGroup,

    #[arg(
        long,
        short = 'x',
        help = "Extract files contents rather than patches (requires either -n or -r)"
    )]
    #[arg(default_value_t = false)]
    #[arg(requires = "added_removed")]
    extract_file: bool,

    #[arg(long, help = "Filter patches by filename regex")]
    #[arg(conflicts_with = "glob")]
    #[arg(group = "filter")]
    #[arg(value_parser = regex::Regex::new)]
    regex: Option<regex::Regex>,

    #[arg(long, help = "Filter patches by filename glob pattern")]
    #[arg(conflicts_with = "regex")]
    #[arg(group = "filter")]
    #[arg(value_parser = globset::Glob::new)]
    glob: Option<globset::Glob>,

    #[arg(long, help = "Patch files to split. Reads from stdin if not specified")]
    #[arg(num_args = 1.., value_delimiter=' ')]
    files: Vec<PathBuf>,
}

fn should_skip_patch<T: Sized + Read>(patch: &Patch<T>, filter: &FilterType) -> bool {
    match filter {
        FilterType::None => false,
        FilterType::Glob(glob) => {
            let matcher = glob.compile_matcher();
            match (patch.old_filename(), patch.old_filename()) {
                (Some(a), Some(b)) => !(matcher.is_match(a) && matcher.is_match(b)),
                (Some(a), None) => !matcher.is_match(a),
                (None, Some(b)) => !matcher.is_match(b),
                (None, None) => unreachable!(),
            }
        }
        FilterType::Regex(expr) => match (patch.old_filename(), patch.old_filename()) {
            (Some(a), Some(b)) => !(expr.is_match(a) && expr.is_match(b)),
            (Some(a), None) => !expr.is_match(a),
            (None, Some(b)) => !expr.is_match(b),
            (None, None) => unreachable!(),
        },
        FilterType::OnlyNew(_) => patch.old_filename().is_none(),
        FilterType::OnlyRemoved(_) => patch.new_filename().is_none(),
    }
}

fn split_patch<T: Sized + Read>(
    handle: T,
    filter: &FilterType,
    patchfile: &String,
    output_dir: &PathBuf,
) -> anyhow::Result<()> {
    let parser = DiffParser::new(handle);

    parser
        .filter_map(|p| {
            if should_skip_patch(&p, filter) {
                return None;
            }

            let f = match filter {
                FilterType::OnlyRemoved(FileProcessing::ExtractFile) => PathBuf::from(
                    p.old_filename().as_ref()
                        .expect("(extremely invalid patch) cannot extract removed file because old filename was /dev/null"),
                ),
                FilterType::OnlyNew(FileProcessing::ExtractFile) => PathBuf::from(
                    p.new_filename().as_ref()
                        .expect("(extremely invalid patch) cannot extract added file because new filename was /dev/null"),
                ),
                _ => {
                    let new_name = match (p.old_filename(), p.new_filename()) {
                        (_, Some(b)) => b,
                        (Some(a), _) => a,
                        _ => unreachable!("(extremely invalid patch) cannot have both old and new filenames /dev/null")
                    }.replace("/", "-");

                    PathBuf::from(if patchfile.is_empty() {
                        new_name
                    } else {
                        format!("{}+{}", new_name, patchfile)
                    })
                    .with_added_extension("patch")
                }
            };

            Some((output_dir.join(f), p))
        })
        .try_for_each(|(f, mut patch)| {
            let dirname = f.parent().ok_or(anyhow::anyhow!(
                "could not find parent of '{}'",
                f.display()
            ))?;
            if !dirname.exists() {
                std::fs::create_dir_all(dirname)?;
            }

            let mut file_patch = File::create(f)?;

            match filter {
                FilterType::OnlyNew(FileProcessing::ExtractFile) => {}
                _ => file_patch.write_all(patch.header().as_bytes())?,
            };

            patch
                .lines()
                .filter_map(|line| match filter {
                    FilterType::OnlyNew(FileProcessing::ExtractFile) => {
                        if line.starts_with("@@ -") {
                            None
                        } else {
                            Some(line[1..].to_string())
                        }
                    }
                    _ => Some(line),
                })
                .try_for_each(|line| -> anyhow::Result<()> {
                    file_patch
                        .write_all(format!("{}\n", line).as_bytes())
                        .map_err(|e| anyhow::Error::from(e))
                })
        })
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let filter = if args.added_removed.only_new {
        FilterType::OnlyNew(if args.extract_file {
            FileProcessing::ExtractFile
        } else {
            FileProcessing::ExtractPatch
        })
    } else if args.added_removed.only_new {
        FilterType::OnlyRemoved(if args.extract_file {
            FileProcessing::ExtractFile
        } else {
            FileProcessing::ExtractPatch
        })
    } else if let Some(glob) = args.glob {
        FilterType::Glob(glob)
    } else if let Some(expr) = args.regex {
        FilterType::Regex(expr)
    } else {
        FilterType::None
    };

    let output = args.output_dir.unwrap_or(std::env::current_dir()?);

    if !output.is_dir() {
        return Err(anyhow::anyhow!(
            "Output path {} is not a directory",
            output.display()
        ));
    }

    if !args.files.is_empty() {
        args.files
            .iter()
            .try_for_each(|path| -> anyhow::Result<()> {
                if !path.is_file() {
                    return Err(anyhow::anyhow!("{} is not a file", path.display()));
                }

                println!("Splitting {}", path.display());
                split_patch(
                    File::open(path)?,
                    &filter,
                    &path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into(),
                    &output,
                )
            })
    } else {
        split_patch(io::stdin().lock(), &filter, &String::new(), &output)
    }
}
