# Roadmap

## Layout

A Cargo workspace. The core logic is a library, and the command line is a thin shell over it.

```
crates/
  tidyward-core/       masking, local identification, rule parsing, the pipeline, plans (no I/O)
  tidyward-platform/   scanning and system metadata (macOS: Spotlight, FSEvents; Windows later)
  tidyward-jev/        model providers, batching, retries, token accounting
  tidyward-store/      SQLite: file memory, token ledger, plans, operation log
  tidyward-exec/       move / Trash with an operation log, undo
  tidyward-cli/        the `tidyward` command
```

The core is meant to expose extension points (not done yet), so new capabilities become new implementations rather than changes to the pipeline:
`Source` (where files come from), `Identifier` (local signals), `Condition` (exact rule conditions), `Action` (what can be done) and `Provider` (model APIs).

## Milestones

| | Scope | Status |
|---|---|---|
| **M0** | Workspace, masking, rule parser (English and Chinese keywords), local identification | Done |
| **M1** | Scanning, Spotlight metadata, Magika, duplicates, file memory | Done |
| **M2** | Providers (TypeSafe, Vercel), batching, token ledger, `tidyward usage` | Done |
| **M3** | `tidyward apply` / `tidyward undo`, Trash with a known location | Done; verified in temporary folders |
| **M4** | `tidyward report` (done); weekly schedule (launchd); FSEvents-based change detection | Partly done |
| Later | Windows platform layer, more sources (external drives, iCloud Drive) | |

Known gaps:

- Spotlight metadata is read with batched `mdls` runs (100 files per run), not the native MDItem API yet.
- Magika is used through its command-line tool when installed; it is not linked as a library.
- `tidyward key set` (keychain storage) is implemented, but the tests use environment variables, so it has no automated test.
- There are no extension-point traits yet; the pieces are plain modules.

## Command line

```bash
tidyward scan [--full]     # scan, identify, apply rules, write a plan; incremental by default
tidyward plan              # show the latest plan, grouped by Trash / ask / archive / keep
tidyward apply             # confirm group by group; each Trash item is confirmed on its own
tidyward undo [id]         # undo an operation
tidyward rules             # validate the rules file, mark rules the latest plan did not use
tidyward usage             # tokens and cost: this run, total, estimate per week
tidyward key set           # store the provider key in the keychain
tidyward key status        # where the key would come from (never printed)
tidyward history           # recent operations
tidyward report            # what is in the folder, what the plan proposes, the largest items
tidyward init              # create config.toml and rules.txt
```

## Testing

- Masking, identification, exact conditions and rule parsing are pure functions with unit tests. Test data is made up; no real file names go into the repository.
- `crates/tidyward-cli/tests/offline.rs` runs scan, memory, apply, skip-if-changed and undo end to end, with exact-only rules, so it needs no model and no network.
- Anything that moves or deletes runs only inside temporary directories.
