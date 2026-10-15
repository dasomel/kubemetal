use super::*;

fn attempt(root: &Path, name: &str) -> Attempt {
    create_attempt(
        root,
        &AttemptSpec {
            adapter_name: name.into(),
            runtime: "mlx-lm".into(),
            base_model: "same-model".into(),
            iters: 10,
            mlflow_run_id: None,
        },
    )
    .unwrap()
}

fn saved(a: &Attempt) {
    fs::write(
        a.out_dir().join("0000002_adapters.safetensors"),
        b"fixture weights",
    )
    .unwrap();
    fs::write(a.out_dir().join("adapter_config.json"), b"{}").unwrap();
}

#[test]
fn checkpoint_survives_partial_output_and_resume_is_isolated() {
    let root = TempRoot::new();
    let mut source = attempt(&root.0, "source");
    mark_running(&mut source, 1, Some(1)).unwrap();
    saved(&source);
    let checkpoint = record_checkpoint(&mut source, 2).unwrap();
    fs::write(
        source.out_dir().join("adapters.safetensors"),
        b"partial next save",
    )
    .unwrap();
    transition(&mut source, AttemptState::Killed).unwrap();
    let mut next = attempt(&root.0, "next");
    let path = prepare_resume(&mut next, &source.record.attempt_id).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"fixture weights");
    fs::write(&path, b"changed destination").unwrap();
    assert!(verified_snapshot(&source, &checkpoint).is_ok());
    assert_eq!(source.record.state, AttemptState::Killed);
    assert!(!source.final_path().exists());
}

#[test]
fn tampered_missing_running_and_wrong_model_sources_are_refused() {
    let root = TempRoot::new();
    let mut source = attempt(&root.0, "source");
    mark_running(&mut source, 1, Some(1)).unwrap();
    assert!(record_checkpoint(&mut source, 2).is_err());
    // Failed snapshots are retained and cannot be published as verified.
    assert!(source.record.last_checkpoint.is_none());
    saved(&source);
    record_checkpoint(&mut source, 3).unwrap_err();
    fs::write(
        source.out_dir().join("0000004_adapters.safetensors"),
        b"weights",
    )
    .unwrap();
    record_checkpoint(&mut source, 4).unwrap();
    let mut next = attempt(&root.0, "next");
    assert!(prepare_resume(&mut next, &source.record.attempt_id).is_err());
    transition(&mut source, AttemptState::Failed).unwrap();
    next.record.base_model = "other-model".into();
    assert!(prepare_resume(&mut next, &source.record.attempt_id).is_err());
    next.record.base_model = "same-model".into();
    fs::write(
        snapshot(&source, 4).join("adapters.safetensors"),
        b"tampered",
    )
    .unwrap();
    assert!(prepare_resume(&mut next, &source.record.attempt_id).is_err());
    assert!(prepare_resume(&mut next, "../escape").is_err());
}

struct TempRoot(PathBuf);
impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(new_attempt_id());
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[ignore = "Runs real MLX training on Apple Silicon; opt in with --ignored"]
fn ondevice_kill_and_warm_resume() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let root = repo.join("evidence/mlx-checkpoints/ondevice");
    fs::create_dir_all(&root).unwrap();
    let model = home.join(".kubemetal/models/mlx-community__Qwen2.5-0.5B-Instruct-4bit");
    let python = home.join(".kubemetal/venv/bin/python");
    let data = home.join(".kubemetal/datasets/smoke");
    let spec = |name: String, iters| AttemptSpec {
        adapter_name: name,
        runtime: "mlx-lm".into(),
        base_model: model.to_string_lossy().into_owned(),
        iters,
        mlflow_run_id: None,
    };
    let mut source =
        create_attempt(&root, &spec(format!("source-{}", new_attempt_id()), 20)).unwrap();
    let command = |a: &Attempt| {
        let mut cmd = Command::new(&python);
        cmd.arg(repo.join("scripts/mlx/finetune_wrapper.py"))
            .args(["--model"])
            .arg(&model)
            .arg("--data")
            .arg(&data)
            .arg("--iters")
            .arg(a.record.iters.to_string())
            .args([
                "--batch-size",
                "1",
                "--learning-rate",
                "0.00001",
                "--save-every",
                "1",
                "--adapter-name",
            ])
            .arg(&a.record.adapter_name)
            .arg("--output-dir")
            .arg(a.out_dir())
            .args(["--mlflow-uri", "http://127.0.0.1:1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0);
        cmd
    };
    let mut child = command(&source).spawn().unwrap();
    let pid = child.id();
    mark_running(
        &mut source,
        pid,
        crate::services::process::process_start_time(pid),
    )
    .unwrap();
    let mut checkpoint_seen = false;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        println!("source: {line}");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["type"] == "checkpoint" {
            record_checkpoint(&mut source, value["iter"].as_u64().unwrap() as u32).unwrap();
            checkpoint_seen = true;
            // Test owns this freshly spawned process group; no existing app PID is touched.
            assert_eq!(unsafe { libc::kill(-(pid as i32), libc::SIGTERM) }, 0);
            break;
        }
    }
    let exit = child.wait().unwrap();
    assert!(
        checkpoint_seen && !exit.success(),
        "must observe save then kill before completion"
    );
    transition(&mut source, AttemptState::Killed).unwrap();
    let mut next = create_attempt(&root, &spec(format!("resume-{}", new_attempt_id()), 2)).unwrap();
    let resume = prepare_resume(&mut next, &source.record.attempt_id).unwrap();
    let mut cmd = command(&next);
    cmd.arg("--resume-adapter-file").arg(&resume);
    let mut child = cmd.spawn().unwrap();
    mark_running(
        &mut next,
        child.id(),
        crate::services::process::process_start_time(child.id()),
    )
    .unwrap();
    let mut done = false;
    for line in BufReader::new(child.stdout.take().unwrap()).lines() {
        let line = line.unwrap();
        println!("resume: {line}");
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        if value["type"] == "checkpoint" {
            record_checkpoint(&mut next, value["iter"].as_u64().unwrap() as u32).unwrap();
        }
        done |=
            value["type"] == "done" && value["adapter_path"].as_str() == next.out_dir().to_str();
    }
    assert!(child.wait().unwrap().success() && done);
    transition(&mut next, AttemptState::ExitedOk).unwrap();
    verify_out(&mut next).unwrap();
    let admission = std::sync::Mutex::new(());
    promote(&mut next, &admission.lock().unwrap()).unwrap();
    assert_eq!(
        read_record(&source.dir()).unwrap().state,
        AttemptState::Killed
    );
    assert!(!source.final_path().exists());
    println!(
        "PASS: killed source {} retained; resumed {} verified and promoted",
        source.record.attempt_id, next.record.attempt_id
    );
}
