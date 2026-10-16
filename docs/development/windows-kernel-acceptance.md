# Windows kernel acceptance — 2026-10-16

Scope: native Windows core, Git Bash session wrapper, process lifecycle, Git/worktrees, environment discovery and Electron sidecar protocol. This does not certify the agy adapter, installer or a paid model loop.

## Reproduction

Run from a regular Windows PowerShell terminal with Rust/MSVC, Git for Windows and Node installed:

```powershell
cd D:\AgentCode\autome
./scripts/ci-windows.ps1 -Offline
```

The script builds the actual binary before tests, because Windows session Jobs use an independent `automed session-job-keeper` process. It does not enable `AUTOMED_REAL_CLI` or `AUTOMED_REAL_LOOP`. Ordinary Node tests work without downloading the Electron GUI runtime; the actual Electron DOM test explicitly skips when that runtime is absent.

## Evidence

- Native build and Clippy with `-D warnings`: passed.
- Domain: 271 passed.
- Core: 541 passed in the full run, including immediate Stop, PID identity rejection and child-process termination.
- End-to-end: all 33 passed, including the previously failing clean-worktree case, merge commits, multi-repository tasks, recovery, Stop/Cancel and usage recording.
- Prompt golden tests: all 10 passed.
- Desktop Node tests: 103 passed, 1 explicitly skipped (Electron GUI/DOM runtime unavailable).
- The full `ci-windows.ps1 -Offline` command exited successfully. Paid `real_cli` tests remain gated; their test-runner entries are not evidence of a real-model loop.
- Git Bash version probe: GNU Bash patch-level syntax was not parsed. Fixed and added one passing regression test after the full run; rebuilt the binary and reran format/Clippy successfully.
- Real JSON-RPC smoke test in a Chinese, space-containing state directory: passed. Detected Git `2.52.0.windows.1`, Codex `0.154.0` with login OK, Git Bash `5.2.37`; Claude correctly reported absent. No inference calls were made.
- Full log: `target/windows-acceptance.log` (generated locally, ignored by Git).
- Probe result: `target/windows-environment-smoke.json`; added regression log: `target/windows-bash-version-test.log`.

Verdict: Windows kernel automated acceptance passed. This verdict is scoped to the implementation and test coverage above, not the remaining product acceptance below.

## Implementation boundaries

- Git Bash is validated against a Git/MSYS installation; it never falls back to the WSL bash on PATH.
- The wrapper starts suspended and is assigned to a Windows Job before any child can spawn. A separate keeper retains the Job handle across core restarts. Stop reconnects using PID plus creation time, terminates the original Job and waits for member process handles to signal exit.
- Legacy sessions without identity metadata are refused by Stop instead of risking a reused PID. Cancel preserves the worktree when termination fails.
- Generated script attributes are merged into existing `.gitattributes`, preserved on repeated initialization, and included in the init commit.
- The Node sidecar tests exercise the real `automed.exe`; fixture-based CLI sessions exercise real Git and merge commits. Real model calls remain separate acceptance work.

## Remaining product acceptance

- Actual Electron GUI/DOM test requires the Electron distribution; its download previously failed with a terminated network request.
- Installer/Start Menu behavior and a real multi-model loop are later phases of `windows_port_plan.md`.
- macOS regression is retained in CI but cannot be executed on this Windows host.
