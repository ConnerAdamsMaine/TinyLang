use tinyone_xtask::{Task, plan_for};

#[test]
fn release_gate_expands_to_ci_order() {
    let plan = plan_for(Task::ReleaseGate);
    let labels: Vec<_> = plan.steps.iter().map(|step| step.label).collect();

    assert_eq!(
        labels,
        [
            "check",
            "test",
            "test-hooks",
            "fmt-check",
            "clippy",
            "bench-smoke",
            "tools-test",
        ]
    );
}

#[test]
fn command_plans_use_current_repo_paths() {
    let check = plan_for(Task::Check).render_commands();
    assert!(check.iter().any(|cmd| cmd.contains("xtask/Cargo.toml")));
    assert!(check.iter().any(|cmd| cmd.contains("tinyone_core/Cargo.toml")));
    assert!(check.iter().any(|cmd| cmd.contains("tinyone_ralloc/Cargo.toml")));

    let hooks = plan_for(Task::TestHooks).render_commands();
    assert!(hooks.iter().any(|cmd| cmd.contains("--features testing-hooks")));

    let tools = plan_for(Task::ToolsTest).render_commands();
    assert!(tools.iter().all(|cmd| cmd.starts_with("uv run --no-project python ")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools.test_abi_manifest")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools.test_hash")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools.test_loc")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools.test_zip")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools/abi_manifest.py")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools/hash.py")));
    assert!(tools.iter().any(|cmd| cmd.contains("tools/loc.py")));
}

#[test]
fn formatting_commands_select_nightly_without_changing_other_cargo_tasks() {
    let expected = vec![
        "rustup run nightly cargo fmt --manifest-path crates/xtask/Cargo.toml --all --check",
        "rustup run nightly cargo fmt --manifest-path crates/tinyone_core/Cargo.toml --all --check",
        "rustup run nightly cargo fmt --manifest-path crates/tinyone_ralloc/Cargo.toml --all --check",
    ];
    assert_eq!(plan_for(Task::FmtCheck).render_commands(), expected);

    let release = plan_for(Task::ReleaseGate);
    let release_fmt = release
        .steps
        .iter()
        .find(|step| step.label == "fmt-check")
        .expect("format stage");
    let commands: Vec<_> = release_fmt.commands.iter().map(|command| command.render()).collect();
    assert_eq!(commands, expected);

    for task in [Task::Check, Task::Test, Task::TestHooks, Task::Clippy, Task::BenchSmoke] {
        assert!(
            plan_for(task)
                .render_commands()
                .iter()
                .all(|command| command.starts_with("cargo "))
        );
    }
}
