use color_eyre::Help;
use eyre::{Context, Result};
use ignore::WalkBuilder;
use itertools::Itertools;
use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::Cli;

#[derive(Debug)]
pub enum WriteMode {
    ToFile,
    DryRun,
    ToConsole,
    ToStdOut,
    CheckFormatted,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum CustomRegexEntryInput {
    String(String),
    Pair((String, String)),
}

pub type RegexPair = (Regex, Option<Regex>);

#[derive(Debug)]
pub enum FinderRegex {
    DefaultRegex,
    CustomRegex(Regex),
    CustomRegexEntries(Vec<RegexPair>),
}

#[derive(Debug)]
pub enum Sorter {
    DefaultSorter,
    CustomSorter(HashMap<String, usize>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfigFileContents {
    sort_order: Option<Vec<String>>,
    custom_regex: Option<Vec<CustomRegexEntryInput>>,
}

#[derive(Debug)]
pub struct Options {
    pub stdin: Option<String>,
    pub write_mode: WriteMode,
    pub regex: FinderRegex,
    pub sorter: Sorter,
    pub starting_paths: Vec<PathBuf>,
    pub allow_duplicates: bool,
    pub search_paths: Vec<PathBuf>,
    pub ignored_files: HashSet<PathBuf>,
}

impl Options {
    pub fn new_from_cli(cli: Cli) -> Result<Options> {
        let stdin = if cli.stdin {
            let mut buffer = String::new();
            let mut stdin = std::io::stdin(); // We get `Stdin` here.
            stdin.read_to_string(&mut buffer).unwrap();
            Some(buffer.to_string())
        } else {
            None
        };

        let starting_paths = get_starting_path_from_cli(&cli);
        let search_paths = get_search_paths_from_starting_paths(&starting_paths);
        let cli_regex = get_custom_regex_from_cli(&cli)?;
        let (sorter, config_regex) = get_options_from_config(&cli)?;

        Ok(Options {
            stdin,
            starting_paths,
            search_paths,
            write_mode: get_write_mode_from_cli(&cli),
            regex: match cli_regex {
                // if custom regex is received from the CLI, it takes highest priority
                FinderRegex::CustomRegex(_) => cli_regex,
                // if no regex was received from the CLI, check if regex was supplied in config file
                FinderRegex::DefaultRegex => match config_regex {
                    Some(entries) => FinderRegex::CustomRegexEntries(entries),
                    None => FinderRegex::DefaultRegex,
                },
                // It's not currently possible to pass in nested entry arrays from the CLI
                FinderRegex::CustomRegexEntries(_) => unreachable!(),
            },
            sorter,
            allow_duplicates: cli.allow_duplicates,
            ignored_files: get_ignored_files_from_cli(&cli),
        })
    }
}

fn get_options_from_config(cli: &Cli) -> Result<(Sorter, Option<Vec<RegexPair>>)> {
    match &cli.config_file {
        Some(config_file) => {
            let file_contents = fs::read_to_string(config_file)
                .wrap_err_with(|| format!("Error reading the config file {config_file}"))
                .with_suggestion(|| format!("Make sure the file {config_file} exists"));

            let config_file: ConfigFileContents = serde_json::from_str(&file_contents?)
                .wrap_err_with(|| format!("Error while parsing the config file {config_file}"))
                .with_suggestion(|| format!("Make sure the config_file {config_file} is valid json with the expected format"))?;

            Ok((
                config_file
                    .sort_order
                    .map_or(Sorter::DefaultSorter, parse_custom_sorter),
                config_file.custom_regex.map(parse_custom_regex),
            ))
        }
        None => Ok((Sorter::DefaultSorter, None)),
    }
}

fn get_custom_regex_from_cli(cli: &Cli) -> Result<FinderRegex> {
    match &cli.custom_regex {
        Some(regex_string) => {
            let regex = Regex::new(regex_string).wrap_err("Unable to parse custom regex")?;

            if regex.captures_len() < 2 {
                eyre::bail!("custom regex error, requires at-least 2 capture groups");
            }

            Ok(FinderRegex::CustomRegex(regex))
        }
        None => Ok(FinderRegex::DefaultRegex),
    }
}

fn get_starting_path_from_cli(cli: &Cli) -> Vec<PathBuf> {
    cli.file_or_dir
        .iter()
        .map(|path| Path::new(path).to_owned())
        .collect()
}

fn get_write_mode_from_cli(cli: &Cli) -> WriteMode {
    if cli.dry_run {
        WriteMode::DryRun
    } else if cli.write {
        WriteMode::ToFile
    } else if cli.check_formatted {
        WriteMode::CheckFormatted
    } else if cli.stdin {
        WriteMode::ToStdOut
    } else {
        WriteMode::DryRun
    }
}

fn get_search_paths_from_starting_paths(starting_paths: &[PathBuf]) -> Vec<PathBuf> {
    starting_paths
        .iter()
        .flat_map(|starting_path| {
            WalkBuilder::new(starting_path)
                .build()
                .filter_map(Result::ok)
                .filter(|f| f.path().is_file())
                .map(|file| file.path().to_owned())
        })
        .unique()
        .collect()
}

fn get_ignored_files_from_cli(cli: &Cli) -> HashSet<PathBuf> {
    match &cli.ignored_files {
        Some(ignored_files) => ignored_files
            .iter()
            .map(|string| PathBuf::from_str(string))
            .filter_map(Result::ok)
            .map(std::fs::canonicalize)
            .filter_map(Result::ok)
            .collect(),
        None => HashSet::new(),
    }
}

fn parse_custom_sorter(contents: Vec<String>) -> Sorter {
    Sorter::CustomSorter(
        contents
            .into_iter()
            .enumerate()
            .map(|(index, class)| (class, index))
            .collect(),
    )
}

fn parse_custom_regex(entries: Vec<CustomRegexEntryInput>) -> Vec<RegexPair> {
    entries.iter().for_each(|entry| {
        match entry {
            CustomRegexEntryInput::String(container_string) => {
                println!("{}", container_string);
            }
            CustomRegexEntryInput::Pair((container_string, classes_string)) => {
                println!("[{}, {}]", container_string, classes_string);
            }
        };
    });

    entries
        .into_iter()
        .map(|entry| match entry {
            CustomRegexEntryInput::Pair((container_regex_string, class_regex_string)) => (
                Regex::new(&container_regex_string).unwrap(),
                Some(Regex::new(&class_regex_string).unwrap()),
            ),

            CustomRegexEntryInput::String(container_regex_string) => {
                (Regex::new(&container_regex_string).unwrap(), None)
            }
        })
        .collect()
}
