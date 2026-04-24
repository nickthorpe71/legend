# Git integration audit (#27)

**Recorded:** 2026-04-24

Closes queue item #27: "Audit git integration hooks beyond merge
driver." Captures what's installed today, what's missing, and which
gaps are worth filling.

## What's installed today

`legend init` (and re-init) runs `setup_git_merge_driver()` which sets
up three pieces:

1. **Custom merge driver** (`merge.legend.driver` in `git config`)
   - Maps `.legend/memory.lz4` and `.legend/events.jsonl` to a
     three-way merge handler (`legend git-merge-driver` subcommand).
   - Resolves binary state-file conflicts during pulls and merges.
2. **`.gitattributes`** entries declaring those two files use the
   `legend` merge driver. Idempotent — old patterns are pruned.
3. **`post-merge` hook** at `.git/hooks/post-merge`.
   - Triggered after `git pull` / `git merge` succeeds.
   - If `.legend/memory.lz4` changed in the merge, runs
     `legend merge-local <pre-pull-state>` to fold the user's
     pre-pull state back in (recovers data the fast-forward would
     otherwise have dropped).
   - Idempotent via `# --- Legend post-merge: ... # --- End Legend
     post-merge ---` markers.

`tests/conformance_merge_driver.rs` covers all three.

## Hooks that exist in git but Legend does not install

| Hook                  | Purpose if installed                                          |
|-----------------------|---------------------------------------------------------------|
| `pre-commit`          | Warn if `.legend/memory.lz4` is dirty but unstaged            |
| `prepare-commit-msg`  | Auto-tick a memory entry from the commit message              |
| `post-commit`         | Checkpoint the daemon so the WAL is empty after each commit   |
| `pre-push`            | Block push if WAL has unflushed mutations (rare; daemon stop) |
| `post-checkout`       | Refresh the daemon's in-RAM state after `git checkout` swaps the file under it |
| `post-rewrite`        | Like post-merge, but for `git rebase` / `commit --amend`      |

## Gaps and recommendations

### Worth adding

- **`post-checkout`**. When the user switches branches, the daemon's
  in-RAM state diverges from the on-disk file. Today the user has to
  `legend daemon stop` manually. A hook that does
  `legend daemon stop` (or a future `legend daemon reload`) would
  prevent silent state divergence.
- **`post-rewrite`**. `git rebase` is not covered by `post-merge`.
  After a rebase that touched `.legend/memory.lz4`, the daemon's
  in-RAM state is wrong. Same fix as `post-checkout`.

### Worth considering

- **`pre-commit` warning**. Surface "you've ticked memories since the
  last commit; consider staging `.legend/`". Aligns with the user
  feedback memory ("commit .legend/ state files with every commit").
  Not a hard block — just a one-liner reminder.

### Not recommended

- **Auto-stage on `prepare-commit-msg`**. Surprising the user by
  staging files they didn't `git add` is a trust-busting move. Skip.
- **Auto-tick from commit message**. The reverse direction (commit
  message → memory tick) duplicates what the user already does
  manually with `legend memory tick` and would create a feedback
  loop with the post-commit checkpoint. Skip.

## Decision for this audit

- Document the current set (this file).
- Apply no hook changes in this commit. The recommended additions
  (`post-checkout`, `post-rewrite`, optional `pre-commit` warning)
  affect daily user workflow and should land with their own queue
  items + conformance tests, not as a drive-by addition.
- Future queue items can pick up the recommendations:
  - "Add `post-checkout` hook to refresh daemon state after branch
    switch"
  - "Add `post-rewrite` hook for state recovery after rebase"
  - "Add `pre-commit` warning when `.legend/` is dirty"

## Related

- `tests/conformance_merge_driver.rs` — coverage for the existing
  three pieces.
- `docs/init-rerun-behavior.md` (#26): re-init refreshes hooks via
  marker idempotency, so adding new hooks later is safe.
- User feedback memory: `commit .legend/ state files with every
  commit` — the `pre-commit` warning would surface this convention to
  new contributors.
