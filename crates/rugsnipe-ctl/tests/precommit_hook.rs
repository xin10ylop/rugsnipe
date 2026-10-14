//! The tracked `.githooks/pre-commit` (docs/06 section 13), driven in a
//! throwaway git repository: key material is refused (files under `keys/`,
//! 64-byte keypair arrays, PEM private keys), ordinary files and
//! 64-element non-key lists (tx fixture account indexes) pass. Key-like
//! strings are assembled at run time so this file itself passes the hook.

use std::path::{Path, PathBuf};
use std::process::Command;

fn hooks_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.githooks")
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
        ])
        .args(args)
        .output()
        .expect("git")
}

/// Stage `files` in a fresh commit attempt; `true` if the commit went through.
fn try_commit(dir: &Path, files: &[(&str, String)]) -> (bool, String) {
    for (name, body) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        assert!(git(dir, &["add", "-f", name]).status.success());
    }
    let out = git(dir, &["commit", "-q", "-m", "t"]);
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    if !out.status.success() {
        // leave the index clean for the next case
        let _ = git(dir, &["reset", "-q"]);
        for (name, _) in files {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
    (out.status.success(), err)
}

fn bytes_array(n: usize, sep: &str) -> String {
    let v: Vec<String> = (0..n).map(|i| ((i * 37 + 11) % 256).to_string()).collect();
    format!("[{}]", v.join(sep))
}

#[test]
fn pre_commit_hook_refuses_key_material() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git not available: skipped");
        return;
    }
    let hook = hooks_dir().join("pre-commit");
    assert!(hook.exists(), "tracked hook missing");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&hook).unwrap().permissions().mode();
        assert!(mode & 0o111 != 0, "hook must be executable");
    }
    let tmp = tempfile::Builder::new()
        .prefix("rugsnipe-hook-test-")
        .tempdir()
        .unwrap();
    let dir = tmp.path();
    assert!(git(dir, &["init", "-q"]).status.success());
    let hooks = hooks_dir().canonicalize().unwrap();
    assert!(
        git(dir, &["config", "core.hooksPath", hooks.to_str().unwrap()])
            .status
            .success()
    );

    let pem = format!(
        "-----BEGIN {} KEY-----\nMC4CAQAwBQYDK2VwBCIEIA==\n-----END {} KEY-----\n",
        "PRIVATE", "PRIVATE"
    );
    let ec_pem = format!("x\n-----BEGIN EC {} KEY-----\nAAAA\n", "PRIVATE");
    let refused: Vec<(&str, Vec<(&str, String)>)> = vec![
        ("keys/ dir", vec![("keys/readme.txt", "hello".into())]),
        ("nested keys/ dir", vec![("deploy/keys/x.txt", "x".into())]),
        ("keypair file", vec![("wallet.json", bytes_array(64, ","))]),
        (
            "pretty-printed keypair",
            vec![("dev.json", bytes_array(64, ",\n  "))],
        ),
        (
            "keypair under a key-like name",
            vec![(
                "cfg.json",
                format!("{{\"a\": 1, \"secret_key\": {}}}", bytes_array(64, ", ")),
            )],
        ),
        (
            "keypair in TOML",
            vec![(
                "x.toml",
                format!("[w]\nkeypair = {}\n", bytes_array(64, ", ")),
            )],
        ),
        // F3: keypairs that are neither the whole file nor under a key-like
        // name passed the hook before
        (
            "keypair as a Rust const",
            vec![(
                "k.rs",
                format!("pub const HOT: [u8; 64] = {};\n", bytes_array(64, ", ")),
            )],
        ),
        (
            "keypair under a short name",
            vec![("kp.json", format!("{{\"kp\": {}}}", bytes_array(64, ",")))],
        ),
        (
            "keypair after a comment line",
            vec![(
                "dev.txt",
                format!("// dev wallet\n{}\n", bytes_array(64, ",")),
            )],
        ),
        (
            "keypair in Python bytes()",
            vec![("s.py", format!("S = bytes({})\n", bytes_array(64, ", ")))],
        ),
        (
            "keypair in Uint8Array.from()",
            vec![(
                "s.ts",
                format!("const sk = Uint8Array.from({});\n", bytes_array(64, ",")),
            )],
        ),
        ("PEM private key", vec![("id.pem.txt", pem)]),
        ("EC PEM private key", vec![("notes.md", ec_pem)]),
    ];
    let allowed: Vec<(&str, Vec<(&str, String)>)> = vec![
        ("plain file", vec![("README.md", "rugsnipe\n".into())]),
        (
            "64 account indexes in a tx fixture",
            vec![(
                "fixture.json",
                format!(
                    "{{\"instructions\": [{{\"accounts\": {}}}]}}",
                    bytes_array(64, ",")
                ),
            )],
        ),
        (
            "the tracked fixture with a 64-entry accounts list",
            vec![(
                "fx.json",
                std::fs::read_to_string(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../pump-codec/fixtures/tx_json/fresh_v1_sell_v3_then_amm_sell_v2.json"
                ))
                .unwrap(),
            )],
        ),
        ("63-byte array", vec![("a.json", bytes_array(63, ","))]),
        ("65-byte array", vec![("b.json", bytes_array(65, ","))]),
        (
            "public PEM",
            vec![("pub.pem.txt", "-----BEGIN PUBLIC KEY-----\nAAAA\n".into())],
        ),
    ];
    for (what, files) in &allowed {
        let (ok, err) = try_commit(dir, files);
        assert!(ok, "{what} must be allowed: {err}");
    }
    for (what, files) in &refused {
        let (ok, err) = try_commit(dir, files);
        assert!(!ok, "{what} must be refused");
        assert!(err.contains("pre-commit: REFUSED"), "{what}: {err}");
    }
}
