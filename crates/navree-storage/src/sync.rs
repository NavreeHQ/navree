//! Reconciling two disks: [`Storage::sync`] and the `storage:sync` command.

use std::collections::{BTreeMap, BTreeSet};

use futures_util::{StreamExt, stream};
use navree_kernel::{AppContext, BoxError, CommandSpec};
use serde::Serialize;

use crate::{Disk, Storage, StorageError};

/// Files copied at the same time by [`Disk::sync_to`].
const SYNC_CONCURRENCY: usize = 4;

/// The directory of the readiness probe's files, never synced.
const HEALTH_DIR: &str = ".navree-health/";

/// What [`Storage::sync`] does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncOptions {
    /// Only files under this directory (default: the whole disk).
    pub prefix: String,
    /// Also delete the target's files that the source does not have.
    pub delete: bool,
    /// Work out what would change, and change nothing.
    pub dry_run: bool,
}

/// What [`Storage::sync`] did (or, in a dry run, would do).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    /// The source disk.
    pub from: String,
    /// The target disk.
    pub to: String,
    /// Whether nothing was changed.
    pub dry_run: bool,
    /// Files copied to the target: missing there, or of another size.
    pub copied: Vec<String>,
    /// Files deleted from the target (with [`SyncOptions::delete`]).
    pub deleted: Vec<String>,
    /// Files already on the target with the same size.
    pub unchanged: usize,
    /// Bytes copied (or to copy).
    pub bytes: u64,
    /// Files that could not be copied or deleted.
    pub failed: Vec<SyncFailure>,
}

/// A file [`Storage::sync`] could not copy or delete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SyncFailure {
    /// The file's path.
    pub path: String,
    /// What went wrong.
    pub error: String,
}

impl Storage {
    /// Makes disk `to` hold the files of disk `from` (under
    /// `options.prefix`): every file that is missing on `to`, or there with
    /// another size, is streamed over; with `options.delete`, the files of
    /// `to` that `from` does not have are deleted. Files are compared by
    /// path and size, so a file changed in place without changing size is
    /// not copied again.
    ///
    /// A file that fails is listed in [`SyncReport::failed`] and the sync
    /// goes on; listing either disk failing is an error.
    pub async fn sync(
        &self,
        from: &str,
        to: &str,
        options: &SyncOptions,
    ) -> Result<SyncReport, StorageError> {
        self.disk(from)?.sync_to(&self.disk(to)?, options).await
    }
}

impl Disk {
    /// Makes `target` hold the files of this disk; see [`Storage::sync`].
    pub async fn sync_to(
        &self,
        target: &Disk,
        options: &SyncOptions,
    ) -> Result<SyncReport, StorageError> {
        if self.name() == target.name() {
            return Err(StorageError::InvalidOptions(format!(
                "cannot sync disk `{}` to itself",
                self.name()
            )));
        }
        let synced = |path: &String| !path.starts_with(HEALTH_DIR);
        let source = self.files(&options.prefix).await?;
        let existing: BTreeMap<String, u64> = target
            .files(&options.prefix)
            .await?
            .into_iter()
            .filter(|file| synced(&file.path))
            .map(|file| (file.path, file.size))
            .collect();

        let mut report = SyncReport {
            from: self.name().to_owned(),
            to: target.name().to_owned(),
            dry_run: options.dry_run,
            ..SyncReport::default()
        };
        let mut to_copy = Vec::new();
        let mut in_source = BTreeSet::new();
        for file in source.into_iter().filter(|file| synced(&file.path)) {
            in_source.insert(file.path.clone());
            if existing.get(&file.path) == Some(&file.size) {
                report.unchanged += 1;
            } else {
                to_copy.push(file);
            }
        }
        let to_delete: Vec<String> = if options.delete {
            existing
                .into_keys()
                .filter(|path| !in_source.contains(path))
                .collect()
        } else {
            Vec::new()
        };

        if options.dry_run {
            report.bytes = to_copy.iter().map(|file| file.size).sum();
            report.copied = to_copy.into_iter().map(|file| file.path).collect();
            report.deleted = to_delete;
            return Ok(report);
        }

        let copies: Vec<(String, Result<u64, StorageError>)> = stream::iter(to_copy)
            .map(|file| async move {
                let result = async {
                    let contents = self.get_stream(&file.path).await?;
                    target.put_stream(&file.path, contents).await
                }
                .await;
                (file.path, result)
            })
            .buffer_unordered(SYNC_CONCURRENCY)
            .collect()
            .await;
        for (path, result) in copies {
            match result {
                Ok(bytes) => {
                    report.bytes += bytes;
                    report.copied.push(path);
                }
                Err(error) => report.failed.push(SyncFailure {
                    path,
                    error: error.to_string(),
                }),
            }
        }
        for path in to_delete {
            match target.delete(&path).await {
                Ok(()) => report.deleted.push(path),
                Err(error) => report.failed.push(SyncFailure {
                    path,
                    error: error.to_string(),
                }),
            }
        }
        report.copied.sort();
        report.failed.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(report)
    }
}

/// `storage:sync`.
pub(crate) fn command() -> CommandSpec {
    CommandSpec::new(
        "storage:sync",
        "Copy the files one disk lacks from another: storage:sync <from> <to> [--prefix <dir>] [--delete] [--dry-run]",
        |ctx, args| async move { sync_command(ctx, args).await },
    )
}

async fn sync_command(ctx: AppContext, args: Vec<String>) -> Result<(), BoxError> {
    let (from, to, options) = parse_sync(&args)?;
    let storage = ctx.get::<Storage>().await?;
    let report = storage.sync(&from, &to, &options).await?;
    println!("{}", render(&report));
    if report.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "storage:sync: {} file{} failed",
            report.failed.len(),
            plural(report.failed.len())
        )
        .into())
    }
}

/// `<from> <to> [--prefix <dir>] [--delete] [--dry-run]`.
pub(crate) fn parse_sync(args: &[String]) -> Result<(String, String, SyncOptions), String> {
    let usage = "usage: storage:sync <from> <to> [--prefix <dir>] [--delete] [--dry-run]";
    let mut options = SyncOptions::default();
    let mut disks = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--delete" => options.delete = true,
            "--dry-run" => options.dry_run = true,
            "--prefix" => {
                options.prefix = args
                    .next()
                    .ok_or_else(|| format!("storage:sync: --prefix needs a directory; {usage}"))?
                    .clone();
            }
            other => match other.strip_prefix("--prefix=") {
                Some(prefix) => options.prefix = prefix.to_owned(),
                None if other.starts_with("--") => {
                    return Err(format!("storage:sync: unknown option `{other}`; {usage}"));
                }
                None => disks.push(other.to_owned()),
            },
        }
    }
    match <[String; 2]>::try_from(disks) {
        Ok([from, to]) => Ok((from, to, options)),
        Err(_) => Err(format!("storage:sync: name two disks; {usage}")),
    }
}

/// The command's output.
pub(crate) fn render(report: &SyncReport) -> String {
    let mut lines = vec![if report.dry_run {
        format!(
            "Syncing `{}` to `{}` (dry run: nothing is changed)",
            report.from, report.to
        )
    } else {
        format!("Syncing `{}` to `{}`", report.from, report.to)
    }];
    let copy = if report.dry_run { "copy" } else { "copied" };
    let delete = if report.dry_run { "delete" } else { "deleted" };
    lines.extend(
        report
            .copied
            .iter()
            .map(|path| format!("  {copy:<8} {path}")),
    );
    lines.extend(
        report
            .deleted
            .iter()
            .map(|path| format!("  {delete:<8} {path}")),
    );
    lines.extend(
        report
            .failed
            .iter()
            .map(|failure| format!("  {:<8} {}: {}", "failed", failure.path, failure.error)),
    );
    let mut summary = if report.dry_run {
        format!(
            "{} to copy ({} bytes), {} to delete, {} unchanged",
            report.copied.len(),
            report.bytes,
            report.deleted.len(),
            report.unchanged
        )
    } else {
        format!(
            "{} copied ({} bytes), {} deleted, {} unchanged",
            report.copied.len(),
            report.bytes,
            report.deleted.len(),
            report.unchanged
        )
    };
    if !report.failed.is_empty() {
        summary.push_str(&format!(", {} failed", report.failed.len()));
    }
    lines.push(format!("{summary}."));
    lines.join("\n")
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn parses_disks_and_options() {
        let (from, to, options) = parse_sync(&args(&["local", "s3"])).unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("local", "s3"));
        assert_eq!(options, SyncOptions::default());

        let (_, _, options) = parse_sync(&args(&[
            "--dry-run",
            "local",
            "--prefix",
            "avatars",
            "s3",
            "--delete",
        ]))
        .unwrap();
        assert_eq!(
            options,
            SyncOptions {
                prefix: "avatars".into(),
                delete: true,
                dry_run: true,
            }
        );
        let (_, _, options) = parse_sync(&args(&["a", "b", "--prefix=reports/2026"])).unwrap();
        assert_eq!(options.prefix, "reports/2026");
    }

    #[test]
    fn rejects_bad_arguments() {
        for bad in [
            &["local"][..],
            &["a", "b", "c"],
            &["a", "b", "--force"],
            &["a", "b", "--prefix"],
            &[],
        ] {
            let error = parse_sync(&args(bad)).unwrap_err();
            assert!(error.starts_with("storage:sync: "), "{bad:?}: {error}");
            assert!(error.contains("usage: storage:sync"), "{error}");
        }
    }

    #[test]
    fn renders_a_dry_run_and_a_run() {
        let mut report = SyncReport {
            from: "local".into(),
            to: "uploads".into(),
            dry_run: true,
            copied: vec!["a.txt".into(), "b/c.png".into()],
            deleted: vec!["old.txt".into()],
            unchanged: 7,
            bytes: 42,
            failed: vec![],
        };
        assert_eq!(
            render(&report),
            "Syncing `local` to `uploads` (dry run: nothing is changed)\n  \
             copy     a.txt\n  copy     b/c.png\n  delete   old.txt\n\
             2 to copy (42 bytes), 1 to delete, 7 unchanged."
        );
        report.dry_run = false;
        report.failed.push(SyncFailure {
            path: "x.bin".into(),
            error: "boom".into(),
        });
        assert_eq!(
            render(&report),
            "Syncing `local` to `uploads`\n  copied   a.txt\n  copied   b/c.png\n  \
             deleted  old.txt\n  failed   x.bin: boom\n\
             2 copied (42 bytes), 1 deleted, 7 unchanged, 1 failed."
        );
    }
}
