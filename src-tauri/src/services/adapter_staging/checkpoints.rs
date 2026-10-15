//! D49: immutable recovery snapshots, independent of mutable training output.
//! Cost: one retained copy per save; escape hatch: disable saves by a larger interval.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub iter: u32,
    pub manifest_sha256: String,
}

fn snapshot(attempt: &Attempt, step: u32) -> PathBuf {
    attempt.dir().join("checkpoints").join(step.to_string())
}

fn verified_snapshot(attempt: &Attempt, checkpoint: &Checkpoint) -> Result<PathBuf> {
    ensure_real_dir(&attempt.dir(), false)?;
    ensure_real_dir(&attempt.dir().join("checkpoints"), false)?;
    let dir = snapshot(attempt, checkpoint.iter);
    ensure_real_dir(&dir, false)?;
    scan_out(&dir)?;
    if sha256_file(&dir.join(MANIFEST_FILE)).map_err(StagingError::Other)?
        != checkpoint.manifest_sha256
        || !verify_manifest(&dir)
            .map_err(StagingError::Other)?
            .is_valid()
    {
        return other("Checkpoint manifest integrity failed");
    }
    Ok(dir)
}

fn copy_regular(source: &Path, destination: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(source).map_err(|e| io_err("Inspect checkpoint", source, e))?;
    if !metadata.is_file() || metadata.len() == 0 {
        return other("Checkpoint input is missing, empty or not a regular file");
    }
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|e| io_err("Open checkpoint", source, e))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)
        .map_err(|e| io_err("Create checkpoint", destination, e))?;
    io::copy(&mut input, &mut output).map_err(|e| io_err("Copy checkpoint", destination, e))?;
    output
        .sync_all()
        .map_err(|e| io_err("Sync checkpoint", destination, e))
}

pub fn record_checkpoint(attempt: &mut Attempt, step: u32) -> Result<Checkpoint> {
    if attempt.record.state != AttemptState::Running
        || attempt.record.runtime != "mlx-lm"
        || step == 0
        || step > attempt.record.iters
        || attempt
            .record
            .last_checkpoint
            .as_ref()
            .is_some_and(|c| step <= c.iter)
    {
        return other("Unexpected checkpoint event");
    }
    ensure_real_dir(&attempt.dir(), false)?;
    ensure_real_dir(&attempt.out_dir(), false)?;
    ensure_real_dir(&attempt.dir().join("checkpoints"), true)?;
    let dir = snapshot(attempt, step);
    DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| io_err("Create snapshot", &dir, e))?;
    copy_regular(
        &attempt
            .out_dir()
            .join(format!("{step:07}_adapters.safetensors")),
        &dir.join("adapters.safetensors"),
    )?;
    copy_regular(
        &attempt.out_dir().join("adapter_config.json"),
        &dir.join("adapter_config.json"),
    )?;
    write_manifest(
        &dir,
        ManifestContext {
            runtime: attempt.record.runtime.clone(),
            base_model: attempt.record.base_model.clone(),
        },
    )
    .map_err(StagingError::Other)?;
    let checkpoint = Checkpoint {
        iter: step,
        manifest_sha256: sha256_file(&dir.join(MANIFEST_FILE)).map_err(StagingError::Other)?,
    };
    verified_snapshot(attempt, &checkpoint)?;
    sync_dir(&dir)?;
    sync_dir(&attempt.dir().join("checkpoints"))?;
    persist(attempt, |r| r.last_checkpoint = Some(checkpoint.clone()))?;
    Ok(checkpoint)
}

pub fn prepare_resume(attempt: &mut Attempt, source_id: &str) -> Result<PathBuf> {
    validate_name(source_id, "resume_attempt_id")?;
    if attempt.record.state != AttemptState::Created || attempt.record.runtime != "mlx-lm" {
        return other("Resume requires a new mlx-lm attempt");
    }
    let source_dir = staging_root(&attempt.root).join(source_id);
    ensure_real_dir(&staging_root(&attempt.root), false)?;
    ensure_real_dir(&source_dir, false)?;
    let source = Attempt {
        root: attempt.root.clone(),
        record: read_record(&source_dir)?,
    };
    if !matches!(
        source.record.state,
        AttemptState::Failed | AttemptState::Killed
    ) || source.record.runtime != "mlx-lm"
        || source.record.base_model != attempt.record.base_model
    {
        return other(
            "Resume source must be a failed/killed mlx-lm attempt with the same base model",
        );
    }
    let checkpoint = source
        .record
        .last_checkpoint
        .as_ref()
        .ok_or_else(|| StagingError::Other("No verified checkpoint available".into()))?;
    let origin = verified_snapshot(&source, checkpoint)?;
    let destination = attempt.dir().join("resume");
    DirBuilder::new()
        .mode(0o700)
        .create(&destination)
        .map_err(|e| io_err("Create resume snapshot", &destination, e))?;
    for file in ["adapters.safetensors", "adapter_config.json", MANIFEST_FILE] {
        copy_regular(&origin.join(file), &destination.join(file))?;
    }
    if sha256_file(&destination.join(MANIFEST_FILE)).map_err(StagingError::Other)?
        != checkpoint.manifest_sha256
        || !verify_manifest(&destination)
            .map_err(StagingError::Other)?
            .is_valid()
    {
        return other("Copied resume snapshot failed verification");
    }
    sync_dir(&destination)?;
    persist(attempt, |r| r.resume_source = Some(source_id.into()))?;
    Ok(destination.join("adapters.safetensors"))
}

#[derive(Debug, Serialize)]
pub struct ResumeSource {
    pub attempt_id: String,
    pub adapter_name: String,
    pub base_model: String,
    pub checkpoint: Checkpoint,
}

pub fn list_resume_sources(root: &Path) -> Result<Vec<ResumeSource>> {
    let mut sources = Vec::new();
    for row in list_attempts(root) {
        if matches!(row.status, AttemptStatus::ScanFailed { .. }) {
            return other("Cannot enumerate staging recovery sources");
        }
        if !matches!(
            row.status,
            AttemptStatus::Unchanged {
                state: AttemptState::Failed | AttemptState::Killed
            }
        ) {
            continue;
        }
        let record = read_record(&staging_root(root).join(&row.attempt_id))?;
        if record.runtime == "mlx-lm" {
            if let Some(checkpoint) = record.last_checkpoint {
                sources.push(ResumeSource {
                    attempt_id: record.attempt_id,
                    adapter_name: record.adapter_name,
                    base_model: record.base_model,
                    checkpoint,
                });
            }
        }
    }
    Ok(sources)
}

#[cfg(test)]
mod tests;
