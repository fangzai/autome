# Findings

- launcher falls back to PATH bash (WSL here), PATH splitting is heuristic.
- stop_session and dispatch ignore taskkill errors; PID registration is delayed.
- Git common-directory comparison fails on Windows normalized/canonical paths.
- env_probe still searches exact names and domain environment lists iTerm2.
- No .gitattributes exists; golden prompt files are CRLF while rendered strings use LF.
- Root cause of startup failure: non-login Git Bash does not add /usr/bin; wrapper must explicitly supply its utilities. Additional MSYS temporary-path denial is sandbox-specific, not a product permission requirement.
- After fixes, 538 automed unit tests and 10 prompt golden tests pass; happy-path E2E passes with normal Windows permissions (15.96s).

## Subsequent review — current worktree with agy changes

- agy Runtime/adapter/rendering/usage/skills/models IPC and NSIS script now exist; do not describe them as unimplemented wholesale.
- Default roles remain Claude; system runtime selection checks only binary existence.
- runtime.models synchronously calls cmd.output without a deadline, ignores unsuccessful exit and collapses failures to empty data; unit test can call the actual installed agy.
- Model identity still accepts different-runtime defaults and arbitrary string IDs; UI omits retro/impl and does not consume the new model-list API.
- agy is missing from env.install/env.login write allowlist. Install/login assumptions lack capability evidence in the repository.
- Current build passes; 274 domain tests pass. Format and Clippy fail. Desktop/Electron DOM tests pass 166/166 in normal Windows permissions (sandbox-only GPU/cache failure is not a product finding).
- Detailed assessment and next implementation plan: windows_port_next_plan.md.
