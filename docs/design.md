# Design

This document describes how Tidyward decides what to suggest, and exactly what leaves your machine.

## Principles

1. **Suggest; never act alone.** Tidyward produces a plan. Moves and deletions happen only after you confirm them. A deletion sends the file to the Trash, never a hard delete, and every operation is logged so it can be undone.
2. **Masking happens locally; the cloud model only judges.** Real names, paths and contents never leave the machine.
3. **Each part does what it is best at.** Local code handles anything exact. The model handles only questions of meaning.
4. **Cheap and fast.** A weekly run is enough. Files that have already been judged are not sent again.

## Pipeline

```
1. scan       local     size, dates, duplicates, system metadata, content type
2. identify   local     camera, screenshot, extension ...        → most files end here
              model     what is this? (only for files still unclear)
3. rules      local     exact conditions: age, size, extension, duplicates, name
              model     semantic conditions: "an invoice", "an ID document" ...
4. plan       local     first rule that applies wins; weak or no match → ask you
5. confirm    you
6. execute    local     move / Trash, logged, undoable
```

### 1. Scan

For each item, Tidyward collects:

- size, arrival date, last opened, last modified
- a content fingerprint, used to find exact duplicates. Each group of duplicates keeps one original, the earliest to arrive, and only the extra copies count as duplicates.
- system metadata. On macOS this comes from Spotlight: camera model, screenshot flag, pixel size, the app that created the file, page count and download source.
- content type from [Magika](https://github.com/google/magika). This is used only when a file has no extension, or when the extension clearly contradicts the content. Magika reads container formats as their wrapper (HEIC as MP4, 3MF as ZIP), so it never overrides the extension on its own.
- for installers, whether the matching app is already installed

### 2. Identify

Local signals come first, and the first sure signal wins:

| Signal | Result |
|---|---|
| camera model present | photo (or video) |
| system screenshot flag | screenshot |
| no camera, and the size exactly matches a phone or tablet screen | screenshot |
| name says screenshot or screen recording | screenshot |
| extension: installer, video/audio, 3D model, code, partial download | that purpose |

The screen-size table leaves out sizes that are common for other images (for example 1080×1920 wallpapers). Everything else goes to the model with a single question: *what is this?*

### 3. Rules

Rules live in a plain text file, one per line:

```
action | semantic condition | exact conditions
```

- **action**: `keep`, `archive:<folder>`, `trash` or `ask`
- **semantic condition**: plain language, judged by the model; `*` means any
- **exact conditions**: checked locally, comma-separated, all must hold:
  `arrived>N days`, `arrived<N days`, `opened>N days`, `opened<N days`, `never opened`, `size>N MB`, `size<N MB`,
  `extension=.a/.b`, `has duplicate`, `app installed`, `name starts with X`, `name contains X`.
  Name conditions use the real name and are checked on the machine.
  The keywords also have Chinese forms (`到达>N天`, `从未打开`, `扩展名=`, `有副本`, `名称以X开头`, ...).

Order is priority: the first rule that applies wins. Protective rules (IDs, keys, financial papers) belong at the top.

Evaluation goes in two steps:

1. **Local.** Check the exact conditions of every rule and drop the rules that fail. If the first rule left standing has no semantic condition, local code decides and the model is not asked.
2. **Model.** Ask only the semantic conditions of the rules still standing before that point. Each condition is a yes/no question, and the model returns a probability.

A semantic condition counts as matched at probability ≥ 0.7. If the first candidate is between 0.4 and 0.7, or no rule applies, the item is left to you.

Why yes/no questions instead of "pick a rule": a decision model asked to choose rarely picks "none of these". A yes/no question per rule lets "nothing applies" emerge naturally.

Why exact conditions stay local: comparing "3-6 months ago" with "older than 90 days" is unreliable for a model and trivial for code. Exact checks are also free, and they change with time, so they are recomputed on every run.

### 4. Plan

Each item gets a decision, the rule that produced it, and whether local code or the model decided. Nothing is changed until you confirm.

## What is sent to the model

Only words, never raw values, because decision models read bucketed words better than numbers:

| Field | Example |
|---|---|
| masked file name | `Invoice_<word>_<date:1-2 years ago>.pdf` |
| type | `document (.pdf)` |
| size | `small (under 10 MB)` |
| arrived / opened / modified | `3-6 months ago`, `never opened since it arrived` |
| duplicate | `yes, and this one is the extra copy` |
| made by (category only) | `a web browser`, `an automated system (generated PDF)` |
| pages | `2-5 pages` |
| download source | domain only; can be turned off |
| clues from the name, computed locally | `looks like a screenshot`, `has a copy suffix such as (1)` |
| what layer 1 said | `a screenshot (certain, camera metadata)` |

The path, the real name, the contents and the exact app name and version are never sent.

### Masking

Generic words that say what kind of file something is (IMG, Screenshot, Invoice, Resume, contract, setup, arm64, backup, recovery, and their Chinese equivalents) are kept. Everything else becomes a placeholder:

| Placeholder | Stands for |
|---|---|
| `<date:…>` | a date in the name, turned into how long ago it was (the month of an invoice still matters) |
| `<time>` `<version>` `<id>` `<n>` | clock times, versions, hex or UUID ids, other numbers |
| `<word>` | any Latin word not in the allowlist: names, companies, topics |
| `<中文>` | any Chinese run not in the allowlist |

Runs of placeholders are merged, so a name's length does not leak.

```
IMG_4821.HEIC                 → IMG_<n>.HEIC
Screenshot 2026-10-01 at 9.12.03 AM.png → Screenshot <date:this month> at <time>.png
Jane Doe Resume (1).docx      → <word> Resume (<n>).docx
张三_劳动合同_签字版.pdf         → <中文>_劳动合同_签字版.pdf
```

The allowlist is the part most likely to need care. A generic word missing from it hides useful meaning: if `code` is missing, `…_backup_code.txt` no longer looks like a secret.

## Batching

A decision model judges each question against the state on its own, so one call can carry hundreds of questions. The limit is the size of the state. Tidyward puts up to about 40 files in one call, keyed by an item id, with each question naming the item it is about.

## File memory

A local database remembers, for each file, its fingerprint (size + modification time + masked name), the identify result, and every semantic answer.

| Situation | What is sent |
|---|---|
| file unchanged | nothing |
| new or changed file | only that file |
| new rule added | only that rule's semantic condition |
| file moved (same content, same masked name) | nothing |
| masking rules changed | files whose masked name changed |

Decisions are never cached. Exact conditions are recomputed on every run, because "not opened for 90 days" changes with time. The weekly cost therefore depends on how many files are new, not on how many files the disk holds.

## Model providers

Tidyward calls Jev through a provider the user signs up with:

| Provider | Endpoint | Model |
|---|---|---|
| TypeSafe (direct) | `POST https://api.typesafe.ai/v1/systemone` | `jev-latest` |
| Vercel AI Gateway | `POST https://ai-gateway.vercel.sh/v1/evaluate` | `typesafe-ai/jev` |

The two differ in small ways. For example, the direct API spells the boolean type `noul`. A provider layer hides those differences. Every call is logged locally with its token count, so the user can see what each run cost.
