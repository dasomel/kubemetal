import json, subprocess, tempfile, shutil
from pathlib import Path
TMP=Path(tempfile.mkdtemp(prefix='s2-mutations-'))
BASE=Path.cwd()
R=Path('src-tauri/src/commands/mlx/training_staging.rs')
S=Path('src-tauri/src/services/adapter_staging.rs')
P=Path('scripts/mlx/finetune_wrapper.py')
C=Path('src-tauri/src/commands/mlx.rs')
originals={p:p.read_text() for p in [R,S,P,C]}
results=[]
def run(name,path,mutate,command,expected):
    for p,s in originals.items():p.write_text(s)
    text=mutate(originals[path])
    assert text!=originals[path],name
    path.write_text(text)
    try:
        result=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,timeout=900)
        (TMP/('s2-mutation-'+name+'.log')).write_text(result.stdout)
        observed=result.returncode!=0 and expected in result.stdout and 'could not compile' not in result.stdout
        results.append({'mutation':name,'command':command,'exit':result.returncode,'expected_failure_observed':observed})
        print(name, 'EXPECTED FAILURE' if observed else 'INVALID MUTATION RESULT', 'exit='+str(result.returncode),flush=True)
        if not observed:print(result.stdout[-3000:],flush=True)
    finally:
        for p,s in originals.items():p.write_text(s)
def py(test):return ['python3','-m','unittest','discover','-s','tests/mlx','-p','test_output_admission.py','-k',test,'-v']
def rust(test):return ['cargo','test','--locked','--manifest-path','src-tauri/Cargo.toml','--lib',test,'--','--nocapture']
run('required-output',P,lambda s:s.replace('add_mutually_exclusive_group(required=True)','add_mutually_exclusive_group(required=False)'),py('test_output_dir_is_required'),'FAIL: test_output_dir_is_required')
run('reject-symlinks',P,lambda s:s.replace('metadata = adapter_path.lstat()','metadata = adapter_path.stat()'),py('test_invalid_output'),'FAIL: test_invalid_output')
run('reject-nonempty',P,lambda s:s.replace('if any(adapter_path.iterdir()):','if False:'),py('test_invalid_output'),'FAIL: test_invalid_output')
def create_missing(s):
    return s.replace('metadata = adapter_path.lstat()','adapter_path.mkdir(parents=True, exist_ok=True)\n            metadata = adapter_path.lstat()')
run('reject-missing-file',P,create_missing,py('test_invalid_output'),'FAIL: test_invalid_output')
run('lm-output-mapping',P,lambda s:s.replace('"--adapter-path", str(adapter_path)','"--output-path", str(adapter_path)'),py('test_runtime_output'),'ERROR: test_runtime_output')
run('vlm-output-mapping',P,lambda s:s.replace('"--output-path", str(adapter_path)','"--adapter-path", str(adapter_path)'),py('test_runtime_output'),'ERROR: test_runtime_output')
run('forged-path',R,lambda s:s.replace('if path.is_none() || path != expected.to_str() || self.path.is_some() {','if false {'),rust('forged_done_path'),'forged_done_path_never_publishes_or_promotes ... FAILED')
run('requires-done',R,lambda s:s.replace('if !outcome.report.matches_expected(&attempt.out_dir()) {','if false {'),rust('zero_exit_without_done'),'zero_exit_without_done_is_error ... FAILED')
run('manifest-failure-is-error',R,lambda s:s.replace('training.status = if killed { "killed" } else { "error" }.into();','training.status = if killed { "killed" } else { "done" }.into();'),rust('manifest_write_failure'),'manifest_write_failure_is_error_never_done ... FAILED')
def omit_exit(s):
    a=s.index('        match &outcome.exit {');b=s.index('        if let Some(error) = &outcome.report.error {\n            return Err(error.clone());',a)
    return s[:a]+s[b:]
run('requires-zero-exit',R,omit_exit,rust('failed_exit_cannot'),'failed_exit_cannot_be_overridden_by_done ... FAILED')
def omit_stop(s):
    return s.replace('stopped.load(Ordering::SeqCst)','false').replace('t.status == "killed"','false')
run('stop-intent',R,omit_stop,rust('killed_'),'killed_zero_exit_with_done_stays_killed ... FAILED')
def old_event_authority(s):
    a=s.index('    if !mlx_lifecycle::is_non_terminal_training_status');b=s.index('    match value.get',a)
    return (s[:a]+s[b:]).replace('        Some("done") => {\n            // Only finalize','        Some("done") => {\n            training.status = "done".into();\n            // Only finalize')
run('late-child-event',R,old_event_authority,rust('wrapper_error_and_late'),'wrapper_error_and_late_done_cannot_erase_stop ... FAILED')
def omit_collision(s):
    a=s.index('    match fs::symlink_metadata(&final_path) {',s.index('fn create_attempt_with'));b=s.index('    if !fs::metadata(root)',a)
    return s[:a]+s[b:]
run('legacy-collision',S,omit_collision,rust('legacy_name_collision'),'legacy_name_collision_is_refused_byte_for_byte ... FAILED')
run('same-delete-mutex',R,lambda s:s.replace('let admission = state.adapter_admission.lock().map_err(|e| e.to_string())?;','let unrelated = std::sync::Mutex::new(());\n    let admission = unrelated.lock().map_err(|e| e.to_string())?;'),rust('promotion_waits'),'promotion_waits_for_delete_admission_mutex ... FAILED')
run('spawn-cleanup',R,lambda s:s.replace('cleanup_unspawned(attempt)','Ok::<(), String>(())',1),rust('spawn_failure_removes'),'spawn_failure_removes_only_new_empty_attempt ... FAILED')
run('rmdir-only',R,lambda s:s.replace('std::fs::remove_dir(attempt.out_dir())','std::fs::remove_dir_all(attempt.out_dir())'),rust('spawn_failure_removes'),'spawn_failure_removes_only_new_empty_attempt ... FAILED')
# Tempdir-only negative control for preservation of the older attempt.
run('preserve-older-attempt',R,lambda s:s.replace('std::fs::remove_dir(attempt.out_dir())','std::fs::remove_dir_all(attempt.dir().parent().unwrap())'),rust('spawn_failure_removes'),'spawn_failure_removes_only_new_empty_attempt ... FAILED')
run('persist-stop-across-slot-replacement',R,lambda s:s.replace('stopped.store(true, Ordering::SeqCst);','// stop intent omitted'),rust('killed_attempt_survives'),'killed_attempt_survives_slot_replacement ... FAILED')
run('failed-stop-clear-intent',R,lambda s:s.replace('stopped.store(false, Ordering::SeqCst);','// clear omitted'),rust('failed_stop_does_not'),'failed_stop_does_not_claim_killed_or_done ... FAILED')
run('delete-root-excludes-staging',C,lambda s:s.replace('let requested_root = home.join(".kubemetal").join("adapters");','let requested_root = home.join(".kubemetal");'),rust('staging_is_protected'),'staging_is_protected_and_outside_delete_ipc_root ... FAILED')
def omit_publication_failure_state(s):
    return s.replace('| AttemptState::ExitedOk\n                    | AttemptState::Verified\n                    | AttemptState::VerifiedUnpromoted', '| AttemptState::ExitedOk')
run('promotion-collision-is-failed',R,omit_publication_failure_state,rust('promotion_collision_records'),'promotion_collision_records_failed_and_preserves_both_outputs ... FAILED')
run('promotion-permission-is-failed',R,omit_publication_failure_state,rust('promotion_permission_failure'),'promotion_permission_failure_records_failed ... FAILED')
run('late-mlflow-reconciliation',R,lambda s:s.replace('        && value.get("type").and_then(Value::as_str) != Some("mlflow_run_started")',''),rust('stopped_child_late_mlflow'),'stopped_child_late_mlflow_run_id_is_retained ... FAILED')
run('missing-done-unknown-expected-path',R,lambda s:s.replace('self.path.is_some() && self.path.as_deref() == expected.to_str()', 'self.path.as_deref() == expected.to_str()'),rust('missing_done_cannot_match_non_utf8'),'missing_done_cannot_match_non_utf8_expected_path ... FAILED')
run('same-attempt-slot-ownership',R,lambda s:s.replace('t.pid == pid && same_attempt','t.pid == pid'),rust('reused_pid_does_not'),'reused_pid_does_not_overwrite_next_attempt ... FAILED')
# New in the opus-review fix round (findings 1, 2, 6).
run('hold-training-lock-while-hashing',R,lambda s:s.replace('before_hashing();','let held = state.training.lock().unwrap();\n        before_hashing();\n        drop(held);'),rust('hashing_runs_with'),'hashing_runs_with_admission_held_and_training_slot_free ... FAILED')
run('done-overwrites-stop-during-hashing',R,lambda s:s.replace('Ok(promoted) if live && !killed =>','Ok(promoted) =>'),rust('stop_during_hashing'),'stop_during_hashing_is_not_overwritten_by_done ... FAILED')
run('generic-stderr-over-wrapper-error',R,lambda s:s.replace('if let Some(error) = &outcome.report.error {\n                    error.clone()\n                } else if','if'),rust('failed_exit_surfaces'),'failed_exit_surfaces_wrapper_error_over_generic_stderr ... FAILED')
run('never-started-killed-stays-created',R,lambda s:s.replace('if killed && attempt.record().state != AttemptState::Created {','if killed {'),rust('killed_attempt_that_never'),'killed_attempt_that_never_started_ends_failed_not_created ... FAILED')
print(len(results),'mutations run;',sum(r['expected_failure_observed'] for r in results),'detected',flush=True)
for r in results:
    if not r['expected_failure_observed']:print('NOT DETECTED:',r['mutation'],flush=True)
shutil.rmtree(TMP,ignore_errors=True)
assert all(r['expected_failure_observed'] for r in results), 'Some mutation did not fail as expected'
print('ALL',len(results),'MUTATIONS FAILED THEIR SAFETY TESTS',flush=True)
