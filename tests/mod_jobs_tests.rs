use serde_json::{json, Value};
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use swch::{
    mods::{package, ModManager},
    storage::Database,
};
use zip::{write::SimpleFileOptions, ZipWriter};

const BACKEND: &str = r#"
export default async s=>{
  s.registerMethod('ping',input=>({pong:input}));
  s.jobs.register('download',async(input,job)=>{
    if(job.savedCheckpoint?.offset===42){await job.progress({resumed:true});return {resumed:true,offset:42};}
    await job.checkpoint({offset:42});
    await job.progress({bytes:42,total:100});
    await new Promise((resolve,reject)=>{
      const timer=setTimeout(resolve,input.delay);
      job.signal.addEventListener('abort',()=>{clearTimeout(timer);reject(job.signal.reason);},{once:true});
      if(job.signal.aborted){clearTimeout(timer);reject(job.signal.reason);}
    });
    return {downloaded:true};
  });
  return async()=>{await s.storage.set('cleanup',Number(await s.storage.get('cleanup')||0)+1);};
}
"#;

fn node() -> PathBuf {
    std::env::var_os("SWCH_TEST_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("node"))
}

async fn fixture() -> (tempfile::TempDir, Database, Arc<ModManager>) {
    let directory = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let manager = Arc::new(ModManager::with_dir(
        db.clone(),
        directory.path().join("mods"),
        Some(node()),
    ));
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [
        ("manifest.json", json!({"id":"jobs.mod","name":"Jobs","version":"1.0.0","api":"^1.0.0","platforms":["windows-x64","macos-arm64"],"backend":"backend.mjs"}).to_string()),
        ("backend.mjs", BACKEND.into()),
    ] {
        archive.start_file(name, SimpleFileOptions::default()).unwrap();
        archive.write_all(content.as_bytes()).unwrap();
    }
    let bytes = archive.finish().unwrap().into_inner();
    manager
        .install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    (directory, db, manager)
}

async fn wait_job(manager: &ModManager, id: &str, ready: impl Fn(&Value) -> bool) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let job = manager
                .sdk_call("jobs.mod", "jobs.get".into(), json!({"id":id}))
                .await
                .unwrap();
            if ready(&job) {
                return job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Job state did not advance")
}

#[tokio::test]
async fn background_progress_and_cancellation_keep_internal_ui_methods_responsive() {
    let (_directory, _db, manager) = fixture().await;
    assert!(manager.list().await[0].contributions.actions.is_empty());
    let job = manager
        .sdk_call(
            "jobs.mod",
            "jobs.start".into(),
            json!({"type":"download","input":{"delay":20000}}),
        )
        .await
        .unwrap();
    let id = job["id"].as_str().unwrap();
    let running = wait_job(&manager, id, |job| job["progress"]["bytes"] == 42).await;
    assert_eq!(running["status"], "running");
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        manager.sdk_call("jobs.mod", "method".into(), json!({"id":"ping","input":42})),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result["pong"], 42);
    let cancelled = manager
        .sdk_call("jobs.mod", "jobs.cancel".into(), json!({"id":id}))
        .await
        .unwrap();
    assert_eq!(cancelled["status"], "cancelled");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        manager
            .sdk_call("jobs.mod", "jobs.get".into(), json!({"id":id}))
            .await
            .unwrap()["status"],
        "cancelled"
    );
    assert!(manager
        .sdk_call("jobs.mod", "method".into(), json!({"id":"missing"}))
        .await
        .is_err());
    manager.shutdown().await;
}

#[tokio::test]
async fn background_jobs_resume_checkpoints_and_keep_results_after_host_restart() {
    let (directory, db, manager) = fixture().await;
    let job = manager
        .sdk_call(
            "jobs.mod",
            "jobs.start".into(),
            json!({"type":"download","input":{"delay":20000}}),
        )
        .await
        .unwrap();
    let id = job["id"].as_str().unwrap().to_owned();
    wait_job(&manager, &id, |job| job["progress"]["bytes"] == 42).await;
    manager.shutdown().await;
    drop(manager);
    let reopened = ModManager::with_dir(db, directory.path().join("mods"), Some(node()));
    reopened.initialize().await;
    let complete = wait_job(&reopened, &id, |job| job["status"] == "completed").await;
    assert_eq!(complete["result"], json!({"resumed":true,"offset":42}));
    assert_eq!(complete["attempt"], 2);
    assert_eq!(
        reopened
            .sdk_call("jobs.mod", "storage.get".into(), json!({"key":"cleanup"}))
            .await
            .unwrap(),
        1
    );
    reopened.reload("jobs.mod").await.unwrap();
    assert_eq!(
        reopened
            .sdk_call("jobs.mod", "jobs.get".into(), json!({"id":id}))
            .await
            .unwrap()["result"],
        complete["result"]
    );
    reopened.shutdown().await;
}
