mod builds;
mod releases;

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use builds::BuildSpecification;
use builds::Generation;
use indexmap::IndexMap;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use ripunzip::UnzipOptions;

use crate::builds::get_download_uri;

fn main() -> Result<()> {
    let os = std::env::consts::OS;
    let platform = match os {
        "macos" => "mac",
        "windows" => "win64",
        _ => os,
    };

    let specification = BuildSpecification {
        build_type: "asan",
        platform,
        debugness: "release",
    };

    println!("Fetching branch information");
    let channels = releases::get_channel_branch_positions()?;
    // Sometimes several channels can relate to the same branch, especially
    // for stable & extended stable. Aggregate them.
    let mut downloads: IndexMap<u64, Vec<String>> = IndexMap::new();
    for channel in channels.into_iter() {
        let desc = format!("{}-{}", channel.0, channel.1.milestone);
        downloads
            .entry(channel.1.chromium_main_branch_position)
            .and_modify(|v| v.push(desc.clone()))
            .or_insert(vec![desc]);
    }
    println!(
        "Downloads we need to do: {:?}. Investigating available builds.",
        downloads
    );

    let errors: Vec<_> = downloads
        .into_par_iter()
        .map(|(branch_point, channel_description)| {
            fetch_build(&specification, branch_point, &channel_description)
        })
        .filter_map(Result::err)
        .collect();

    // Output any errors we found on any file
    for error in &errors {
        eprintln!("Error: {}", error)
    }
    // Return the first error code, if any.
    errors.into_iter().next().map(Result::Err).unwrap_or(Ok(()))
}

fn fetch_build(
    specification: &BuildSpecification,
    branch_point: u64,
    channel_descriptions: &[String],
) -> Result<()> {
    // Find the build immediately before the branch point.
    let build = find_a_build_just_before(specification, branch_point)?;
    let uri = get_download_uri(specification, &build);
    println!(
        "Channel {:?}: branch point was {}, downloading build {:?} from {}",
        channel_descriptions, branch_point, build, uri
    );
    let concatenated_descriptions = channel_descriptions.join("_");
    let result = ripunzip::unzip_uri(
        &uri,
        &UnzipOptions {
            output_directory: Some(PathBuf::from(concatenated_descriptions)),
        },
    );
    println!("Completed download from {}.", uri);
    result
}

fn find_a_build_just_before(
    specification: &BuildSpecification,
    branch_point: u64,
) -> Result<(u64, Generation)> {
    // The build listing takes a version prefix, which we want to be as precise as possible,
    // to be quick and because there's a maximum result count. We'll take it digit by digit
    // and keep searching outwards until we find one which is at or below the intended branch
    // point.
    let branch_point_string = format!("{}", branch_point);

    for prefix_length in (0..branch_point_string.len()).rev() {
        if let Ok(builds) =
            builds::get_builds(specification, &branch_point_string[0..prefix_length])
        {
            let the_build = builds
                .into_iter()
                .filter(|build| build.0 <= branch_point)
                .max_by_key(|build| build.0);
            if let Some(the_build) = the_build {
                return Ok(the_build);
            }
        }
    }
    Err(anyhow!("No matching builds found"))
}
