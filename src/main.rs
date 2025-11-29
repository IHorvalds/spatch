use spatch::diff_parser::{DiffParser, Patch};
use anyhow;
use clap::{self, Parser};
use std::{fs::File, io::{self, Write}, path::PathBuf};

#[derive(Clone, Debug)]
enum NewFileProcessing {
    ExtractPatch,
    ExtractFile,
}

#[derive(Clone, Debug)]
enum FilterType {
    Regex(regex::Regex),
    Glob(globset::Glob),
    OnlyNew(NewFileProcessing),
    None
}

#[derive(Clone, Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, short, help="Output directory for split patches")]
    output_dir: Option<PathBuf>,

    #[arg(long, short = 'n', help="Only extract patches for newly added files")]
    #[arg(default_value_t = false)]
    only_new: bool,

    #[arg(long, short = 'x', help="Extract new files contents rather than patches")]
    #[arg(default_value_t = false)]
    #[arg(requires = "only_new")]
    extract_file: bool,

    #[arg(long, help="Filter patches by filename regex")]
    #[arg(conflicts_with = "glob")]
    #[arg(group = "filter")]
    #[arg(value_parser = regex::Regex::new)]
    regex: Option<regex::Regex>,

    #[arg(long, help="Filter patches by filename glob pattern")]
    #[arg(conflicts_with = "regex")]
    #[arg(group = "filter")]
    #[arg(value_parser = globset::Glob::new)]
    glob: Option<globset::Glob>,

    #[arg(long, help = "Patch files to split. Reads from stdin if not specified")]
    #[arg(num_args = 1.., value_delimiter=' ')]
    files: Vec<PathBuf>,
}

fn should_skip_patch<T: io::Read>(patch: &Patch<T>, filter: &FilterType) -> anyhow::Result<bool> {
    let res = match filter {
        FilterType::None => false,
        FilterType::Glob(glob) => {
            let matcher = glob.compile_matcher();
            !matcher.is_match(&patch.new_filename()) 
        }
        FilterType::Regex(expr) => {
            !expr.is_match(patch.new_filename())
        }
        FilterType::OnlyNew(_) => patch.old_filename() != "/dev/null",
    };

    Ok(res)
}

fn split_patch<T: io::Read>(handle: T, filter: &FilterType, patchfile: &String, output_dir: &PathBuf) -> anyhow::Result<()> {

    let mut parser = DiffParser::new(handle);

    while let Some(mut patch) = parser.next_patch() {

        if should_skip_patch(&patch, filter)? {
            continue;
        }

        let filename = match filter {
            FilterType::OnlyNew(NewFileProcessing::ExtractFile) => PathBuf::from(patch.new_filename()),
            _ => {
                let new_name = patch.new_filename().replace("/", "-");
                PathBuf::from(if patchfile.is_empty() {
                  new_name
                } else {
                    format!("{}+{}", new_name, patchfile)
                }).with_added_extension("patch")
            }
        };

        let full_path = output_dir.join(&filename);
        let dirname = full_path.parent().ok_or(anyhow::anyhow!("could not find parent of '{}'", full_path.display()))?;

        if !dirname.exists() {
            std::fs::create_dir_all(dirname)?;
        }

        let mut f = File::create(full_path)?;

        match filter {
            FilterType::OnlyNew(NewFileProcessing::ExtractFile) => {},
            _ => {
                f.write_all(patch.header().as_bytes())?
            }
        };

        patch
        .lines()
        .filter_map(|line| match filter {
            FilterType::OnlyNew(NewFileProcessing::ExtractFile) => {
                if line.starts_with("@@ -") {
                    None
                } else {
                    Some(line[1..].to_string())
                }
            }
            _ => Some(line)
        })
        .try_for_each(|line| {
            f.write_all(format!("{}\n", line).as_bytes())
        })?;
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let filter = if args.only_new {
        FilterType::OnlyNew(if args.extract_file {NewFileProcessing::ExtractFile} else {NewFileProcessing::ExtractPatch})
    } else if let Some(glob) = args.glob {
        FilterType::Glob(glob)
    } else if let Some(expr) = args.regex {
        FilterType::Regex(expr)
    } else {
        FilterType::None
    };
    
    let output = args.output_dir.unwrap_or(std::env::current_dir()?);

    if !output.is_dir() {
        return Err(anyhow::anyhow!("Output path {} is not a directory", output.display()));
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
                &path.file_stem().unwrap_or_default().to_string_lossy().into(),
                &output)
            })
    } else {
        split_patch(io::stdin().lock(), &filter, &String::new(),  &output)
    }
}