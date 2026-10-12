use serde_json::{json, Value};
use std::{
    io::{Cursor, Write},
    path::PathBuf,
    sync::Arc,
};
use swch::{
    mods::{package, ModManager},
    storage::Database,
};
use zip::{write::SimpleFileOptions, ZipWriter};

fn archive(files: Vec<(&str, String)>) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in files {
        writer
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(content.as_bytes()).unwrap();
    }
    writer.finish().unwrap().into_inner()
}
fn manifest(version: &str) -> Value {
    json!({"id":"test.mod","name":"Test Mod","version":version,"api":"^1.0.0","platforms":["windows-x64","macos-arm64"],"backend":"backend.mjs"})
}
fn backend(version: &str, source: &str) -> Vec<u8> {
    archive(vec![
        ("manifest.json", manifest(version).to_string()),
        ("backend.mjs", source.into()),
    ])
}
fn node() -> PathBuf {
    std::env::var_os("SWCH_TEST_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("node"))
}
async fn fixture() -> (tempfile::TempDir, Database, Arc<ModManager>) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let manager = Arc::new(ModManager::with_dir(
        db.clone(),
        dir.path().join("mods"),
        Some(node()),
    ));
    (dir, db, manager)
}
const PROVIDER: &str = r#"
export default async function(s){
 const accounts=async()=>await s.storage.get('accounts')||[];
 s.registerLauncher({id:'demo',name:'Demo',capabilities:['library','sync','accounts','connect','switch','disconnect','launch']},{
 accounts,async connect(){await s.storage.set('accounts',[{id:'alice',username:'Alice',is_active:true},{id:'bob',username:'Bob',is_active:false}]);},
 async library(){return [{id:'game',app_id:'game',name:'Mod Game',is_installed:true,owner_ids:['alice','bob']}];},
 async sync(){return [{id:'game',app_id:'game',name:'Mod Game',is_installed:true,owner_ids:['alice','bob']}];},
 async switch({account_id}){await s.storage.set('accounts',(await accounts()).map(a=>({...a,is_active:a.id===account_id})));},
 async disconnect({account_id}){await s.storage.set('accounts',(await accounts()).filter(a=>a.id!==account_id));},
 async launch(input){await s.storage.set('launched',input);return {message:'launched'};}
 });
 s.registerAction({id:'count',name:'Count'},async()=>{const n=Number(await s.storage.get('count')||0)+1;await s.storage.set('count',n);return n;});
 s.registerAction({id:'crash',name:'Crash'},()=>process.exit(9));
 s.registerAction({id:'hang',name:'Hang'},()=>new Promise(()=>{}));
 s.registerAction({id:'slow',name:'Slow'},async()=>{await new Promise(r=>setTimeout(r,200));return 'complete';});
 s.registerAction({id:'fail',name:'Fail'},()=>{throw new Error('Recoverable');});
 console.log('activation log');
}
"#;

#[test]
fn packages_reject_corruption_paths_api_platform_and_addons() {
    for id in ["con", "aux.mod", "trailing."] {
        assert!(!swch::mods::types::valid_id(id));
    }
    let dir = tempfile::tempdir().unwrap();
    let bytes = backend("1.0.0", PROVIDER);
    assert!(package::extract(&bytes, "bad", dir.path(), None).is_err());
    let valid = package::extract(&bytes, &package::digest(&bytes), dir.path(), None).unwrap();
    assert_eq!(valid.id, "test.mod");
    assert!(valid.validate(Some("linux-x64")).is_err());
    let mut invalid = valid.clone();
    invalid.api = "^2.0.0".into();
    assert!(invalid.validate(None).is_err());
    for name in [
        "../outside",
        "/absolute",
        "dir\\file",
        "C:/file",
        "con.txt",
        "data.NODE",
        "a/../b",
    ] {
        let zip = archive(vec![
            ("manifest.json", manifest("1.0.0").to_string()),
            ("backend.mjs", PROVIDER.into()),
            (name, "evil".into()),
        ]);
        assert!(
            package::extract(
                &zip,
                &package::digest(&zip),
                tempfile::tempdir().unwrap().path(),
                None
            )
            .is_err(),
            "{name}"
        );
    }
    let zip = archive(vec![
        ("manifest.json", manifest("1.0.0").to_string()),
        ("backend.mjs", PROVIDER.into()),
        ("BACKEND.MJS", "evil".into()),
    ]);
    assert!(package::extract(&zip, &package::digest(&zip), dir.path(), None).is_err());
}

#[test]
fn symlinks_are_rejected_before_extraction() {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("link", "../outside", SimpleFileOptions::default())
        .unwrap();
    let zip = writer.finish().unwrap().into_inner();
    let dir = tempfile::tempdir().unwrap();
    assert!(package::extract(&zip, &package::digest(&zip), dir.path(), None).is_err());
    assert!(!dir.path().join("link").exists());
}

#[tokio::test]
async fn launcher_lifecycle_namespaces_ownership_and_persistence() {
    let (dir, db, m) = fixture().await;
    let bytes = backend("1.0.0", PROVIDER);
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    let p = "mod:test.mod:demo";
    assert_eq!(m.platforms().await[0].id, p);
    m.platform_operation(p, "connect", json!({})).await.unwrap();
    m.platform_operation(p, "sync", json!({})).await.unwrap();
    let library = m.library().await.unwrap();
    assert_eq!(library["games"][0]["id"], format!("{p}:game:game"));
    assert_eq!(
        library["games"][0]["owner_ids"].as_array().unwrap().len(),
        2
    );
    m.platform_operation(
        p,
        "switch",
        json!({"account_id":format!("{p}:account:bob")}),
    )
    .await
    .unwrap();
    assert!(db
        .get_accounts()
        .await
        .unwrap()
        .iter()
        .any(|a| a.username == "Bob" && a.is_active));
    m.platform_operation(
        p,
        "launch",
        json!({"game_id":format!("{p}:game:game"),"account_id":format!("{p}:account:bob")}),
    )
    .await
    .unwrap();
    assert_eq!(
        m.sdk_call("test.mod", "storage.get".into(), json!({"key":"launched"}))
            .await
            .unwrap()["game_id"],
        "game"
    );
    assert!(m
        .platform_operation(p, "launch", json!({"game_id":"mod:other:demo:game:game"}))
        .await
        .is_err());
    assert_eq!(m.action("test.mod", "count", json!({})).await.unwrap(), 1);
    assert!(m
        .logs("test.mod")
        .await
        .unwrap()
        .iter()
        .any(|s| s.contains("activation log")));
    m.toggle("test.mod", false).await.unwrap();
    assert!(m.library().await.unwrap()["games"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(db.get_games().await.unwrap().len(), 1);
    m.shutdown().await;
    drop(m);
    let reopened = ModManager::with_dir(db.clone(), dir.path().join("mods"), Some(node()));
    assert!(!reopened.list().await[0].enabled);
    reopened.toggle("test.mod", true).await.unwrap();
    assert_eq!(
        reopened
            .action("test.mod", "count", json!({}))
            .await
            .unwrap(),
        2
    );
    reopened
        .platform_operation(
            p,
            "disconnect",
            json!({"account_id":format!("{p}:account:alice")}),
        )
        .await
        .unwrap();
    assert_eq!(db.get_accounts().await.unwrap().len(), 1);
    reopened.uninstall("test.mod").await.unwrap();
    assert_eq!(db.get_games().await.unwrap().len(), 1);
    reopened.clear_data("test.mod").await.unwrap();
    assert!(db.get_games().await.unwrap().is_empty());
    assert!(db.get_accounts().await.unwrap().is_empty());
}

#[tokio::test]
async fn failing_update_rolls_back_and_update_waits_for_current_operation() {
    let (_dir, _db, m) = fixture().await;
    let bytes = backend("1.0.0", PROVIDER);
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    let bad = backend(
        "2.0.0",
        "export default ()=>{throw new Error('bad activation')}",
    );
    let failure = m
        .install_bytes(&bad, &package::digest(&bad), None)
        .await
        .unwrap_err();
    assert!(failure.contains("rolled back"), "{failure}");
    assert_eq!(m.list().await[0].manifest.version, "1.0.0");
    assert_eq!(m.action("test.mod", "count", json!({})).await.unwrap(), 1);
    let next = backend("2.0.0", PROVIDER);
    let clone = m.clone();
    let task = tokio::spawn(async move { clone.action("test.mod", "slow", json!({})).await });
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    m.install_bytes(&next, &package::digest(&next), None)
        .await
        .unwrap();
    assert_eq!(task.await.unwrap().unwrap(), "complete");
    assert_eq!(m.list().await[0].manifest.version, "2.0.0");
    assert_eq!(m.action("test.mod", "count", json!({})).await.unwrap(), 2);
    assert!(m
        .install_bytes(&next, &package::digest(&next), None)
        .await
        .is_err());
    m.shutdown().await;
}

#[tokio::test]
async fn errors_crashes_restart_and_timeout_do_not_crash_host() {
    let (_dir, _db, m) = fixture().await;
    let bytes = backend("1.0.0", PROVIDER);
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    assert!(m.action("test.mod", "fail", json!({})).await.is_err());
    assert_eq!(m.list().await[0].status, "active");
    assert!(m.action("test.mod", "crash", json!({})).await.is_err());
    assert_eq!(m.list().await[0].status, "error");
    m.reload("test.mod").await.unwrap();
    assert_eq!(m.action("test.mod", "count", json!({})).await.unwrap(), 1);
    assert!(m
        .action("test.mod", "hang", json!({}))
        .await
        .unwrap_err()
        .contains("timed out"));
    assert_eq!(m.list().await[0].status, "error");
    m.reload("test.mod").await.unwrap();
    m.shutdown().await;
}

#[tokio::test]
async fn catalog_cache_and_cross_origin_installation() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let (_dir, db, m) = fixture().await;
    std::fs::create_dir_all(&m.root).unwrap();
    std::fs::write(
        m.root.join("catalog.json"),
        json!({"schema":1,"mods":[],"blocked":[]}).to_string(),
    )
    .unwrap();
    assert!(m.catalog(false).await.unwrap()["warning"].is_null());
    let epic = Arc::new(swch::launchers::epic::EpicAuthStore::default_store(
        db.clone(),
    ));
    let state = swch::server::AppState::new(db.clone(), epic.clone());
    let claimer = swch::auto_claim::AutoClaimer::new(db, epic);
    let app = swch::server::create_app_with_auto_claim_and_mods(state, claimer, m.clone());
    let request = Request::builder()
        .uri("/api/mods")
        .header("host", "127.0.0.1:4242")
        .header("origin", "https://evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let request = Request::builder()
        .uri("/api/mods")
        .header("host", "127.0.0.1:4242")
        .body(Body::empty())
        .unwrap();
    assert_eq!(app.oneshot(request).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn background_sdk_calls_and_event_subscriptions_are_serviced_while_idle() {
    let (_dir, _db, m) = fixture().await;
    let source = r#"export default async s=>{
      s.events.on('library.changed',async()=>{await s.storage.set('changed',true);s.events.emit('received',true);});
      setTimeout(async()=>{await s.storage.set('background',42);s.events.emit('ready',true);},60);
    }"#;
    let mut events = m.events.subscribe();
    let bytes = backend("1.0.0", source);
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    async fn wait_event(rx: &mut tokio::sync::broadcast::Receiver<Value>, expected: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if rx.recv().await.unwrap()["event"] == expected {
                    break;
                }
            }
        })
        .await
        .unwrap();
    }
    wait_event(&mut events, "mod:test.mod:ready").await;
    assert_eq!(
        m.sdk_call(
            "test.mod",
            "storage.get".into(),
            json!({"key":"background"})
        )
        .await
        .unwrap(),
        42
    );
    m.events.send(json!({"event":"library.changed"})).unwrap();
    wait_event(&mut events, "mod:test.mod:received").await;
    assert_eq!(
        m.sdk_call("test.mod", "storage.get".into(), json!({"key":"changed"}))
            .await
            .unwrap(),
        true
    );
    m.shutdown().await;
}

#[tokio::test]
async fn malformed_and_oversized_protocol_output_are_rejected() {
    for source in [
        "export default ()=>process.stdout.write('not-json\\n')",
        "export default ()=>process.stdout.write('x'.repeat(1048600)+'\\n')",
    ] {
        let (_dir, _db, m) = fixture().await;
        let bytes = backend("1.0.0", source);
        assert!(m
            .install_bytes(&bytes, &package::digest(&bytes), None)
            .await
            .is_err());
        assert!(m.list().await.is_empty());
    }
}

#[tokio::test]
async fn author_cli_zip_installs_and_development_reload_preserves_sources() {
    let (dir, _db, m) = fixture().await;
    let source = dir.path().join("author");
    let status = std::process::Command::new(node())
        .args(["mod-sdk/cli.mjs", "init"])
        .arg(&source)
        .args(["--template", "launcher", "--id", "alice.launcher"])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let package = dir.path().join("mod.zip");
    let status = std::process::Command::new(node())
        .args(["mod-sdk/cli.mjs", "pack"])
        .arg(&source)
        .arg(&package)
        .output()
        .unwrap();
    assert!(status.status.success());
    let bytes = std::fs::read(&package).unwrap();
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    m.uninstall("alice.launcher").await.unwrap();
    m.development(source.clone()).await.unwrap();
    m.reload("alice.launcher").await.unwrap();
    assert!(m.list().await[0].development);
    m.uninstall("alice.launcher").await.unwrap();
    assert!(source.join("manifest.json").is_file());
}

#[tokio::test]
async fn launched_child_does_not_lock_package_during_update() {
    let (_dir, _db, m) = fixture().await;
    let source = r#"import {spawn} from 'node:child_process';export default s=>s.registerAction({id:'launch',name:'Launch'},async()=>{
      const child=spawn(process.execPath,['-e','setTimeout(()=>process.exit(0),1000)'],{stdio:'ignore',windowsHide:true});
      await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});return child.pid;
    });"#;
    let bytes = backend("1.0.0", source);
    m.install_bytes(&bytes, &package::digest(&bytes), None)
        .await
        .unwrap();
    m.action("test.mod", "launch", json!({})).await.unwrap();
    let next = backend("2.0.0", source);
    m.install_bytes(&next, &package::digest(&next), None)
        .await
        .unwrap();
    assert_eq!(m.list().await[0].manifest.version, "2.0.0");
    m.shutdown().await;
}
