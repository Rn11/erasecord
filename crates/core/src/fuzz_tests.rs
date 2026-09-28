//! Randomized tests that throw broken, strange and hostile input at the
//! parts that read it: data packages, filters, search results, backups and
//! statistics. Nothing may panic; bad input must end in an error or be
//! ignored. The seeds are fixed, so a failure can be reproduced.

use std::io::Write;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use rand::rngs::StdRng;
use rand::{Rng, RngExt, SeedableRng};

use crate::filter::{Filter, Has};
use crate::insights::{Index, Scope};
use crate::models::{Message, SearchResponse, User};
use crate::package::Package;
use crate::snowflake::Snowflake;

const ME: Snowflake = Snowflake(111111111111111111);

/// Strings that have broken parsers before, and random ones.
fn nasty(rng: &mut StdRng) -> String {
    const PIECES: [&str; 24] = [
        "",
        " ",
        "\"",
        "'",
        ",",
        "\n",
        "\r\n",
        "\\",
        "\u{0}",
        "\u{feff}",
        "😀",
        "👩‍👩‍👧",
        "ﷺ",
        "\u{202e}",
        "<@123>",
        "<:x:1>",
        "https://x.y/z?a=b",
        "..",
        "../../etc",
        "9999999999999999999999",
        "-1",
        "null",
        "{",
        "[",
    ];
    let mut s = String::new();
    for _ in 0..rng.random_range(0..6) {
        if rng.random_bool(0.6) {
            s.push_str(PIECES[rng.random_range(0..PIECES.len())]);
        } else {
            for _ in 0..rng.random_range(0..12) {
                // Any Unicode scalar value.
                if let Some(c) = char::from_u32(rng.random_range(0..0x11_0000)) {
                    s.push(c);
                }
            }
        }
    }
    s
}

fn any_id(rng: &mut StdRng) -> u64 {
    match rng.random_range(0..4) {
        0 => 0,
        1 => u64::MAX,
        2 => rng.random_range(0..1u64 << 22),
        _ => rng.random(),
    }
}

fn message(rng: &mut StdRng) -> Message {
    let json = |rng: &mut StdRng| match rng.random_range(0..4) {
        0 => {
            serde_json::json!({ "url": nasty(rng), "content_type": nasty(rng), "filename": nasty(rng) })
        }
        1 => serde_json::json!({ "type": nasty(rng), "image": {}, "video": null }),
        2 => serde_json::json!(nasty(rng)),
        _ => serde_json::json!(null),
    };
    Message {
        id: Snowflake(any_id(rng)),
        channel_id: Snowflake(any_id(rng)),
        kind: rng.random(),
        content: nasty(rng),
        author: User {
            id: if rng.random_bool(0.8) {
                ME
            } else {
                Snowflake(any_id(rng))
            },
            username: nasty(rng),
            global_name: None,
            avatar: None,
        },
        pinned: rng.random(),
        attachments: (0..rng.random_range(0..3)).map(|_| json(rng)).collect(),
        embeds: (0..rng.random_range(0..3)).map(|_| json(rng)).collect(),
        sticker_items: vec![],
        hit: None,
    }
}

fn filter(rng: &mut StdRng) -> Filter {
    let time = |rng: &mut StdRng| {
        rng.random_bool(0.5).then(|| {
            let secs = match rng.random_range(0..3) {
                0 => rng.random_range(-100_000_000_000i64..100_000_000_000),
                1 => rng.random_range(1_420_070_400..2_000_000_000),
                _ => [i64::MIN / 1000, -1, 0, 253_402_300_799][rng.random_range(0..4)],
            };
            Utc.timestamp_opt(secs, 0).single().unwrap_or_default()
        })
    };
    let kinds = |rng: &mut StdRng| {
        Has::ALL
            .iter()
            .copied()
            .filter(|_| rng.random_bool(0.2))
            .collect::<Vec<_>>()
    };
    Filter {
        after: time(rng),
        before: time(rng),
        skip_pinned: rng.random(),
        content: rng.random_bool(0.5).then(|| nasty(rng)),
        pattern: rng.random_bool(0.5).then(|| {
            // Mostly valid-looking regular expressions, some broken or huge.
            match rng.random_range(0..4) {
                0 => nasty(rng),
                1 => "(a+)+$".repeat(rng.random_range(1..50)),
                2 => format!("[{}", nasty(rng)),
                _ => format!("\\b{}\\b", regex_escape(&nasty(rng))),
            }
        }),
        has: kinds(rng),
        without: kinds(rng),
    }
}

fn regex_escape(s: &str) -> String {
    s.chars()
        .map(|c| {
            if "\\.+*?()|[]{}^$#&-~".contains(c) {
                format!("\\{c}")
            } else {
                c.to_string()
            }
        })
        .collect()
}

#[test]
fn filters_never_panic() {
    let mut rng = StdRng::seed_from_u64(1);
    for _ in 0..3000 {
        let f = filter(&mut rng);
        let query = f.search_query(ME);
        if let (Some(min), Some(max)) = (query.min_id, query.max_id) {
            // Only a valid range may be searched for.
            if f.compile().is_ok() {
                assert!(min < max, "{f:?} gave {query:?}");
            }
        }
        let _ = query.params();
        for matcher in [f.compile(), f.compile_for_package()].into_iter().flatten() {
            for _ in 0..5 {
                let m = message(&mut rng);
                let _ = f.contains(m.id);
                let _ = matcher.matches(&m);
            }
        }
    }
}

#[test]
fn search_results_of_any_shape_are_read_safely() {
    let mut rng = StdRng::seed_from_u64(2);
    for _ in 0..2000 {
        let groups: Vec<serde_json::Value> = (0..rng.random_range(0..5))
            .map(|_| {
                let group: Vec<serde_json::Value> = (0..rng.random_range(0..4))
                    .map(|_| {
                        let mut m = serde_json::to_value(message(&mut rng)).unwrap();
                        if rng.random_bool(0.3) {
                            m["hit"] = serde_json::json!(rng.random::<bool>());
                        }
                        if rng.random_bool(0.1) {
                            m["id"] = serde_json::json!(nasty(&mut rng));
                        }
                        m
                    })
                    .collect();
                serde_json::json!(group)
            })
            .collect();
        let body = serde_json::json!({ "total_results": rng.random::<u64>(), "messages": groups });
        if let Ok(response) = serde_json::from_value::<SearchResponse>(body) {
            let hits = response.into_hits();
            assert!(hits.windows(2).all(|w| w[0].id > w[1].id));
        }
    }
}

#[test]
fn statistics_take_any_text() {
    let mut rng = StdRng::seed_from_u64(3);
    let mut builder = crate::scan::tests_support::builder();
    for _ in 0..5000 {
        crate::scan::tests_support::add(&mut builder, &message(&mut rng));
    }
    let stats = crate::scan::tests_support::snapshot(&builder);
    assert_eq!(stats.messages, 5000);
    assert!(stats.top_words.len() <= 24 && stats.top_emoji.len() <= 8);
}

/// The sample package, with each file's content changed at random.
fn mutated_package(rng: &mut StdRng, dir: &Path) -> std::path::PathBuf {
    let sample = crate::inspect::tests::sample_package(dir);
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&sample).unwrap()).unwrap();
    let path = dir.join(format!("mutated-{}.zip", rng.random::<u32>()));
    let mut out = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut name = file.name().to_owned();
        let mut content = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut content).unwrap();
        match rng.random_range(0..8) {
            0 => content.truncate(rng.random_range(0..=content.len())),
            1 => {
                for _ in 0..rng.random_range(1..10) {
                    if !content.is_empty() {
                        let at = rng.random_range(0..content.len());
                        content[at] = rng.random();
                    }
                }
            }
            2 => {
                let at = rng.random_range(0..=content.len());
                let junk = nasty(rng);
                content.splice(at..at, junk.bytes());
            }
            3 => content = nasty(rng).into_bytes(),
            4 => name = format!("{name}{}", nasty(rng).replace('\u{0}', "")),
            5 => content.clear(),
            _ => {}
        }
        if name.is_empty() {
            continue;
        }
        if out
            .start_file(name, zip::write::SimpleFileOptions::default())
            .is_ok()
        {
            out.write_all(&content).unwrap();
        }
    }
    out.finish().unwrap();
    path
}

/// Everything that reads a package, on one package.
fn read_everything(path: &Path, dir: &Path) {
    let _ = crate::inspect::inspect(path);
    let _ = crate::anonymize::anonymize(path, &dir.join("anon.zip"));
    let Ok(package) = Package::open(path) else {
        return;
    };
    let _ = package.check_owner(ME);
    let targets: Vec<_> = package.targets().into_iter().map(|t| t.target).collect();
    let mut rng = StdRng::seed_from_u64(path.as_os_str().len() as u64);
    let f = filter(&mut rng);
    let _ = crate::job::preview_package(&package, ME, &targets, &f);
    let _ = crate::scan::package_stats(&package, ME, &targets, &f);
    let index = Index::build(Arc::new(package), &Utc);
    let scope = Scope::default();
    let _ = index.info();
    let _ = index.overview(&scope);
    let _ = index.timeline(&scope);
    let _ = index.places(&scope);
    let _ = index.words(&scope);
    let _ = index.links(&scope);
    let _ = index.search(&scope, &nasty(&mut rng), 50);
}

#[test]
fn broken_data_packages_never_panic() {
    let mut rng = StdRng::seed_from_u64(4);
    let dir = tempfile::tempdir().unwrap();
    for round in 0..500 {
        let path = mutated_package(&mut rng, dir.path());
        let result = catch_unwind(AssertUnwindSafe(|| read_everything(&path, dir.path())));
        if result.is_err() {
            let kept = std::env::temp_dir().join(format!("erasecord-fuzz-{round}.zip"));
            let _ = std::fs::copy(&path, &kept);
            panic!(
                "reading a broken package panicked; it was kept as {}",
                kept.display()
            );
        }
        let _ = std::fs::remove_file(path);
    }
}

#[test]
fn random_files_are_not_packages_or_backups() {
    let mut rng = StdRng::seed_from_u64(5);
    let dir = tempfile::tempdir().unwrap();
    let passphrase = crate::vault::secret("correct horse battery staple");
    for i in 0..60 {
        let path = dir.path().join(format!("junk-{i}.age"));
        let len = rng.random_range(0..4096);
        let mut bytes = vec![0u8; len];
        rng.fill_bytes(&mut bytes);
        if i % 3 == 0 {
            // Starts like a real age file.
            let mut header = b"age-encryption.org/v1\n-> scrypt ".to_vec();
            header.extend(bytes);
            bytes = header;
        }
        std::fs::write(&path, &bytes).unwrap();
        assert!(Package::open(&path).is_err());
        assert!(crate::vault::open(&path, &passphrase, &dir.path().join("out")).is_err());
    }
}

#[test]
fn damaged_backups_are_refused_not_half_opened() {
    let dir = tempfile::tempdir().unwrap();
    let passphrase = crate::vault::secret("correct horse battery staple");
    let plain = dir.path().join("list.csv.age");
    {
        let file = std::fs::File::create(&plain).unwrap();
        let mut w = crate::vault::encrypt(&passphrase, file).unwrap();
        for i in 0..5000 {
            writeln!(w, "row {i},some text that fills more than one chunk").unwrap();
        }
        w.finish().unwrap();
    }
    let good = std::fs::read(&plain).unwrap();
    let mut rng = StdRng::seed_from_u64(6);
    // Each attempt derives the key with scrypt, which is slow on purpose.
    for i in 0..6 {
        let mut bytes = good.clone();
        if i % 2 == 0 {
            bytes.truncate(rng.random_range(0..bytes.len()));
        } else {
            let at = rng.random_range(100..bytes.len());
            bytes[at] ^= 1 << rng.random_range(0..8);
        }
        let path = dir.path().join(format!("bad-{i}.csv.age"));
        std::fs::write(&path, &bytes).unwrap();
        let out = dir.path().join(format!("out-{i}"));
        assert!(
            crate::vault::open(&path, &passphrase, &out).is_err(),
            "a damaged file opened"
        );
    }
    let out = dir.path().join("out-good");
    assert!(crate::vault::open(&plain, &passphrase, &out).is_ok());
}

/// `ERASECORD_BIG_PACKAGE=big.zip cargo test --release -p erasecord-core
/// counting_a_big_package -- --ignored --nocapture`
#[test]
#[ignore = "needs a big package"]
fn counting_a_big_package() {
    let Ok(path) = std::env::var("ERASECORD_BIG_PACKAGE") else {
        return;
    };
    let package = Package::open(Path::new(&path)).unwrap();
    let me = package.owner.expect("owner");
    let targets: Vec<_> = package.targets().into_iter().map(|t| t.target).collect();
    let filter = Filter::default();
    let started = std::time::Instant::now();
    let entries = crate::job::preview_package(&package, me, &targets, &filter).unwrap();
    let counted = started.elapsed();
    let stats = crate::scan::package_stats(&package, me, &targets, &filter).unwrap();
    println!(
        "{} messages: counted in {counted:?}, statistics in {:?}",
        entries.iter().filter_map(|e| e.count).sum::<u64>(),
        started.elapsed() - counted
    );
    assert_eq!(
        stats.messages,
        entries.iter().filter_map(|e| e.count).sum::<u64>()
    );
}
