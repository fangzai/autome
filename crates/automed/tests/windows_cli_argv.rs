//! A real Windows session wrapper, with a fake CLI; never invokes a model.
#![cfg(windows)]

use std::path::Path;
use std::time::{Duration, Instant};

use autome_domain::role::Runtime;
use automed::launcher::{LaunchMode, LaunchSpec, launch, which};

#[test]
fn batch_sessions_preserve_toml_model_effort_stdin_and_exit_status() {
    let Some(node) = which("node") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("autome-batch-argv-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let helper = env!("CARGO_BIN_EXE_automed");
    // This integration binary has a process of its own; no other test shares
    // these overrides. Both rendering and the native bridge use the real core.
    unsafe {
        std::env::set_var("AUTOMED_RENDERER_BINARY", helper);
    }
    for (index, exit_code) in [0, 17].into_iter().enumerate() {
        let repo = root.join(format!("中文 space {index}"));
        automed::init::init(&repo, automed::protocol::seed()).unwrap();
        let capture = repo.join("capture.cjs");
        std::fs::write(&capture, format!(
            "const fs=require('node:fs'); console.log(JSON.stringify({{args:process.argv.slice(2),stdin:fs.readFileSync(0,'utf8')}})); process.exit({exit_code});"
        )).unwrap();
        let shim = repo.join(if index == 0 { "fake.CMD" } else { "fake.bat" });
        std::fs::write(&shim, format!("@\"{node}\" \"%~dp0capture.cjs\" %*\r\n")).unwrap();
        unsafe {
            std::env::set_var("AUTOMED_CODEX_BINARY", &shim);
        }
        let roots = if index == 0 {
            r#"sandbox_workspace_write.writable_roots=["D:/AgentCode/autome/.git"]"#
        } else {
            r#"sandbox_workspace_write.writable_roots=["D:/AgentCode/autome/.git","D:/中文 space/.git"]"#
        };
        let args = vec![
            "--config".into(),
            roots.into(),
            "--model".into(),
            "gpt-6.1-sol".into(),
            "--config".into(),
            "model_reasoning_effort=medium".into(),
        ];
        let session = format!("argv-{index}");
        let launched = launch(&LaunchSpec {
            session_id: &session,
            task_id: "T-argv",
            cwd: &repo,
            repo: &repo,
            runtime: Runtime::Codex,
            args: args.clone(),
            prompt: "测试 stdin".into(),
            mode: LaunchMode::Headless,
        })
        .unwrap();
        let output = repo.join(".autome/output/sessions/T-argv");
        let marker = output.join(format!("{session}.exit"));
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if std::fs::read_to_string(&marker).is_ok_and(|s| s.contains("marker_end")) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "session timed out: {}",
                std::fs::read_to_string(&launched.log_path).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = std::fs::read_to_string(output.join(format!("{session}.jsonl"))).unwrap();
        let captured: serde_json::Value =
            serde_json::from_str(text.lines().next().unwrap()).unwrap();
        assert_eq!(captured["args"], serde_json::json!(args));
        assert_eq!(captured["stdin"], "测试 stdin");
        assert!(
            std::fs::read_to_string(&marker)
                .unwrap()
                .contains(&format!("exit_code={exit_code}"))
        );
        assert!(Path::new(&launched.log_path).is_file());
    }
    let _ = std::fs::remove_dir_all(root);
}
