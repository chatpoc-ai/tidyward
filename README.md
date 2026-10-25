# Tidyward

Tidyward is a private AI housekeeper for your computer. It works out what each file is, applies rules you write, and suggests what to keep, archive or move to the Trash. Nothing moves until you confirm, and every change can be undone.

> **Status:** early. The core and the command line work on macOS (scan, identify, rules, plan, apply, undo, usage). There is no scheduler or app yet; see the [roadmap](docs/roadmap.md). Windows comes later.

## How it works

```
scan (local) → identify → apply your rules → plan → you confirm → move / Trash (undoable)
                  ↑ remembers files it has seen: unchanged files are not sent again
```

Each part does what it is best at:

1. **Identify locally first.** The system already knows a lot about each file: camera model, screenshot flag, pixel size, the app that made it, and the content type (via [Google Magika](https://github.com/google/magika)). Only files that are still unclear go to the model.
2. **Exact conditions in code.** Age, size, extension, duplicates and file-name patterns are checked locally, with exact numbers.
3. **The model only answers semantic questions,** such as "is this an invoice?" or "is this an ID document?". It returns calibrated yes/no probabilities. Tidyward uses [Jev](https://typesafe.ai), a fast decision model.
4. **Your rules decide.** The model never chooses an action. Your rules turn its answers into a plan, and weak or missing matches are left to you.

See [docs/design.md](docs/design.md) for the details.

## Privacy

- **Masking happens locally; the cloud model only judges.** Real file names, paths and contents never leave your machine. Names are masked so their meaning survives but nothing that identifies a person: `Invoice_Acme_2025-09.pdf` becomes `Invoice_<word>_<date:1-2 years ago>.pdf`.
- **You bring your own key.** You sign up with a model provider yourself (TypeSafe directly, or Vercel AI Gateway). Tidyward has no server and never sees your key, which is stored in the system keychain.
- **You can check it.** The code that decides what is sent lives in this repository.

## Cost

Tidyward is built to be cheap and fast, and a clean-up once a week is enough. It remembers what it has already judged, so after the first run it only pays for new or changed files. Jev costs $0.042 per million input tokens, and output is free.

## Getting started

You need macOS and Rust 1.85 or later. [Magika](https://github.com/google/magika) is optional (`brew install magika`); it helps identify files with misleading extensions.

### 1. Install

```bash
git clone https://github.com/chatpoc-ai/tidyward
cd tidyward
cargo install --path crates/tidyward-cli
```

### 2. Get a key

Tidyward has no server and no account of its own. Sign up with one of these providers and create an API key:

- [TypeSafe](https://typesafe.ai) (direct; the default), or
- [Vercel AI Gateway](https://vercel.com/ai-gateway) (set `provider = "vercel"` in `config.toml`).

### 3. Set up

```bash
tidyward init       # creates config.toml and rules.txt
tidyward key set    # paste your key; it is stored in the macOS keychain
```

Both files live in `~/Library/Application Support/Tidyward/`. `rules.txt` starts with the short, careful [starter rules](rules/starter.txt); see [Writing rules](#writing-rules) to change them. Instead of the keychain you can set `TYPESAFE_API_KEY` (or `AI_GATEWAY_API_KEY`) in the environment. `tidyward key status` tells you which one is used.

### 4. Scan and look at the plan

```bash
tidyward scan       # scans ~/Downloads and writes a plan; nothing is moved
tidyward plan       # what it suggests, and why
```

The plan is grouped into **trash**, **ask you**, **archive → folder** and **keep**. Each line names the rule that decided, and whether code or the model decided it:

```
## trash — 2 items, 411 MB
  Screenshot 2025-03-02 at 10.14.55.png     1.2 MB  R005 (0.97) a screenshot + arrived>30 days + never opened [model]
  SomeApp-4.2.dmg                          410 MB  R006 (1.00) extension=.dmg/.pkg + app installed [code]

## ask you — 1 items, 2 KB
  github-recovery-codes.txt                  2 KB  R001 (0.90) passwords, keys, recovery codes or backup codes [model]

## archive → Documents/Receipts — 1 items, 88 KB
  Invoice_Acme_2025-09.pdf                  88 KB  R008 (0.98) an invoice, receipt or bill [model]
```

The number in brackets is how sure it is. "ask you" means Tidyward will not touch the file: either a protective rule matched, or it was not sure enough.

The first scan asks about every file; later scans only ask about new or changed files. A Downloads folder of about 200 files costs under one cent the first time. `tidyward scan --full` asks about everything again.

### 5. Apply, and undo if needed

```bash
tidyward apply      # asks y/N for each archive folder, and for each item that goes to the Trash
tidyward undo       # puts everything from the last apply back
```

Pressing Return means no. Files that changed after the plan was made are skipped. Items go to the macOS Trash, not deleted, and `undo` puts them back where they were.

### Try it on a copy first

To see how it behaves without touching your real files, use a separate data folder and a copy:

```bash
cp -R ~/Downloads /tmp/tidyward-try
tidyward --home /tmp/tidyward-home init
tidyward --home /tmp/tidyward-home scan /tmp/tidyward-try
tidyward --home /tmp/tidyward-home plan
```

Set `archive_root = "/tmp/tidyward-try-archive"` in `/tmp/tidyward-home/config.toml` if you also want to try `apply` without anything reaching your Documents folder.

### Other commands

| Command | What it does |
|---|---|
| `tidyward usage` | Tokens and cost per run, in total, and an estimate per month |
| `tidyward report` | What is in the folder, what the plan proposes, the largest items |
| `tidyward rules` | Checks `rules.txt` and marks rules the latest plan did not use |
| `tidyward history` | Recent moves and Trash operations, with their numbers for `undo <id>` |

## Writing rules

`rules.txt` has one rule per line, in three parts:

```
action | what the file is | exact conditions
```

- **action**: `keep`, `trash`, `ask`, or `archive:<folder>`. Archive folders are inside your home folder (`archive:Documents/Receipts` means `~/Documents/Receipts`).
- **what the file is**: plain language, such as `a screenshot` or `an invoice, receipt or bill`. The model judges it from the masked name and metadata, never from the real name or contents. `*` means any file.
- **exact conditions** (optional): checked on your Mac, not by the model. Separate them with commas; all must hold.

| Condition | Holds when |
|---|---|
| `arrived>30 days`, `arrived<7 days` | the file arrived on this Mac more or less than N days ago |
| `opened>90 days`, `opened<7 days` | it was last opened more or less than N days ago (`opened>N` also holds if never opened) |
| `never opened` | it has never been opened |
| `size>100 MB`, `size<1 MB` | its size |
| `extension=.dmg/.pkg` | its extension is one of these |
| `has duplicate` | it is an extra copy of another file here (the earliest copy is never counted) |
| `app installed` | it is an installer for an app that is already installed |
| `name starts with IMG_`, `name contains invoice` | the real name, checked locally |

**The first rule that applies wins**, so put protective rules (`ask`, `keep`) at the top. A file no rule applies to is left to you, so end with `keep | * |` if you want everything else left alone.

Start with the [starter rules](rules/starter.txt), look at the plan, and adjust. Changing a rule only costs the questions for that rule; answers to the others are remembered. [rules/more-examples.txt](rules/more-examples.txt) has more ideas, such as sorting photos, screenshots and videos into folders. Every keyword also has a Chinese form; see [docs/design.md](docs/design.md).

### Settings (`config.toml`)

| Setting | Default | Meaning |
|---|---|---|
| `folders` | `["~/Downloads"]` | Folders to scan |
| `provider` | `"typesafe"` | `"typesafe"` or `"vercel"` |
| `archive_root` | `"~"` | Where `archive:` folders are created |
| `send_domain` | `true` | Send the domain a file was downloaded from (domain only) |
| `magika` | `true` | Use Magika when it is installed |
| `hit`, `weak` | `0.7`, `0.4` | Confidence for a rule to apply; between the two, the file is left to you |
| `batch` | `40` | Files per model call |

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
