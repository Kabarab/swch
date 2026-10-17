use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{self, BufReader, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
};
use swch::{
    mods::{
        package,
        types::{Catalog, Manifest, PackageLimits},
        ModManager,
    },
    storage::Database,
};
use zip::{write::SimpleFileOptions, ZipWriter};

fn working_directory() -> tempfile::TempDir {
    // Keep large fixtures on the project disk instead of the operating system disk.
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target_build/mod-package-tests");
    fs::create_dir_all(&directory).unwrap();
    tempfile::tempdir_in(directory).unwrap()
}

fn manifest() -> Value {
    json!({"id":"test.large","name":"Large package","version":"1.0.0","api":"^1.0.0",
        "platforms":["windows-x64","macos-arm64"],"ui":"index.html"})
}

fn write_archive(path: &Path, expanded_bytes: u64) -> String {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default()
        .compression_method(if expanded_bytes > 1024 * 1024 * 1024 {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        })
        .compression_level(if expanded_bytes > 1024 * 1024 * 1024 {None} else {Some(1)});
    writer.start_file("manifest.json", options).unwrap();
    writer.write_all(manifest().to_string().as_bytes()).unwrap();
    writer.start_file("index.html", options).unwrap();
    writer
        .write_all(b"<!doctype html><title>Large mod</title>")
        .unwrap();
    writer
        .start_file("assets/large.bin", options.large_file(true))
        .unwrap();
    let copied = io::copy(&mut io::repeat(0).take(expanded_bytes), &mut writer).unwrap();
    assert_eq!(copied, expanded_bytes);
    writer.finish().unwrap();
    package::digest_reader(&mut BufReader::new(File::open(path).unwrap())).unwrap()
}

fn extract_file(
    path: &Path,
    hash: &str,
    destination: &Path,
    limits: PackageLimits,
) -> Result<Manifest, String> {
    package::extract_reader_with_limits(
        BufReader::new(File::open(path).unwrap()),
        hash,
        destination,
        None,
        limits,
    )
}

#[test]
fn streamed_extraction_accepts_more_than_the_previous_200_mib_limit() {
    let directory = working_directory();
    let archive = directory.path().join("large.zip");
    let expanded = 201 * 1024 * 1024;
    let hash = write_archive(&archive, expanded);
    let destination = directory.path().join("extracted");
    let parsed = extract_file(&archive, &hash, &destination, PackageLimits::default()).unwrap();
    assert_eq!(parsed.id, "test.large");
    assert_eq!(
        fs::metadata(destination.join("assets/large.bin"))
            .unwrap()
            .len(),
        expanded
    );
    assert!(fs::metadata(&archive).unwrap().len() < expanded / 10);
}

#[test]
fn package_hash_and_configured_limits_fail_before_output_is_created() {
    let directory = working_directory();
    let archive = directory.path().join("mod.zip");
    let hash = write_archive(&archive, 4096);
    let destination = directory.path().join("unwritten");
    let failure = extract_file(
        &archive,
        &"0".repeat(64),
        &destination,
        PackageLimits::default(),
    )
    .unwrap_err();
    assert!(failure.contains("SHA-256"), "{failure}");
    assert!(!destination.exists());
    for limits in [
        PackageLimits {
            zip_bytes: Some(fs::metadata(&archive).unwrap().len() - 1),
            ..Default::default()
        },
        PackageLimits {
            expanded_bytes: Some(4095),
            ..Default::default()
        },
        PackageLimits {
            files: Some(2),
            ..Default::default()
        },
    ] {
        let failure = extract_file(&archive, &hash, &destination, limits).unwrap_err();
        assert!(failure.contains("configured"), "{failure}");
        assert!(!destination.exists());
    }
}

#[tokio::test]
async fn streamed_author_cli_zip64_installs_directly_from_disk() {
    let directory = working_directory();
    let source = directory.path().join("author");
    fs::create_dir_all(&source).unwrap();
    let mut value = manifest();
    value["future_metadata"] = json!({"compatible":true});
    fs::write(source.join("manifest.json"), value.to_string()).unwrap();
    fs::write(
        source.join("index.html"),
        "<!doctype html><title>Disk install</title>",
    )
    .unwrap();
    File::create(source.join("large.bin"))
        .unwrap()
        .set_len(8 * 1024 * 1024)
        .unwrap();
    let archive = directory.path().join("mod.zip");
    let node = std::env::var_os("SWCH_TEST_NODE")
        .map(PathBuf::from)
        .unwrap_or_else(|| "node".into());
    let result = Command::new(&node)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["mod-sdk/cli.mjs", "pack"])
        .arg(&source)
        .arg(&archive)
        .env("SWCH_MOD_MAX_ZIP_BYTES", "0")
        .env("SWCH_MOD_MAX_EXPANDED_BYTES", "0")
        .env("SWCH_MOD_MAX_FILES", "0")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let packed: Value = serde_json::from_slice(&result.stdout).unwrap();
    let hash = packed["sha256"].as_str().unwrap();
    let mut input = File::open(&archive).unwrap();
    input.seek(SeekFrom::End(-98)).unwrap();
    let mut signature = [0; 4];
    input.read_exact(&mut signature).unwrap();
    assert_eq!(signature, [0x50, 0x4b, 0x06, 0x06]);
    let database = Database::open_in_memory().unwrap();
    database.run_migrations().await.unwrap();
    let manager = ModManager::with_dir(database, directory.path().join("mods"), Some(node));
    assert_eq!(
        manager.install_file(&archive, hash, None).await.unwrap(),
        "test.large"
    );
    let list = manager.list().await;
    assert_eq!(list[0].manifest.id, "test.large");
    manager.shutdown().await;
}

#[test]
fn native_tools_require_declared_hashes_and_keep_node_addons_unsupported() {
    let directory = working_directory();
    let mut value = manifest();
    let helper = b"native helper";
    value["api"] = json!("^1.1.0");
    value["native_tools"] = json!([{"id":"helper","platform":"windows-x64","version":"2.0",
        "license":"MIT","files":[{"path":"native/helper.exe","sha256":package::digest(helper),"executable":true}]}]);
    let build = |value: &Value, name: &str, bytes: &[u8]| {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [
            ("manifest.json", value.to_string().into_bytes()),
            ("index.html", b"<title>Native</title>".to_vec()),
            (name, bytes.to_vec()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(&bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    };
    let bytes = build(&value, "native/helper.exe", helper);
    package::extract(
        &bytes,
        &package::digest(&bytes),
        &directory.path().join("valid"),
        None,
    )
    .unwrap();
    let corrupt = build(&value, "native/helper.exe", b"changed");
    let error = package::extract(
        &corrupt,
        &package::digest(&corrupt),
        &directory.path().join("bad-hash"),
        None,
    )
    .unwrap_err();
    assert!(
        error.to_lowercase().contains("hash")
            || error.to_lowercase().contains("checksum")
            || error.contains("SHA-256"),
        "{error}"
    );
    let addon = build(&manifest(), "addon.NODE", helper);
    let output = directory.path().join("addon");
    assert!(package::extract(&addon, &package::digest(&addon), &output, None).is_err());
    assert!(!output.exists());
    value["native_tools"][0]["version"] = json!("../2.0");
    assert!(serde_json::from_value::<Manifest>(value)
        .unwrap()
        .validate(None)
        .is_err());
}

#[test]
fn catalog_accepts_legacy_entries_and_future_manifest_metadata() {
    let mut value = manifest();
    value["future_metadata"] = json!({"tools":true});
    let catalog: Catalog = serde_json::from_value(json!({"schema":1,"mods":[{"manifest":value,
        "author":"author","repository":"author/mod","repository_id":1,"owner_id":2,
        "release":"https://github.com/author/mod/releases/v1.0.0",
        "download_url":"https://github.com/author/mod/releases/download/v1.0.0/mod.zip","sha256":"a".repeat(64)}]})).unwrap();
    assert!(catalog.mods[0].manifest.native_tools.is_empty());
    catalog.mods[0].manifest.validate(None).unwrap();
    assert!(catalog.mods[0].downloads.is_none());
    let wire = serde_json::json!({"schema":1,"mods":[{"manifest":manifest(),"manifest_extensions":{"games":[{"launcher":"steam","app_id":"42"}],"future":true},"author":"author","repository":"author/mod","repository_id":1,"owner_id":2,"release":"https://github.com/author/mod/releases/v1.0.0","download_url":"https://github.com/author/mod/releases/download/v1.0.0/mod.zip","sha256":"a".repeat(64)}]});
    let parsed: Catalog = serde_json::from_value(wire).unwrap();
    assert_eq!(parsed.mods[0].manifest.games[0].app_id, "42");
}

#[test]
#[ignore = "writes and extracts a real 5 GiB file; run when the project disk has space"]
fn real_five_gib_zip64_file_extracts_without_buffering_the_payload() {
    let directory = working_directory();
    let archive = directory.path().join("five-gib.zip");
    let expanded = 5 * 1024 * 1024 * 1024u64;
    let hash = write_archive(&archive, expanded);
    let mut zip = zip::ZipArchive::new(File::open(&archive).unwrap()).unwrap();
    assert_eq!(zip.by_name("assets/large.bin").unwrap().size(), expanded);
    drop(zip);
    let output = directory.path().join("extracted");
    extract_file(&archive, &hash, &output, PackageLimits::default()).unwrap();
    let mut extracted = File::open(output.join("assets/large.bin")).unwrap();
    assert_eq!(extracted.metadata().unwrap().len(), expanded);
    extracted
        .seek(SeekFrom::Start(4 * 1024 * 1024 * 1024 + 17))
        .unwrap();
    let mut sample = [1; 4096];
    extracted.read_exact(&mut sample).unwrap();
    assert_eq!(sample, [0; 4096]);
}
