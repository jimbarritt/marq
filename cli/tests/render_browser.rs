//! Runs the rendered page in headless Chromium and reads back where the layout
//! script put each card. A browser is the only thing that can say where a mark
//! sits on the page, so this is the check that the cards line up with their
//! highlights. With no Chromium binary the test prints a skip message and
//! passes: the page logic is covered without a browser in `render.rs`.
//!
//! A binary is taken from `CHROME`, or the first match of
//! `/opt/pw-browsers/chromium-*/chrome-linux/chrome`.

use marq_comments::render::render_page;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn find_chrome() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("CHROME") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut found: Vec<PathBuf> = std::fs::read_dir("/opt/pw-browsers")
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("chromium-"))
        .map(|e| e.path().join("chrome-linux").join("chrome"))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

fn thread(md: &str, needle: &str, nth: usize, body: &str) -> Value {
    let byte = md.match_indices(needle).nth(nth).expect("needle").0;
    let start = md[..byte].chars().count();
    let end = start + needle.chars().count();
    json!({
        "annotation": {
            "id": format!("urn:uuid:{:08x}-0000-4000-8000-000000000000", start),
            "motivation": "commenting",
            "created": format!("2026-09-29T10:{:02}:00Z", start % 60),
            "creator": {"type": "Person", "name": "Jim"},
            "body": {"type": "TextualBody", "value": body, "format": "text/markdown"},
            "target": {"selector": [{"type": "TextQuoteSelector", "exact": needle}]}
        },
        "state": "open",
        "anchor": {"status": "anchored", "start": start, "end": end},
        "stateChanges": [],
        "replies": []
    })
}

fn document() -> (String, Vec<Value>) {
    let filler = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
                  tempor incididunt ut labore et dolore magna aliqua. ";
    let md = format!(
        "# Layout check\n\n\
         {filler}The first marked phrase sits here and {filler}a second phrase follows in the same block.\n\n\
         {filler}{filler}\n\n\
         | Name | Value |\n|------|-------|\n| one | alpha cell |\n| two | beta cell |\n\n\
         {filler}\n\n\
         ```\nlet answer = 42;\nlet other = 7;\n```\n\n\
         {filler}A last phrase near the end.\n"
    );
    let threads = vec![
        thread(
            &md,
            "first marked phrase",
            0,
            "One.\n\nWith **two** paragraphs and a longer body so that the card is tall.",
        ),
        thread(
            &md,
            "second phrase",
            0,
            "Two, level with a second mark in the same block.",
        ),
        thread(&md, "beta cell", 0, "In a table cell."),
        thread(&md, "answer", 0, "In a code block."),
        thread(&md, "last phrase", 0, "Near the end."),
    ];
    (md, threads)
}

/// Writes the page, runs Chromium on it at `width` and returns the dumped DOM.
fn dump_dom(chrome: &PathBuf, html: &str, width: u32) -> String {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("page.html");
    std::fs::write(&file, html).unwrap();
    let profile = dir.path().join("profile");
    let output = Command::new(chrome)
        .args([
            "--headless",
            "--no-sandbox",
            "--disable-gpu",
            "--disable-dev-shm-usage",
            "--virtual-time-budget=3000",
        ])
        .arg(format!("--window-size={width},1000"))
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg("--dump-dom")
        .arg(format!("file://{}", file.display()))
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .expect("run chromium");
    let dom = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(dom.contains("<html"), "no DOM dumped: {dom:?}");
    dom
}

/// The value of attribute `name` in an opening tag.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let from = tag.find(&key)? + key.len();
    Some(&tag[from..from + tag[from..].find('"')?])
}

/// The opening tag that starts at `at`.
fn tag_at(dom: &str, at: usize) -> &str {
    &dom[at..at + dom[at..].find('>').unwrap() + 1]
}

#[derive(Debug)]
struct Placed {
    id: String,
    top: i64,
    bottom: i64,
    mark: i64,
}

fn rail_of(dom: &str) -> &str {
    let from = dom.find("<aside class=\"rail\"").expect("rail");
    &dom[from..from + dom[from..].find("</aside>").expect("rail end")]
}

fn cards_in(region: &str) -> Vec<Placed> {
    region
        .match_indices("<article class=\"card")
        .map(|(at, _)| {
            let tag = tag_at(region, at);
            let num = |name: &str| -> i64 {
                attr(tag, name)
                    .unwrap_or_else(|| panic!("{name} missing on {tag}"))
                    .parse()
                    .unwrap()
            };
            Placed {
                id: attr(tag, "id").unwrap().to_string(),
                top: num("data-top"),
                bottom: num("data-bottom"),
                mark: num("data-mark"),
            }
        })
        .collect()
}

fn html_tag(dom: &str) -> &str {
    tag_at(dom, dom.find("<html").unwrap())
}

#[test]
fn cards_sit_level_with_their_marks_in_a_wide_window_and_return_in_a_narrow_one() {
    let Some(chrome) = find_chrome() else {
        eprintln!("SKIP: no Chromium found (set CHROME or install one under /opt/pw-browsers); the browser layout check did not run");
        return;
    };
    let (md, threads) = document();
    let html = render_page(&md, &threads);

    // Wide: the script moves every card into the rail.
    let dom = dump_dom(&chrome, &html, 1250);
    assert!(attr(html_tag(&dom), "class").is_some_and(|c| c.split(' ').any(|c| c == "rail-on")));
    let rail = rail_of(&dom);
    assert_eq!(rail.matches("data-placed=\"rail\"").count(), threads.len());
    let mut cards = cards_in(rail);
    assert_eq!(cards.len(), threads.len());
    assert_eq!(dom.matches("<article class=\"card").count(), threads.len());
    // Page order: by the first mark's position.
    cards.sort_by_key(|c| (c.mark, c.id.clone()));
    let mut previous_bottom: Option<i64> = None;
    let mut previous_top = i64::MIN;
    let mut level = 0;
    for card in &cards {
        assert!(card.top >= previous_top, "tops go down the page: {cards:?}");
        // Rounding of the two measurements allows one pixel.
        assert!(card.top >= card.mark - 1, "above its mark: {card:?}");
        if let Some(bottom) = previous_bottom {
            assert!(card.top >= bottom - 1, "overlaps the card above: {card:?}");
            if card.mark >= bottom + 8 {
                assert!(
                    (card.top - card.mark).abs() <= 1,
                    "has room, not level: {card:?}"
                );
                level += 1;
            }
        } else {
            assert!(
                (card.top - card.mark.max(0)).abs() <= 1,
                "first not level: {card:?}"
            );
            level += 1;
        }
        assert!(card.bottom > card.top);
        previous_top = card.top;
        previous_bottom = Some(card.bottom);
    }
    assert!(
        level >= 3,
        "only {level} cards level with their mark: {cards:?}"
    );
    assert!(
        cards
            .iter()
            .map(|c| c.mark)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            >= 4,
        "marks are on distinct lines"
    );

    // Narrow: the cards go back under their blocks and the rail class is gone.
    let dom = dump_dom(&chrome, &html, 700);
    assert!(!attr(html_tag(&dom), "class")
        .unwrap_or("")
        .contains("rail-on"));
    assert!(!rail_of(&dom).contains("<article"));
    assert!(!dom.contains("data-placed=\""));
    let rows: Vec<&str> = dom.split("<section class=\"blk").skip(1).collect();
    for n in 1..=threads.len() {
        let mark = format!("<mark data-thread=\"t-{n}\"");
        let card = format!("<article class=\"card\" id=\"t-{n}\"");
        let row = rows
            .iter()
            .find(|row| row.contains(&mark))
            .unwrap_or_else(|| panic!("no block has mark {n}"));
        let margin = row.find("<aside class=\"margin\">").expect("margin");
        assert!(
            row[margin..].contains(&card),
            "card {n} is not in the margin of its block"
        );
    }
}
