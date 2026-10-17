use serde_json::{json, Value};
use std::{fs, io::BufReader, path::PathBuf, sync::Arc, time::Duration};
use swch::{
    mods::{package, types::target, ModManager},
    storage::Database,
};

fn alive(pid: u32) -> bool {
    let mut system = sysinfo::System::new();
    system.refresh_processes();
    system.process(sysinfo::Pid::from_u32(pid)).is_some()
}
async fn wait_until(check: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn declared_tools_and_their_descendants_stop_on_disable_and_backend_crash() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("package");
    fs::create_dir_all(source.join("native")).unwrap();
    let node = std::env::var_os("SWCH_TEST_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| "node".into());
    let output = std::process::Command::new(&node)
        .args(["-p", "process.execPath"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let original = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    let executable = if cfg!(windows) {
        "native/helper.exe"
    } else {
        "native/helper"
    };
    fs::copy(original, source.join(executable)).unwrap();
    let hash = package::digest_reader(&mut BufReader::new(
        fs::File::open(source.join(executable)).unwrap(),
    ))
    .unwrap();
    fs::write(source.join("manifest.json"),json!({"id":"tools.mod","name":"Tools","version":"1.0.0","api":"^1.1.0","platforms":["windows-x64","macos-arm64"],"backend":"backend.mjs","native_tools":[{"id":"helper","version":"1.0","platform":target(),"license":"MIT","files":[{"path":executable,"sha256":hash,"executable":true}]}]}).to_string()).unwrap();
    fs::write(
        source.join("backend.mjs"),
        "export default s=>{s.registerMethod('crash',()=>process.exit(9));}",
    )
    .unwrap();
    let db = Database::open_in_memory().unwrap();
    db.run_migrations().await.unwrap();
    let manager = Arc::new(ModManager::with_dir(
        db,
        directory.path().join("mods"),
        Some(node),
    ));
    manager.development(source.clone()).await.unwrap();
    let tools = manager
        .sdk_call("tools.mod", "tools.list".into(), json!({}))
        .await
        .unwrap();
    assert_eq!(tools[0]["id"], "helper");
    assert_eq!(tools[0]["license"], "MIT");
    for crash in [false, true] {
        if crash {
            manager.toggle("tools.mod", true).await.unwrap();
        }
        let paths = manager
            .sdk_call("tools.mod", "files.paths".into(), json!({}))
            .await
            .unwrap();
        let pid_file = PathBuf::from(paths["data"].as_str().unwrap()).join(if crash {
            "crash.pid"
        } else {
            "disable.pid"
        });
        let script="const {spawn}=require('node:child_process');const child=spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'ignore'});require('node:fs').writeFileSync(process.argv[1],String(child.pid));setInterval(()=>{},1000);";
        let started = manager
            .sdk_call(
                "tools.mod",
                "tools.run".into(),
                json!({"id":"helper","args":["-e",script,pid_file]}),
            )
            .await
            .unwrap();
        let pid = started["process_id"].as_u64().unwrap() as u32;
        wait_until(|| pid_file.exists()).await;
        let descendant = fs::read_to_string(pid_file)
            .unwrap()
            .parse::<u32>()
            .unwrap();
        assert!(alive(pid));
        assert!(alive(descendant));
        if crash {
            assert!(manager
                .sdk_call("tools.mod", "method".into(), json!({"id":"crash"}))
                .await
                .is_err());
        } else {
            manager.toggle("tools.mod", false).await.unwrap();
        }
        wait_until(|| !alive(pid) && !alive(descendant)).await;
    }
    // File management stays usable while a mod is disabled or has crashed.
    let usage: Value = manager
        .sdk_call("tools.mod", "files.usage".into(), json!({}))
        .await
        .unwrap();
    assert!(usage["tools"].as_u64().unwrap() > 0);
    manager.uninstall("tools.mod").await.unwrap();
    manager.clear_data("tools.mod").await.unwrap();
    manager.development(source).await.unwrap();
    let reinstalled=manager.sdk_call("tools.mod","tools.list".into(),json!({})).await.unwrap();
    assert!(PathBuf::from(reinstalled[0]["directory"].as_str().unwrap()).join(executable).is_file());
    manager.shutdown().await;
}
