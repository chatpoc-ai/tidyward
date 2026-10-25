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

The core exposes extension points, so new capabilities are new implementations rather than changes to the pipeline:
`Source` (where files come from), `Identifier` (local signals), `Condition` (exact rule conditions), `Action` (what can be done) and `Provider` (model APIs).

## Milestones

| | Scope | Done when |
|---|---|---|
| **M0** | Workspace, masking, rule parser, local identification | Unit tests cover the masking and rule behaviour described in [design.md](design.md) |
| **M1** | Scanning, Spotlight metadata, Magika, duplicates, file memory | `tidyward scan` identifies files offline |
| **M2** | Providers, batching, token ledger, `tidyward usage` | A full plan is produced; replayed model answers give the same plan with no network |
| **M3** | `tidyward apply` / `tidyward undo` | Move, Trash and undo verified in temporary folders only |
| **M4** | Weekly schedule (launchd), `tidyward report`, FSEvents-based incremental scans | A weekly run pays only for new files |
| Later | Windows platform layer, more sources (external drives, iCloud Drive) | |

## Command line (planned)

```bash
tidyward scan [--full]     # scan, identify, apply rules, write a plan; incremental by default
tidyward plan              # show the latest plan, grouped by Trash / ask / archive / keep
tidyward apply             # confirm group by group; each Trash item is confirmed on its own
tidyward undo [id]         # undo an operation
tidyward rules check       # validate the rules file, list rules that never matched
tidyward usage             # tokens and cost: this run, total, estimate per week
tidyward key set           # store the provider key in the keychain
```

## Testing

- Masking, identification, exact conditions and rule parsing are pure functions with unit tests. Test data is made up; no real file names go into the repository.
- Pipeline tests replay recorded model answers, so they run offline and cost nothing.
- Anything that moves or deletes runs only inside temporary directories.
