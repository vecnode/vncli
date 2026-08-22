//! `vn bib` - export the local Zotero library to a git-tracked BibTeX file.
//!
//! Reads a *snapshot copy* of `zotero.sqlite` rather than the live database:
//! Zotero holds a lock while running, and with WAL enabled the main file alone
//! can be missing recent commits. Copying the three files and opening the copy
//! read-only means this works whether or not Zotero is open, and can never
//! touch the real library.
//!
//! Three properties matter more than anything else here, because the output is
//! committed to git:
//!
//! 1. **The bibliography only ever grows.** Sync merges into the existing file
//!    and there is no mode that removes an entry - not for items deleted in
//!    Zotero, not for entries added to the `.bib` by hand. A reference that
//!    vanishes silently breaks every document citing it, and keeping a stale
//!    entry costs nothing by comparison.
//! 2. **Citation keys are pinned.** `citekeys.json` maps Zotero's stable item
//!    key to the citekey it was first assigned. A key never changes once
//!    handed out, so a `\cite{}` written months ago keeps resolving even if
//!    the title or author is later edited in Zotero.
//! 3. **Output is deterministic.** Entries are sorted by citekey, fields are
//!    emitted in a fixed order, and nothing time-varying is written into the
//!    file. Re-running against an unchanged library reproduces it byte for
//!    byte, so `git diff` shows real bibliography changes and nothing else.
//!
//! Note that deduplication (on by default) collapses items whose bibliographic
//! content is byte-identical *before* entries are generated. That does not
//! violate (1): no reference is lost, only redundant copies of the same one.
//! Every duplicate's Zotero key is pinned to the surviving citekey, so merging
//! those duplicates inside Zotero later does not change the `.bib`.

use crate::commands::run::detect_repo_root;
use crate::config::{expand_tilde, LoadedConfig};
use crate::{BibArgs, BibSubcommand};
use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(args: BibArgs, loaded: &LoadedConfig) -> Result<()> {
    match args.command {
        Some(BibSubcommand::Sync {
            data_dir,
            out,
            no_dedupe,
            dry_run,
        }) => sync(data_dir, out, no_dedupe, dry_run, loaded),
        Some(BibSubcommand::Status { data_dir }) => status(data_dir, loaded),
        // Bare `vn bib` reports rather than writes - the safe default for a
        // command whose other mode edits a tracked file.
        None => status(None, loaded),
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Zotero's default data directory is `~/Zotero` on Windows, Linux and macOS
/// alike, so one rule covers every host. `--data-dir` wins over the
/// `[zotero] data_dir` config key, which wins over the default.
fn resolve_data_dir(override_dir: Option<PathBuf>, loaded: &LoadedConfig) -> Result<PathBuf> {
    let dir = if let Some(dir) = override_dir {
        dir
    } else if let Some(configured) = loaded.config.zotero.data_dir.as_deref() {
        expand_tilde(configured)
    } else {
        dirs::home_dir()
            .context("could not determine home directory to locate the Zotero data dir")?
            .join("Zotero")
    };

    if !dir.join("zotero.sqlite").exists() {
        bail!(
            "no zotero.sqlite in {}. Pass --data-dir, or set [zotero] data_dir in {}",
            dir.display(),
            loaded.path.display()
        );
    }

    Ok(dir)
}

fn resolve_out_path(override_out: Option<PathBuf>, loaded: &LoadedConfig) -> Result<PathBuf> {
    if let Some(out) = override_out {
        return Ok(out);
    }
    if let Some(configured) = loaded.config.zotero.bib_path.as_deref() {
        return Ok(expand_tilde(configured));
    }
    Ok(detect_repo_root(loaded)?
        .join("zotero")
        .join("references.bib"))
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// A throwaway copy of the Zotero database. Removed on drop, including when
/// an error unwinds out of the read - the copy contains the user's whole
/// personal library and has no business outliving the command.
struct Snapshot {
    dir: PathBuf,
}

impl Snapshot {
    fn take(data_dir: &Path) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("vn-bib-{}", std::process::id()));
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create snapshot dir: {}", dir.display()))?;
        let snap = Snapshot { dir };

        let main = data_dir.join("zotero.sqlite");
        fs::copy(&main, snap.db_path())
            .with_context(|| format!("failed to copy {}", main.display()))?;

        // -wal / -shm only exist while Zotero is running. Copy them when
        // present so the snapshot includes commits not yet checkpointed into
        // the main file; skip silently when it is closed.
        for suffix in ["-wal", "-shm"] {
            let src = data_dir.join(format!("zotero.sqlite{suffix}"));
            if src.exists() {
                let dst = snap.dir.join(format!("zotero.sqlite{suffix}"));
                fs::copy(&src, &dst)
                    .with_context(|| format!("failed to copy {}", src.display()))?;
            }
        }

        Ok(snap)
    }

    fn db_path(&self) -> PathBuf {
        self.dir.join("zotero.sqlite")
    }

    fn open(&self) -> Result<Connection> {
        Connection::open_with_flags(self.db_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .context("failed to open the Zotero database snapshot")
    }
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// Reading the library
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Creator {
    creator_type: String,
    first: String,
    last: String,
    /// Zotero's fieldMode: 1 means a single-field name (an institution, say),
    /// stored whole in `last` with no first/last split to honour.
    single_field: bool,
}

#[derive(Debug, Clone)]
struct Item {
    zotero_key: String,
    item_type: String,
    fields: BTreeMap<String, String>,
    creators: Vec<Creator>,
}

fn read_items(conn: &Connection) -> Result<Vec<Item>> {
    // Attachments, notes and annotations are items in Zotero's schema but not
    // bibliography entries. Trashed items live in deletedItems until the trash
    // is emptied, so they must be excluded explicitly.
    let mut stmt = conn.prepare(
        "SELECT i.itemID, i.key, it.typeName
         FROM items i
         JOIN itemTypes it ON it.itemTypeID = i.itemTypeID
         WHERE it.typeName NOT IN ('attachment', 'note', 'annotation')
           AND i.itemID NOT IN (SELECT itemID FROM deletedItems)",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut by_id: HashMap<i64, Item> = HashMap::new();
    for row in rows {
        let (id, key, item_type) = row?;
        by_id.insert(
            id,
            Item {
                zotero_key: key,
                item_type,
                fields: BTreeMap::new(),
                creators: Vec::new(),
            },
        );
    }

    let mut stmt = conn.prepare(
        "SELECT id.itemID, f.fieldName, idv.value
         FROM itemData id
         JOIN fields f ON f.fieldID = id.fieldID
         JOIN itemDataValues idv ON idv.valueID = id.valueID",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (id, field, value) = row?;
        if let Some(item) = by_id.get_mut(&id) {
            item.fields.insert(field, value);
        }
    }

    // orderIndex is the author order shown in Zotero; preserving it is the
    // difference between correct authorship and a scrambled citation.
    let mut stmt = conn.prepare(
        "SELECT ic.itemID, ct.creatorType, c.firstName, c.lastName, c.fieldMode
         FROM itemCreators ic
         JOIN creators c ON c.creatorID = ic.creatorID
         JOIN creatorTypes ct ON ct.creatorTypeID = ic.creatorTypeID
         ORDER BY ic.itemID, ic.orderIndex",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            row.get::<_, Option<i64>>(4)?.unwrap_or(0),
        ))
    })?;
    for row in rows {
        let (id, creator_type, first, last, field_mode) = row?;
        if let Some(item) = by_id.get_mut(&id) {
            item.creators.push(Creator {
                creator_type,
                first,
                last,
                single_field: field_mode == 1,
            });
        }
    }

    let mut items: Vec<Item> = by_id.into_values().collect();
    // Sort by Zotero key so citekey assignment (and its a/b disambiguation)
    // depends only on library content, never on SQLite row order.
    items.sort_by(|a, b| a.zotero_key.cmp(&b.zotero_key));
    Ok(items)
}

// ---------------------------------------------------------------------------
// Citation keys
// ---------------------------------------------------------------------------

const TITLE_STOPWORDS: &[&str] = &[
    "a", "an", "the", "on", "of", "in", "for", "and", "to", "at", "by", "from", "with", "is",
    "are", "how", "what", "why", "towards", "toward",
];

/// Fold the Latin-1 accents that actually turn up in author names down to
/// ASCII, so a citekey stays typeable. Anything else non-alphanumeric is
/// dropped by the caller.
fn fold_ascii(input: &str) -> String {
    input
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' | 'ō' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            'ý' | 'ÿ' => 'y',
            'š' => 's',
            'ž' => 'z',
            'ł' => 'l',
            other => other,
        })
        .collect()
}

fn slug(input: &str) -> String {
    fold_ascii(&input.to_lowercase())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

/// Zotero stores dates multipart, e.g. "2015-06-11 2015-06-11" or
/// "2020-00-00 2020" - the leading four digits are the year when there is one.
fn year_of(item: &Item) -> Option<String> {
    let date = item.fields.get("date")?;
    let year: String = date.chars().take(4).collect();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        Some(year)
    } else {
        None
    }
}

fn primary_author(item: &Item) -> Option<&Creator> {
    item.creators
        .iter()
        .find(|c| c.creator_type == "author")
        .or_else(|| item.creators.first())
}

fn base_citekey(item: &Item) -> String {
    let author = primary_author(item)
        .map(|c| slug(&c.last))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "anon".to_string());

    let year = year_of(item).unwrap_or_else(|| "0000".to_string());

    let word = item
        .fields
        .get("title")
        .map(|t| {
            t.split_whitespace()
                .map(slug)
                .find(|w| !w.is_empty() && !TITLE_STOPWORDS.contains(&w.as_str()))
                .unwrap_or_default()
        })
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| "untitled".to_string());

    format!("{author}{year}{word}")
}

/// A content fingerprint used to collapse duplicate library items: the entry
/// this item would render as, with the citekey blanked out.
///
/// Comparing *rendered* output rather than raw Zotero fields is the point. Two
/// items that produce the same BibTeX entry are the same reference no matter
/// how they differ in fields the `.bib` never emits - `accessDate`,
/// `libraryCatalog`, `shortTitle` and friends vary freely between two imports
/// of one paper, and comparing them left visibly identical entries in the file
/// under different citekeys.
fn signature(item: &Item) -> String {
    render_entry(item, "")
}

/// Group items that describe the same record. Each group is sorted by Zotero
/// key and the groups themselves are ordered by their first key, so the
/// survivor of a duplicate set never depends on row order.
fn group_duplicates(items: Vec<Item>) -> Vec<Vec<Item>> {
    let mut by_signature: BTreeMap<String, Vec<Item>> = BTreeMap::new();
    for item in items {
        by_signature.entry(signature(&item)).or_default().push(item);
    }
    let mut groups: Vec<Vec<Item>> = by_signature.into_values().collect();
    for group in &mut groups {
        group.sort_by(|a, b| a.zotero_key.cmp(&b.zotero_key));
    }
    groups.sort_by(|a, b| a[0].zotero_key.cmp(&b[0].zotero_key));
    groups
}

/// Assign one citekey per group, reusing any key already pinned for *any*
/// member. That last part is what keeps keys stable when duplicates are later
/// merged inside Zotero: the merge keeps one arbitrary item key, and every key
/// in the group already points at the same citekey. New keys are disambiguated
/// with a/b/c against everything taken, so adding an item never renames an
/// existing entry.
fn assign_citekeys(groups: &[Vec<Item>], pinned: &mut BTreeMap<String, String>) -> Vec<String> {
    let mut taken: BTreeSet<String> = pinned.values().cloned().collect();
    let mut keys = Vec::with_capacity(groups.len());

    for group in groups {
        let item = &group[0];

        if let Some(existing) = group
            .iter()
            .find_map(|i| pinned.get(&i.zotero_key))
            .cloned()
        {
            for member in group {
                pinned.insert(member.zotero_key.clone(), existing.clone());
            }
            keys.push(existing);
            continue;
        }

        let base = base_citekey(item);
        let mut candidate = base.clone();
        let mut suffix = b'a';
        while taken.contains(&candidate) {
            candidate = format!("{base}{}", suffix as char);
            if suffix == b'z' {
                // Beyond 26 collisions fall back to the Zotero key, which is
                // unique by construction.
                candidate = format!("{base}{}", item.zotero_key.to_lowercase());
                break;
            }
            suffix += 1;
        }

        taken.insert(candidate.clone());
        for member in group {
            pinned.insert(member.zotero_key.clone(), candidate.clone());
        }
        keys.push(candidate);
    }

    keys
}

// ---------------------------------------------------------------------------
// BibTeX rendering
// ---------------------------------------------------------------------------

fn entry_type(item_type: &str, item: &Item) -> &'static str {
    match item_type {
        "journalArticle" => "article",
        "conferencePaper" => "inproceedings",
        "book" => "book",
        "bookSection" => "incollection",
        "thesis" => match item.fields.get("thesisType").map(|s| s.to_lowercase()) {
            Some(t) if t.contains("master") => "mastersthesis",
            _ => "phdthesis",
        },
        "report" => "techreport",
        "manuscript" | "presentation" => "unpublished",
        _ => "misc",
    }
}

/// Escape the characters BibTeX treats as markup. Skipped for verbatim-ish
/// fields (doi, url, isbn, issn) where the punctuation is part of the value
/// and braces already protect it.
fn escape_bibtex(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '&' => out.push_str("\\&"),
            '%' => out.push_str("\\%"),
            '$' => out.push_str("\\$"),
            '#' => out.push_str("\\#"),
            '_' => out.push_str("\\_"),
            '~' => out.push_str("\\textasciitilde{}"),
            '^' => out.push_str("\\textasciicircum{}"),
            other => out.push(other),
        }
    }
    out
}

fn normalize_ws(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Zotero writes page ranges with a hyphen or an en dash; BibTeX wants `--`.
fn normalize_pages(input: &str) -> String {
    let normalized = input.replace(['\u{2013}', '\u{2014}'], "-");
    let parts: Vec<&str> = normalized.splitn(2, '-').collect();
    if parts.len() == 2 && !parts[1].trim().is_empty() {
        format!("{}--{}", parts[0].trim(), parts[1].trim())
    } else {
        normalized.trim().to_string()
    }
}

fn format_names(creators: &[Creator], want: &str) -> Option<String> {
    let names: Vec<String> = creators
        .iter()
        .filter(|c| c.creator_type == want)
        .map(|c| {
            if c.single_field || c.first.is_empty() {
                // A single-field name is one unit; the extra braces stop
                // BibTeX splitting "MIT Press" into a first and last name.
                format!("{{{}}}", escape_bibtex(&c.last))
            } else {
                format!("{}, {}", escape_bibtex(&c.last), escape_bibtex(&c.first))
            }
        })
        .collect();

    if names.is_empty() {
        None
    } else {
        Some(names.join(" and "))
    }
}

/// The emitted field order. Fixed rather than alphabetical so entries read
/// the way a bibliography does, and so the output stays stable.
const FIELD_ORDER: &[&str] = &[
    "author",
    "editor",
    "title",
    "booktitle",
    "journal",
    "year",
    "volume",
    "number",
    "pages",
    "publisher",
    "address",
    "school",
    "institution",
    "series",
    "edition",
    "type",
    "doi",
    "isbn",
    "issn",
    "url",
    "language",
    "abstract",
    "note",
];

const VERBATIM_FIELDS: &[&str] = &["doi", "url", "isbn", "issn"];

/// Fields whose capitalisation must survive the bibliography style. Titles in
/// this field are full of acronyms and product names (GAN, StyleGAN3, AI);
/// without the extra brace layer many styles lowercase them.
const CASE_PROTECTED: &[&str] = &["title", "booktitle", "journal", "series"];

fn render_entry(item: &Item, citekey: &str) -> String {
    let mut fields: BTreeMap<&str, String> = BTreeMap::new();

    if let Some(authors) = format_names(&item.creators, "author") {
        fields.insert("author", authors);
    }
    if let Some(editors) = format_names(&item.creators, "editor") {
        fields.insert("editor", editors);
    }
    if let Some(year) = year_of(item) {
        fields.insert("year", year);
    }

    for (zotero_field, bibtex_field) in [
        ("title", "title"),
        ("publicationTitle", "journal"),
        ("proceedingsTitle", "booktitle"),
        ("bookTitle", "booktitle"),
        ("publisher", "publisher"),
        ("place", "address"),
        ("volume", "volume"),
        ("issue", "number"),
        ("series", "series"),
        ("edition", "edition"),
        ("university", "school"),
        ("institution", "institution"),
        ("thesisType", "type"),
        ("DOI", "doi"),
        ("ISBN", "isbn"),
        ("ISSN", "issn"),
        ("url", "url"),
        ("language", "language"),
        ("abstractNote", "abstract"),
        ("extra", "note"),
    ] {
        if let Some(raw) = item.fields.get(zotero_field) {
            let value = normalize_ws(raw);
            if value.is_empty() {
                continue;
            }
            // First mapping wins: proceedingsTitle and bookTitle both land on
            // booktitle, and a conference paper carries the former.
            fields.entry(bibtex_field).or_insert_with(|| {
                if VERBATIM_FIELDS.contains(&bibtex_field) {
                    value
                } else {
                    escape_bibtex(&value)
                }
            });
        }
    }

    if let Some(pages) = item.fields.get("pages") {
        let value = normalize_pages(&normalize_ws(pages));
        if !value.is_empty() {
            fields.insert("pages", value);
        }
    }

    let width = FIELD_ORDER
        .iter()
        .filter(|f| fields.contains_key(**f))
        .map(|f| f.len())
        .max()
        .unwrap_or(0);

    let mut out = format!("@{}{{{},\n", entry_type(&item.item_type, item), citekey);
    for field in FIELD_ORDER {
        let Some(value) = fields.get(field) else {
            continue;
        };
        let rendered = if CASE_PROTECTED.contains(field) {
            format!("{{{{{value}}}}}")
        } else {
            format!("{{{value}}}")
        };
        out.push_str(&format!(
            "  {:width$} = {},\n",
            field,
            rendered,
            width = width
        ));
    }
    out.push_str("}\n");
    out
}

// ---------------------------------------------------------------------------
// Existing-file handling
// ---------------------------------------------------------------------------

/// Pull `(citekey, raw entry text)` out of a previously generated file. Only
/// ever run over our own output, whose shape is known: entries start with `@`
/// in column zero and end at the matching brace.
fn parse_existing_entries(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut depth = 0usize;

    for line in text.lines() {
        if current.is_none() {
            if !line.starts_with('@') {
                continue;
            }
            let key = line
                .split_once('{')
                .map(|(_, rest)| rest.trim_end_matches(',').trim().to_string())
                .unwrap_or_default();
            current = Some((key, String::new()));
        }

        if let Some((_, body)) = current.as_mut() {
            body.push_str(line);
            body.push('\n');
            depth += line.matches('{').count();
            depth = depth.saturating_sub(line.matches('}').count());
            if depth == 0 {
                let (key, body) = current.take().expect("entry in progress");
                if !key.is_empty() {
                    entries.push((key, body));
                }
            }
        }
    }

    entries
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn load_pinned(path: &Path) -> Result<BTreeMap<String, String>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read citekey map: {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse citekey map: {}", path.display()))
}

fn sync(
    data_dir: Option<PathBuf>,
    out: Option<PathBuf>,
    no_dedupe: bool,
    dry_run: bool,
    loaded: &LoadedConfig,
) -> Result<()> {
    let data_dir = resolve_data_dir(data_dir, loaded)?;
    let out_path = resolve_out_path(out, loaded)?;
    let citekeys_path = out_path.with_file_name("citekeys.json");

    println!("[INFO] Zotero data dir: {}", data_dir.display());

    let snapshot = Snapshot::take(&data_dir)?;
    let conn = snapshot.open()?;
    let items = read_items(&conn)?;
    drop(conn);
    drop(snapshot);

    let item_count = items.len();

    // The library carries a lot of double-imported records. Collapsing them is
    // the default because a .bib where 40% of entries are byte-identical
    // duplicates is not usable - BibTeX warns on every one, and there is no
    // way to tell which of two identical keys to cite.
    let groups = if no_dedupe {
        items.into_iter().map(|i| vec![i]).collect()
    } else {
        group_duplicates(items)
    };
    let collapsed = item_count.saturating_sub(groups.len());

    let mut pinned = load_pinned(&citekeys_path)?;
    let pinned_before = pinned.len();
    let citekeys = assign_citekeys(&groups, &mut pinned);
    let new_keys = pinned.len().saturating_sub(pinned_before);

    let current: BTreeMap<String, String> = groups
        .iter()
        .zip(citekeys.iter())
        .map(|(group, key)| (key.clone(), render_entry(&group[0], key)))
        .collect();

    // Entries already in the file with no matching item in Zotero: a reference
    // removed there, or one added to the .bib by hand. They are ALWAYS kept.
    // The bibliography only ever grows - `vn bib sync` merges into it and has
    // no mode that drops an entry, deliberately: a citation that disappears
    // silently breaks every document that used it, and the reference is worth
    // more than the tidiness.
    let previous_text = fs::read_to_string(&out_path).unwrap_or_default();
    let previous = parse_existing_entries(&previous_text);
    let orphans: Vec<(String, String)> = previous
        .into_iter()
        .filter(|(key, _)| !current.contains_key(key))
        .collect();

    let mut all: BTreeMap<String, String> = current.clone();
    for (key, body) in &orphans {
        all.insert(key.clone(), body.clone());
    }

    let mut rendered = String::new();
    rendered.push_str(
        "% vncli bibliography - generated by `vn bib sync` from the local Zotero library.\n",
    );
    rendered
        .push_str("% Do not edit by hand; it is regenerated on every sync. Citation keys are\n");
    rendered.push_str("% pinned in citekeys.json and stay stable once assigned.\n");
    rendered.push_str(&format!("% {} entries.\n\n", all.len()));
    for (i, body) in all.values().enumerate() {
        if i > 0 {
            rendered.push('\n');
        }
        rendered.push_str(body);
    }

    let unchanged = previous_text == rendered;

    println!("[INFO] {item_count} items read from Zotero.");
    if collapsed > 0 {
        println!(
            "[INFO] {collapsed} duplicate item(s) collapsed into {} distinct entries.",
            groups.len()
        );
    }
    println!("[INFO] {new_keys} new citation key(s) assigned; {pinned_before} already pinned.");
    if !orphans.is_empty() {
        println!(
            "[INFO] {} entry/entries not currently in Zotero (kept - sync never removes).",
            orphans.len()
        );
    }

    if dry_run {
        println!("[INFO] Dry run - nothing written.");
        println!(
            "[INFO] Would write {} entries to {}.",
            all.len(),
            out_path.display()
        );
        return Ok(());
    }

    if unchanged {
        println!("[INFO] {} is already up to date.", out_path.display());
        return Ok(());
    }

    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output dir: {}", parent.display()))?;
    }
    fs::write(&out_path, &rendered)
        .with_context(|| format!("failed to write: {}", out_path.display()))?;

    let json = serde_json::to_string_pretty(&pinned).context("failed to serialize citekey map")?;
    fs::write(&citekeys_path, format!("{json}\n"))
        .with_context(|| format!("failed to write: {}", citekeys_path.display()))?;

    println!(
        "[INFO] Wrote {} entries to {}.",
        all.len(),
        out_path.display()
    );
    println!("[INFO] Wrote citation keys to {}.", citekeys_path.display());
    Ok(())
}

fn status(data_dir: Option<PathBuf>, loaded: &LoadedConfig) -> Result<()> {
    let data_dir = resolve_data_dir(data_dir, loaded)?;
    let out_path = resolve_out_path(None, loaded)?;
    let citekeys_path = out_path.with_file_name("citekeys.json");

    let snapshot = Snapshot::take(&data_dir)?;
    let conn = snapshot.open()?;
    let items = read_items(&conn)?;
    drop(conn);
    drop(snapshot);

    let pinned = load_pinned(&citekeys_path)?;
    let unpinned = items
        .iter()
        .filter(|i| !pinned.contains_key(&i.zotero_key))
        .count();

    let existing = fs::read_to_string(&out_path).unwrap_or_default();
    let in_file = parse_existing_entries(&existing).len();

    let item_count = items.len();
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    for item in &items {
        *by_type.entry(item.item_type.clone()).or_default() += 1;
    }
    let groups = group_duplicates(items);
    let duplicate_groups = groups.iter().filter(|g| g.len() > 1).count();

    println!("Zotero data dir : {}", data_dir.display());
    println!("Bibliography    : {}", out_path.display());
    println!("Items in Zotero : {item_count}");
    println!("Distinct records: {}", groups.len());
    println!(
        "Duplicates      : {} item(s) across {duplicate_groups} group(s)",
        item_count.saturating_sub(groups.len())
    );
    println!("Entries in file : {in_file}");
    println!("Pinned citekeys : {}", pinned.len());
    println!("Not yet synced  : {unpinned}");
    println!("\nBy item type:");
    for (item_type, count) in by_type {
        println!("  {item_type:<20} {count}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(item_type: &str, fields: &[(&str, &str)], creators: Vec<Creator>) -> Item {
        Item {
            zotero_key: "ABCD1234".to_string(),
            item_type: item_type.to_string(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            creators,
        }
    }

    fn author(first: &str, last: &str) -> Creator {
        Creator {
            creator_type: "author".to_string(),
            first: first.to_string(),
            last: last.to_string(),
            single_field: false,
        }
    }

    #[test]
    fn year_reads_zotero_multipart_dates() {
        let i = item(
            "journalArticle",
            &[("date", "2015-06-11 2015-06-11")],
            vec![],
        );
        assert_eq!(year_of(&i).as_deref(), Some("2015"));
        let i = item("journalArticle", &[("date", "2020-00-00 2020")], vec![]);
        assert_eq!(year_of(&i).as_deref(), Some("2020"));
        let i = item("journalArticle", &[("date", "n.d.")], vec![]);
        assert_eq!(year_of(&i), None);
    }

    #[test]
    fn citekey_skips_stopwords_and_folds_accents() {
        let i = item(
            "journalArticle",
            &[
                ("date", "2019-00-00 2019"),
                ("title", "The Rise of Machines"),
            ],
            vec![author("Jean", "Lecún")],
        );
        assert_eq!(base_citekey(&i), "lecun2019rise");
    }

    #[test]
    fn citekey_falls_back_when_fields_are_missing() {
        let i = item("document", &[], vec![]);
        assert_eq!(base_citekey(&i), "anon0000untitled");
    }

    #[test]
    fn pinned_keys_are_never_reassigned() {
        let mut pinned = BTreeMap::new();
        pinned.insert("ABCD1234".to_string(), "oldkey1999paper".to_string());
        let groups = vec![vec![item(
            "journalArticle",
            &[("date", "2019-00-00 2019"), ("title", "New Title")],
            vec![author("Jean", "Lecun")],
        )]];
        let keys = assign_citekeys(&groups, &mut pinned);
        assert_eq!(keys, vec!["oldkey1999paper".to_string()]);
    }

    #[test]
    fn colliding_keys_get_a_suffix() {
        let mut pinned = BTreeMap::new();
        let mut a = item(
            "journalArticle",
            &[("date", "2019-00-00 2019"), ("title", "Vision")],
            vec![author("A", "Smith")],
        );
        a.zotero_key = "AAAA1111".to_string();
        // Same base citekey, different record - so these must not be deduped
        // into one entry, they must get distinct keys.
        let mut b = item(
            "journalArticle",
            &[("date", "2019-00-00 2019"), ("title", "Vision Systems")],
            vec![author("A", "Smith")],
        );
        b.zotero_key = "BBBB2222".to_string();
        let groups = group_duplicates(vec![a, b]);
        assert_eq!(groups.len(), 2);
        let keys = assign_citekeys(&groups, &mut pinned);
        assert_eq!(keys, vec!["smith2019vision", "smith2019visiona"]);
    }

    #[test]
    fn identical_records_collapse_to_one_group() {
        let mut a = item(
            "journalArticle",
            &[("date", "2019-00-00 2019"), ("title", "Vision")],
            vec![author("A", "Smith")],
        );
        a.zotero_key = "BBBB2222".to_string();
        let mut b = a.clone();
        b.zotero_key = "AAAA1111".to_string();
        let groups = group_duplicates(vec![a, b]);
        assert_eq!(groups.len(), 1);
        // Survivor is the lowest Zotero key, not the order they were read in.
        assert_eq!(groups[0][0].zotero_key, "AAAA1111");
    }

    #[test]
    fn every_duplicate_pins_to_the_same_citekey() {
        // The property that keeps citekeys stable when duplicates are later
        // merged inside Zotero: whichever item key survives the merge is
        // already mapped to the same citekey.
        let mut a = item(
            "journalArticle",
            &[("date", "2019-00-00 2019"), ("title", "Vision")],
            vec![author("A", "Smith")],
        );
        a.zotero_key = "AAAA1111".to_string();
        let mut b = a.clone();
        b.zotero_key = "BBBB2222".to_string();

        let mut pinned = BTreeMap::new();
        let groups = group_duplicates(vec![a, b]);
        assign_citekeys(&groups, &mut pinned);

        assert_eq!(pinned.get("AAAA1111"), Some(&"smith2019vision".to_string()));
        assert_eq!(pinned.get("BBBB2222"), Some(&"smith2019vision".to_string()));
    }

    #[test]
    fn fields_the_bib_never_emits_do_not_block_dedup() {
        // accessDate/libraryCatalog differ freely between two imports of the
        // same paper and are not rendered, so they must not keep two identical
        // entries apart.
        let mut a = item(
            "journalArticle",
            &[
                ("date", "2016-00-00 2016"),
                ("title", "Deep Networks"),
                ("accessDate", "2020-01-01"),
            ],
            vec![author("A", "Smith")],
        );
        a.zotero_key = "AAAA1111".to_string();
        let mut b = item(
            "journalArticle",
            &[
                ("date", "2016-00-00 2016"),
                ("title", "Deep Networks"),
                ("libraryCatalog", "Some Catalog"),
            ],
            vec![author("A", "Smith")],
        );
        b.zotero_key = "BBBB2222".to_string();
        assert_eq!(group_duplicates(vec![a, b]).len(), 1);
    }

    #[test]
    fn differing_fields_are_not_deduped() {
        let mut a = item(
            "journalArticle",
            &[
                ("date", "2019-00-00 2019"),
                ("title", "Vision"),
                ("pages", "1-9"),
            ],
            vec![author("A", "Smith")],
        );
        a.zotero_key = "AAAA1111".to_string();
        let mut b = item(
            "journalArticle",
            &[
                ("date", "2019-00-00 2019"),
                ("title", "Vision"),
                ("pages", "10-19"),
            ],
            vec![author("A", "Smith")],
        );
        b.zotero_key = "BBBB2222".to_string();
        assert_eq!(group_duplicates(vec![a, b]).len(), 2);
    }

    #[test]
    fn pages_normalize_to_bibtex_ranges() {
        assert_eq!(normalize_pages("12-34"), "12--34");
        assert_eq!(normalize_pages("12\u{2013}34"), "12--34");
        assert_eq!(normalize_pages("42"), "42");
    }

    #[test]
    fn special_characters_are_escaped() {
        assert_eq!(escape_bibtex("Cost & Effect_2"), "Cost \\& Effect\\_2");
        assert_eq!(escape_bibtex("100%"), "100\\%");
    }

    #[test]
    fn single_field_creators_stay_one_unit() {
        let creators = vec![Creator {
            creator_type: "author".to_string(),
            first: String::new(),
            last: "MIT Press".to_string(),
            single_field: true,
        }];
        assert_eq!(format_names(&creators, "author").unwrap(), "{MIT Press}");
    }

    #[test]
    fn conference_papers_prefer_proceedings_title() {
        let i = item(
            "conferencePaper",
            &[
                ("title", "A Paper"),
                ("proceedingsTitle", "Proc. Of Things"),
                ("bookTitle", "Ignored Book"),
            ],
            vec![author("A", "Smith")],
        );
        let rendered = render_entry(&i, "smith2020paper");
        assert!(rendered.contains("@inproceedings{smith2020paper,"));
        assert!(rendered.contains("Proc. Of Things"));
        assert!(!rendered.contains("Ignored Book"));
    }

    #[test]
    fn round_trips_its_own_output() {
        let i = item(
            "journalArticle",
            &[("title", "Deep Nets"), ("date", "2019-00-00 2019")],
            vec![author("A", "Smith")],
        );
        let rendered = render_entry(&i, "smith2019deep");
        let parsed = parse_existing_entries(&rendered);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].0, "smith2019deep");
        assert_eq!(parsed[0].1, rendered);
    }

    #[test]
    fn thesis_type_selects_the_entry_type() {
        let masters = item("thesis", &[("thesisType", "Master's thesis")], vec![]);
        assert_eq!(entry_type("thesis", &masters), "mastersthesis");
        let phd = item("thesis", &[("thesisType", "PhD dissertation")], vec![]);
        assert_eq!(entry_type("thesis", &phd), "phdthesis");
    }
}
