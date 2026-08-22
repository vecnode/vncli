Keep track of your bibliography across machines

`references.bib` is generated from the local Zotero library by `vn bib sync`. It is
committed, so the other machine gets the bibliography from git rather than needing Zotero
or a Zotero account.

```bash
vn bib status    # compare Zotero against the checked-in .bib, write nothing
vn bib sync      # merge the library into references.bib
```

Both are also in the TUI's root menu. `vn bib` on its own runs `status`.

## The three guarantees

**It never removes a reference.** Sync merges into the existing file. There is no `--prune`
and no other mode that drops an entry — not for items deleted in Zotero, not for entries
you typed into the `.bib` by hand. A reference that silently disappears breaks every
document citing it; a stale entry costs nothing. Anything in the file that is not currently
in Zotero is reported as `kept - sync never removes` and left alone.

**Citation keys are pinned.** `citekeys.json` maps each Zotero item key to the citekey it
was first assigned, and a key is never reissued. Fix a typo in a title in Zotero and the
citekey stays put, so a `\cite{}` written months ago keeps resolving. Keys look like
`lecun2019rise` — first author, year, first non-stopword of the title, with `a`/`b`
suffixes for collisions.

**Output is deterministic.** Entries are sorted by citekey, fields come in a fixed order,
and nothing time-varying is written into the file — no generation timestamp. Re-running
against an unchanged library reproduces the file byte for byte, so `git diff` only ever
shows real bibliography changes.

## Duplicates

The library currently holds 1,228 items that are only 708 distinct records — most things
were imported twice. Sync collapses items that would render as the same BibTeX entry, and
reports how many it merged.

The comparison is on the *rendered* entry, so fields the `.bib` never emits (`accessDate`,
`libraryCatalog`, `shortTitle`) don't keep two otherwise identical records apart. Every
duplicate's Zotero key is pinned to the same citekey, which means merging those duplicates
inside Zotero later leaves the `.bib` unchanged — whichever item key survives the merge
already points at the right citekey.

`--no-dedupe` emits one entry per Zotero item instead.

## Options

```bash
vn bib sync --dry-run              # report what would change, write nothing
vn bib sync --no-dedupe            # one entry per Zotero item
vn bib sync --data-dir <path>      # non-default Zotero data directory
vn bib sync --out <path>           # write somewhere other than zotero/references.bib
```

The Zotero data directory defaults to `~/Zotero`, which is where Zotero puts it on
Windows, Linux and macOS alike. To override it permanently, add to `config.toml`:

```toml
[zotero]
data_dir = "~/some/other/Zotero"
bib_path = "~/dev/vncli/zotero/references.bib"
```

## Notes

- Sync reads a **temporary copy** of `zotero.sqlite` (plus `-wal`/`-shm`), never the live
  file, so it works whether or not Zotero is running and cannot corrupt the library. The
  copy is deleted when the command exits, including on error.
- Attachments, notes, annotations and trashed items are excluded.
- Local file paths are never written to the `.bib` — they are machine-specific and would
  leak your directory layout into the repo.
- Titles, journals and series are brace-protected (`{{...}}`) so styles don't lowercase
  acronyms like GAN, AI or StyleGAN3.
- The file is UTF-8. Use `biber`/`biblatex` rather than legacy `bibtex` for accented names.
