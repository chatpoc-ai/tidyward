# Tidyward

Tidyward is a private AI housekeeper for your computer. It works out what each file is, applies rules you write, and suggests what to keep, archive or move to the Trash. Nothing moves until you confirm, and every change can be undone.

> **Status:** design stage. The Rust core is being built; see the [roadmap](docs/roadmap.md). macOS comes first, Windows later.

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

## Example rules

```
# action                    | what it is (asked of the model)   | exact conditions (checked locally)
ask                         | passwords, keys or recovery codes |
archive:Documents/Finance   | invoices, receipts or statements  |
trash                       | a screenshot                      | arrived>90 days, never opened
trash                       | *                                 | extension=.dmg/.pkg, app installed
```

The first rule that applies wins. See [rules.example.txt](rules.example.txt).

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
