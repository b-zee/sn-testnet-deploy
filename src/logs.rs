use crate::error::{Error, Result};
use crate::s3::{S3Repository, S3RepositoryInterface};
use fs_extra::dir::{copy, remove, CopyOptions};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

pub async fn get_logs(name: &str) -> Result<()> {
    let dest_path = std::env::current_dir()?.join("logs").join(name);
    tokio::fs::create_dir_all(dest_path.clone()).await?;
    let s3_repository = S3Repository::new("sn-testnet");
    s3_repository
        .download_folder(&format!("testnet-logs/{name}"), &dest_path)
        .await?;
    Ok(())
}

pub async fn reassemble_logs(name: &str) -> Result<()> {
    let src = PathBuf::from(".").join("logs").join(name);
    if !src.exists() {
        return Err(Error::LogsNotRetrievedError(name.to_string()));
    }
    let dest = PathBuf::from(".")
        .join("logs")
        .join(format!("{name}-reassembled"));
    if dest.exists() {
        println!("Removing previous {name}-reassembled directory");
        remove(dest.clone())?;
    }

    std::fs::create_dir_all(&dest)?;
    let mut options = CopyOptions::new();
    options.overwrite = true;
    copy(src.clone(), dest.clone(), &options)?;

    visit_dirs(&dest, &process_part_files, &src, &dest)?;
    Ok(())
}

fn process_part_files(dir_path: &Path, source_root: &PathBuf, dest_root: &PathBuf) -> Result<()> {
    let reassembled_dir_path = if dir_path == dest_root {
        dest_root.clone()
    } else {
        dest_root.join(dir_path.strip_prefix(source_root)?)
    };
    std::fs::create_dir_all(&reassembled_dir_path)?;

    let entries: Vec<_> = std::fs::read_dir(dir_path)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, _>>()?;

    let mut part_files: Vec<_> = entries
        .iter()
        .filter(|path| path.is_file() && path.to_string_lossy().contains("part"))
        .collect();

    part_files.sort_by_key(|a| {
        a.file_name()
            .unwrap()
            .to_string_lossy()
            .split(".part")
            .nth(1)
            .unwrap_or("")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    if part_files.is_empty() {
        return Ok(());
    }

    let output_file_path = reassembled_dir_path.join("reassembled.log");
    println!("Creating reassembled file at {output_file_path:#?}");
    let mut output_file = File::create(&output_file_path)?;
    for part_file in part_files.iter() {
        let mut part_content = String::new();
        File::open(part_file)?.read_to_string(&mut part_content)?;

        // For some reason logstash writes "\n" as a literal string rather than a newline
        // character.
        part_content = part_content.replace("\\n", "\n");

        let mut cursor = Cursor::new(part_content);
        std::io::copy(&mut cursor, &mut output_file)?;
        std::fs::remove_file(part_file)?;
    }

    Ok(())
}

fn visit_dirs(
    dir: &Path,
    cb: &dyn Fn(&Path, &PathBuf, &PathBuf) -> Result<()>,
    source_root: &PathBuf,
    dest_root: &PathBuf,
) -> Result<()> {
    if dir.is_dir() {
        cb(dir, source_root, dest_root)?;
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                visit_dirs(&path, cb, dest_root, dest_root)?;
            }
        }
    }
    Ok(())
}