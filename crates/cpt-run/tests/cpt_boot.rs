//! Boot the real CPT program disk (`disk/cpt.img`, decoded from `cpt.td0`) on the real-hardware
//! model, through `cpt-run --cpt-boot`.
//!
//! If the images have not been made (run `setup-disks.bat`, or `python tools/setup_disks.py`),
//! the tests that need them print a note and pass without running.

use std::process::Command;

fn image() -> Option<String> {
    let path = format!("{}/../../disk/cpt.img", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&path).exists() {
        Some(path)
    } else {
        eprintln!("skipping: disk/cpt.img not found; run setup-disks.bat (python tools/setup_disks.py)");
        None
    }
}

/// The second program disk, with TTY communications (`cpt2489.td0`).
fn tty_image() -> Option<String> {
    let path = format!("{}/../../disk/cpt2489.img", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&path).exists() {
        Some(path)
    } else {
        eprintln!("skipping: disk/cpt2489.img not found; run setup-disks.bat (python tools/setup_disks.py)");
        None
    }
}

fn boot(image: &str, cycles: &str, brk: &str, delay: &str) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_cpt-run"))
        .args(["--cpt-boot", image, cycles, brk, delay])
        .output()
        .expect("run cpt-run");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// With instant disk reads, the loader's timing check (65FFH) fails: it shows "E1" in the display
/// list and stops in its error loop. This pins down that the check is a timing check.
#[test]
fn instant_reads_fail_the_loaders_timing_check() {
    let Some(img) = image() else { return };
    let out = boot(&img, "20000000", "-", "0");
    assert!(out.contains("sectors read: 600"), "{}", out);
    assert!(out.contains("   1 |E1  "), "expected the E1 error on the first screen row:\n{}", out);
    assert!(out.contains("PC=60CD"), "parked in the loader's error loop:\n{}", out);
}

/// With reads that take time, the boot completes: the loader reads the system, the main system
/// runs with interrupts enabled, programs the 8253 (counts 16000 and 20000), and its handlers
/// acknowledge the timer interrupts on port 00H (RST 5 writes 20H, RST 7 writes 80H) at the
/// rates the counters produce.
#[test]
fn the_boot_reaches_the_main_system_and_services_the_timer() {
    let Some(img) = image() else { return };
    let out = boot(&img, "40000000", "-", "40000");
    assert!(out.contains("with nonzero status: 0"), "{}", out);
    assert!(out.contains("interrupts enabled"), "{}", out);
    assert!(!out.contains("PC=60CD"), "should not be in the loader's error loop:\n{}", out);
    assert!(out.contains("8253: counter reloads [0, 16000, 20000]"), "{}", out);
    let acks = out.lines().find(|l| l.starts_with("interrupt acknowledges")).expect("ack line");
    let count = |v: &str| -> u64 {
        acks.split_whitespace()
            .find_map(|t| t.strip_prefix(&format!("{}x", v)).and_then(|n| n.parse().ok()))
            .unwrap_or(0)
    };
    // About 4.6 s of main-system time: ~575 x 125 Hz and ~460 x 100 Hz.
    assert!(count("20") > 400, "RST 5 (125 Hz) acknowledges: {}", acks);
    assert!(count("80") > 300, "RST 7 (100 Hz) acknowledges: {}", acks);
    assert!(count("20") > count("80"), "125 Hz outpaces 100 Hz: {}", acks);
}

/// With the retrace interrupt (RST 6) running, the system draws its screen into the display
/// list: the typing-line ruler appears, with its margin and tab glyphs (87H, 80H dots, 81H every
/// fifth position, 82H every tenth, 83H at the end).
#[test]
fn the_system_draws_the_ruler_into_the_display_memory() {
    let Some(img) = image() else { return };
    let tmp = std::env::temp_dir().join(format!("cpt_boot_mem_{}.bin", std::process::id()));
    let out = Command::new(env!("CARGO_BIN_EXE_cpt-run"))
        .args(["--cpt-boot", &img, "40000000", "-", "40000"])
        .arg(&tmp)
        .output()
        .expect("run cpt-run");
    assert!(out.status.success());
    let mem = std::fs::read(&tmp).expect("memory snapshot");
    let _ = std::fs::remove_file(&tmp);
    let bank0 = &mem[..0x8000];
    let ruler = [0x87, 0x80, 0x80, 0x80, 0x81, 0x80, 0x80, 0x80, 0x80, 0x82];
    assert!(bank0.windows(ruler.len()).any(|w| w == ruler), "the ruler is in bank 0");
    // The display list starts at the top of bank 0 with a link (C2 lo hi).
    assert_eq!(bank0[0x7FF8], 0xC2, "display list head at 7FF8H");
}

/// Run `--cpt-boot` with extra named options (keys, snapshots).
fn boot_with(image: &str, cycles: &str, extra: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_cpt-run"))
        .args(["--cpt-boot", image, cycles, "-", "40000"])
        .args(extra)
        .output()
        .expect("run cpt-run");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The final screen's row starting with `prefix` (e.g. "  46 |"). Snapshots taken with
/// --screen-at come before it in the output, so this takes the last match.
fn last_row<'a>(out: &'a str, prefix: &str) -> &'a str {
    out.lines().filter(|l| l.starts_with(prefix)).last().unwrap_or_else(|| panic!("no row {:?}:\n{}", prefix, out))
}

/// Once the system has started (about 35 s in: the loader's 600 reads at 20 ms each, then the
/// system's own interrupt-driven reads), it settles on its editing screen. That screen has a
/// title line with the status (column 1, line 0) at its right end, 44 text rows, the typing
/// line (row 46), and the ruler with margins at columns 1 and 80 and the cursor marker at
/// column 1. The title line and ruler are white on black throughout. The screen ends at the B1
/// mark. Getting here needs the floppy interrupt (RST 3) and the drive Not Ready probe; without
/// them the system halts on error 1 at 15C1H.
#[test]
fn the_started_system_shows_its_editing_screen() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "80000000", &[]);
    assert!(out.contains("screen from the display list at 7FF8H (47 rows)"), "{}", out);
    let ruler = last_row(&out, "  47 |");
    assert!(ruler.starts_with("  47 |^...:....|....:....|"), "ruler, cursor at column 1: {}", ruler);
    assert!(ruler.ends_with("....:....M| white on black"), "right margin at column 80, reversed: {}", ruler);
    let title = last_row(&out, "   1 |");
    assert!(title.contains(" 1    0 "), "the status at the title's right end: {}", title);
    assert!(title.ends_with("| white on black"), "no black-on-white cells in the title line: {}", title);
    assert!(out.contains("ended at: end mark at"), "{}", out);
}

/// Keys typed through the real keyboard path: each key goes out as a make/break event on
/// 01H/02H, the RST 2 handler puts it in the ring at 058BH, the editor's task translates it with
/// the table at DEFFH (block 2), and the text appears on the typing line. RETURN (key 14H)
/// moves "Hello" up a line. The status (at the title's right end) then shows column 7, line 1,
/// and the ruler's cursor marker follows the column.
#[test]
fn typing_shows_text_on_the_typing_line() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "Hello{14}World!", "--keys-at", "80000000"]);
    let kb = out.lines().find(|l| l.starts_with("keyboard:")).expect("keyboard line");
    assert!(kb.contains("0 still waiting"), "{}", kb);
    assert!(last_row(&out, "  45 |").starts_with("  45 |Hello   "), "{}", out);
    assert!(last_row(&out, "  46 |").starts_with("  46 |World!   "), "{}", out);
    assert!(last_row(&out, "  47 |").starts_with("  47 |M...:.^..|"), "cursor marker at column 7:\n{}", out);
    let title = last_row(&out, "   1 |");
    assert!(title.contains(" 7    1 "), "status (column 7, line 1): {}", title);
    // The title line, the empty filler rows above the typed text (2-44) and the ruler are
    // white on black; the text and the typing line are not.
    assert!(out.contains("white on black (rows): 1-44, 47\n"), "{}", out);
    assert!(last_row(&out, "  47 |").ends_with("| white on black"), "the whole ruler is reversed");
    assert!(last_row(&out, "  46 |").ends_with('|'), "the typing line is black on white");
}

/// The unit keys name themselves on the status row, and IN/OUT/PRINT open a command prompt on
/// the top line that takes the typed document name (upper-cased).
#[test]
fn command_keys_show_their_prompts() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "Hello{2E}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").contains("WORD"), "{}", out);
    let out = boot_with(&img, "100000000", &["--keys", "Hello{2D}world", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |OUT        1 WORLD"), "{}", out);
}

/// IN, a document name and RETURN open a document from the disk through the system's own
/// filer. LEGAL is a print-wheel table: its first line goes on the typing line (row 46) and
/// the following lines below the ruler. The filer reads its four blocks at
/// track 48, physical sectors 0, 3, 6 and 9 (logical 0-3 with the skew of 3), and the screen
/// matches those sectors byte for byte.
#[test]
fn in_opens_a_document_from_the_disk() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "110000000", &["--keys", "{3D}legal{14}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |IN         1 LEGAL"), "{}", out);
    assert!(last_row(&out, "  46 |").starts_with("  46 |T.017-5  REV (G-1-1)    USA LEGAL           03/31/81  LEGAL"), "{}", out);
    assert!(last_row(&out, "  48 |").starts_with("  48 |1   20  22  23  24  25  26  229 8   9"), "{}", out);
    let reads = out.lines().find(|l| l.contains("after the keys started")).expect("read list");
    assert!(reads.contains("(4): t48s0(768) t48s3(771) t48s6(774) t48s9(777)"), "{}", reads);
}

/// UP (key 0FH) moves through an open document: the letter RC-L1 scrolls up past the
/// typing line, its own format line (margins at 1 and 60, white on black) goes above, and the
/// main ruler takes the document's margins.
#[test]
fn up_scrolls_through_a_letter() {
    let Some(img) = image() else { return };
    let keys = format!("{{3D}}rc-l1{{14}}{{ms 8000}}{}", "{0F}{ms 300}".repeat(14));
    let out = boot_with(&img, "150000000", &["--keys", &keys, "--keys-at", "80000000"]);
    assert!(last_row(&out, "  32 |").starts_with("  32 |Letter to Carl Wales"), "{}", out);
    assert!(last_row(&out, "  33 |").starts_with("  33 |M...:....|") && last_row(&out, "  33 |").ends_with("white on black"),
        "the letter's format line:\n{}", out);
    assert!(last_row(&out, "  45 |").starts_with("  45 |Dear Carl:"), "{}", out);
    assert!(last_row(&out, "  48 |").starts_with("  48 |Since July 13, 1987, you began calling in sick on the"), "{}", out);
    assert!(last_row(&out, "   1 |").contains(" 1   14 "), "line 14: {}", last_row(&out, "   1 |"));
    assert!(last_row(&out, "  47 |").starts_with("  47 |^...:....|....:....|....:....|....:....|....:....|....:....M"),
        "the ruler takes the letter's right margin (60): {}", last_row(&out, "  47 |"));
}

/// UP (key 0FH) and DOWN (key 1FH) move by the selected unit; with no unit, by a line. UP moves
/// the text up (on through the document), DOWN moves it down (back toward the start). In the
/// letter, five UPs and one DOWN leave line 4, and PAGE then UP goes to the end of the page
/// (line 79).
#[test]
fn up_and_down_move_by_the_selected_unit() {
    let Some(img) = image() else { return };
    let keys = format!("{{3D}}rc-l1{{14}}{{ms 8000}}{}{{1F}}{{ms 800}}", "{0F}{ms 300}".repeat(5));
    let out = boot_with(&img, "125000000", &["--keys", &keys, "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").contains(" 1    4 "), "DOWN: back to line 4: {}", last_row(&out, "   1 |"));
    let out = boot_with(&img, "130000000", &["--keys", "{3D}rc-l1{14}{ms 8000}{5F}{ms 300}{0F}", "--keys-at", "80000000"]);
    let title = last_row(&out, "   1 |");
    assert!(title.contains(" 1   79  PAGE "), "PAGE then UP: line 79: {}", title);
}

/// The user's check: CODE + IN, then RETURN, builds the table of contents and scrolls it into
/// the upper screen (the cursor at its end, line 33). PAGE then DOWN scrolls the upper screen
/// completely into the lower screen: back to line 0, and the TOC's start in the 7 rows below
/// the ruler. The screen is 54 rows (the system's layout: 45 rows through the typing line and
/// 7 below the ruler), even though the display list holds more of the TOC after them.
#[test]
fn page_down_scrolls_the_upper_screen_into_the_lower() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "120000000", &["--keys", "{+77}{3D}{-77}{14}{ms 10000}{5F}{ms 500}{1F}{ms 8000}",
        "--keys-at", "80000000", "--screen-at", "100000000"]);
    // Before PAGE, DOWN (the snapshot at 100M cycles): the TOC above the typing line.
    let start = out.find("=== at").expect("the snapshot");
    let end = out[start..].find("\nran ").map_or(out.len(), |j| start + j);
    let before = &out[start..end];
    assert!(before.contains("  16 |                            TABLE OF CONTENTS"), "TOC above:\n{}", before);
    assert!(before.contains(" 1   33 "), "cursor at the TOC's end (line 33):\n{}", before);
    // After: line 0, the TOC below the ruler, rows down to 80.
    let title = last_row(&out, "   1 |");
    assert!(title.starts_with("   1 |CONTENTS   1") && title.contains(" 1    0  PAGE "), "{}", title);
    assert!(last_row(&out, "  50 |").starts_with("  50 |                            TABLE OF CONTENTS"), "{}", out);
    assert!(last_row(&out, "  53 |").starts_with("  53 |D-PAGE 8100/8515/8525"), "{}", out);
    assert!(out.contains("screen from the display list at 7FF8H (54 rows)"), "the screen is 54 rows:\n{}", out);
    assert!(!out.contains("  55 |"), "no rows past 54:\n{}", out);
    // With the whole page below, the upper screen is empty white-on-black lines (rows 2-45);
    // the typing line (46) stays black on white.
    assert!(last_row(&out, "   2 |").starts_with("   2 | (44 empty rows, white on black)"), "{}", out);
    assert!(out.contains("white on black (rows): 1-45, 47\n"), "{}", out);
}

/// SKIP (key 2FH) discards text after the cursor by the selected unit. In the letter, PAGE then
/// SKIP sends the whole lower screen to the bit bucket: the display list then ends at the
/// ruler, leaving rows 48-54 empty (white). LINE then SKIP discards one line (the letter's
/// format line), and the rest moves up.
#[test]
fn skip_discards_the_lower_screen_by_unit() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "115000000", &["--keys", "{3D}rc-l1{14}{ms 8000}{5F}{ms 400}{2F}{ms 4000}", "--keys-at", "80000000"]);
    assert!(out.contains("screen from the display list at 7FF8H (47 rows)"), "nothing below the ruler:\n{}", out);
    assert!(!out.contains("August 19, 1987"), "the lower text is gone:\n{}", out);
    assert!(last_row(&out, "   1 |").contains(" 1    0  PAGE "), "{}", last_row(&out, "   1 |"));
    let out = boot_with(&img, "115000000", &["--keys", "{3D}rc-l1{14}{ms 8000}{7E}{ms 400}{2F}{ms 4000}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "  48 |").starts_with("  48 |August 19, 1987"), "LINE, SKIP: the format line is gone:\n{}", out);
}

/// A blank, formatted CPT data disk, as `tools/mkdatadisk.py` makes it: a 16-byte label, track
/// 0 reserved in the allocation map, and an empty directory leaf as the root.
fn blank_data_disk() -> Vec<u8> {
    let sec = |t: usize, s: usize| (t * 16 + s) * 256;
    let mut img = vec![0u8; 77 * 16 * 256];
    img[sec(0, 0)..sec(0, 0) + 16].copy_from_slice(&[0, 1, 1, 2, 0, 4, 0xD0, 0, 0, 1, 0, 0, 0x0F, 0, 0, 0]);
    img[sec(0, 3)..sec(0, 3) + 4].copy_from_slice(&[0x55; 4]);
    img[sec(0, 13)] = 0x30;
    img
}

/// The physical (linear) sector of a logical sector on a CPT disk: a skew of 3 within the
/// track, shifted by 2 per track (see `tools/cptfs.py`).
fn physical(logical: usize) -> usize {
    let (t, s) = (logical / 16, logical % 16);
    t * 16 + (3 * s + 2 * t) % 16
}

/// Every (name, first-block logical sector) in a CPT directory, in tree order. Nodes are a
/// level byte ('0' leaf, '1' inner) then 11 items of 16 name + 4 field + 3 pointer bytes.
fn directory(img: &[u8], logical: usize) -> Vec<(String, usize)> {
    let node = &img[physical(logical) * 256..][..256];
    let mut out = Vec::new();
    for k in 0..11 {
        let e = &node[1 + 23 * k..24 + 23 * k];
        if e.iter().all(|&b| b == 0) {
            continue;
        }
        let ptr = (e[20] as usize) << 16 | (e[21] as usize) << 8 | e[22] as usize;
        if node[0] == b'0' {
            out.push((String::from_utf8_lossy(&e[..16]).trim_end().to_string(), ptr));
        } else if ptr != 0 {
            out.extend(directory(img, ptr));
        }
    }
    out
}

/// Filing twelve pages to a blank data disk fills the root leaf (a slot to spare for the empty
/// first one, ten pages) and splits it: the root becomes an inner node over two leaves. Every
/// page is still listed, in order, and its pointer leads to its own `01 NAME 02` block. The
/// names go in shuffled so some land mid-leaf.
#[test]
fn a_data_disk_directory_splits_when_a_leaf_fills() {
    let Some(img) = image() else { return };
    let tag = std::process::id();
    let blank = std::env::temp_dir().join(format!("cpt-split-blank-{}.img", tag));
    let saved = std::env::temp_dir().join(format!("cpt-split-saved-{}.img", tag));
    std::fs::write(&blank, blank_data_disk()).unwrap();
    let order = ["p07", "p02", "p11", "p05", "p12", "p01", "p09", "p04", "p03", "p06", "p10", "p08"];
    let keys: String = order.iter()
        .map(|n| format!("{n}{{14}}{{ms 800}}{{2D}}{{ms 1500}}{{06}}{{06}}2{n}{{14}}{{ms 2500}}"))
        .collect();
    let out = boot_with(&img, "225000000", &["--disk2", blank.to_str().unwrap(), "--save-disk2", saved.to_str().unwrap(),
        "--keys", &keys, "--keys-at", "80000000"]);
    let bytes = std::fs::read(&saved).expect("the saved disk");
    assert_eq!(bytes[13 * 256], b'1', "the root became an inner node:\n{}", out);
    let dir = directory(&bytes, 15);
    let mut want: Vec<String> = order.iter().map(|n| n.to_uppercase()).collect();
    want.sort();
    let names: Vec<String> = dir.iter().map(|d| d.0.clone()).collect();
    assert_eq!(names, want, "all twelve pages, in order");
    for (name, ptr) in &dir {
        let blk = &bytes[physical(*ptr) * 256..];
        assert_eq!(blk[0], 0x01, "{} header", name);
        assert_eq!(&blk[1..1 + name.len()], name.as_bytes(), "{} points at its own block", name);
        assert_eq!(blk[1 + name.len()], 0x02, "{} header end", name);
    }
    let _ = std::fs::remove_file(&blank);
    let _ = std::fs::remove_file(&saved);
}

/// The blank data disk kept in the repository (for the web page) is exactly what the generator
/// makes, so the two can't drift apart. Needs no program disk.
#[test]
fn the_committed_blank_data_disk_matches_the_generator() {
    let path = format!("{}/../cpt-wasm/web/blank-data.img", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path, e));
    assert!(bytes == blank_data_disk(),
        "regenerate it with: python tools/mkdatadisk.py crates/cpt-wasm/web/blank-data.img");
}

/// A blank data disk in station 2 takes a page: OUT 2 ZZ changes exactly the allocation map
/// (t0s3), the directory root (t0s13) and the page's first block (t1s2). Rebooting with the
/// saved disk in station 2, IN 2 ZZ brings the page back. (Backspacing twice over the prompt's
/// station digit and typing 2 selects station 2.)
#[test]
fn a_blank_data_disk_takes_a_page_and_keeps_it() {
    let Some(img) = image() else { return };
    let tag = std::process::id();
    let blank = std::env::temp_dir().join(format!("cpt-blank-{}.img", tag));
    let saved = std::env::temp_dir().join(format!("cpt-saved-{}.img", tag));
    std::fs::write(&blank, blank_data_disk()).unwrap();

    let out = boot_with(&img, "105000000", &["--disk2", blank.to_str().unwrap(), "--save-disk2", saved.to_str().unwrap(),
        "--keys", "TEST{14}hello world{14}{ms 1000}{2D}{ms 1500}{06}{06}2zz{14}{ms 8000}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |OUT        2 ZZ"), "{}", last_row(&out, "   1 |"));
    let bytes = std::fs::read(&saved).expect("the saved disk");
    let before = blank_data_disk();
    let changed: Vec<usize> = (0..1232).filter(|&p| before[p * 256..(p + 1) * 256] != bytes[p * 256..(p + 1) * 256]).collect();
    assert_eq!(changed, vec![3, 13, 18], "t0s3, t0s13 and t1s2 (linear sectors)");
    assert_eq!(&bytes[18 * 256..18 * 256 + 4], &[0x01, b'Z', b'Z', 0x02], "the page's header block");
    assert_eq!(bytes[13 * 256], 0x30, "the root is still a leaf");
    assert_eq!(&bytes[13 * 256 + 24..13 * 256 + 26], b"ZZ", "with ZZ in its first used slot");

    let out = boot_with(&img, "105000000", &["--disk2", saved.to_str().unwrap(),
        "--keys", "{3D}{ms 1500}{06}{06}2zz{14}{ms 8000}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |IN         2 ZZ"), "{}", last_row(&out, "   1 |"));
    assert!(last_row(&out, "  46 |").starts_with("  46 |TEST"), "{}", out);
    assert!(last_row(&out, "  48 |").starts_with("  48 |hello world"), "{}", out);
    let _ = std::fs::remove_file(&blank);
    let _ = std::fs::remove_file(&saved);
}

/// `--watch` finds who touches a byte. Start-up stores the operation number at 0E71H: the
/// module loader's DMA puts the first (zero) value there, then the instruction at 0D9CH
/// writes it (STA 0E71H). With --watch-stop the run halts at that write.
#[test]
fn watch_finds_who_writes_the_operation_number() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "40000000", &["--watch", "w:0E71"]);
    assert!(out.contains("watch w:0E71: 2 accesses"), "{}", out);
    assert!(out.contains("65C3 write DMA x1"), "the loader's DMA: {}", out);
    assert!(out.contains("0D9C write     x1"), "the start-up code's store: {}", out);
    // Both a read and a write are seen when the range isn't limited; the read is at 0E23H.
    let out = boot_with(&img, "40000000", &["--watch", "0E71"]);
    assert!(out.contains("0E23 read      x1"), "{}", out);
    // Stop at the first write after the DMA load.
    let out = boot_with(&img, "40000000", &["--watch", "w:0E71", "--watch-from", "25000000", "--watch-stop"]);
    assert!(out.contains("watch: write of 0E71 (value 00) by the instruction at 0D9C"), "{}", out);
    assert!(out.contains("stack at "), "{}", out);
}

/// `--brk-read` stops as the read of a given sector is issued. IN RC-L1 reads the letter's
/// first block, logical 675 (track 42, physical sector 13 with the skew), into 2800H; the
/// three ways of naming the sector stop at the same place.
#[test]
fn brk_read_stops_when_a_sector_read_is_issued() {
    let Some(img) = image() else { return };
    for sector in ["L675", "t42s13", "685"] {
        let out = boot_with(&img, "120000000", &["--keys", "{3D}rc-l1{14}", "--keys-at", "80000000",
            "--brk-after", "80000000", "--brk-read", sector]);
        assert!(out.contains("read of sector t42s13 (linear 685) issued, DMA to 2800 (256 bytes) at cycle"),
            "{}: {}", sector, out.lines().find(|l| l.starts_with("read of") || l.starts_with("ran ")).unwrap_or(""));
        assert!(out.contains("stack at "), "{}", out);
        assert!(!out.contains("Letter to Carl Wales"), "stopped before the letter was shown");
    }
}

/// The staple key (05H, under WORDPAK) takes the typing line as a document name and brings
/// that document in: "wn" typed over the start of "P. O. Box 7004" becomes WN's text, read
/// from WN's sector (t34s15).
#[test]
fn staple_brings_in_the_document_named_on_the_typing_line() {
    let Some(img) = image() else { return };
    let keys = format!("{{3D}}rc-l1{{14}}{{ms 8000}}{}wn{{ms 500}}{{05}}{{ms 10000}}", "{0F}{ms 300}".repeat(8));
    let out = boot_with(&img, "190000000", &["--keys", &keys, "--keys-at", "80000000"]);
    assert!(last_row(&out, "  46 |").starts_with("  46 |WORDNET.  RATE: 9600"), "{}", out);
    assert!(!out.contains("P. O. Box 7004"), "the name line is replaced:\n{}", out);
    let reads = out.lines().find(|l| l.contains("after the keys started")).expect("read list");
    assert!(reads.contains("t34s15(559)"), "{}", reads);
}

/// TTY communications on the TTY disk: CODE + I, then the control page `CTTY`, enables them.
/// The system sets up the 8251 at 4AH (reset 00 00 00 40, mode 4EH, command 37H) and leaves
/// the transmitter on with nothing to send. Characters from the line interrupt on RST 1
/// (acknowledged with 02H), and with the default reception (Screen) they appear on the screen
/// with the parity bit stripped. (Before the transmit request was held off until 10H on port
/// 00H, a stream of transmit interrupts let timer ticks nest until the stack ran through the
/// comm control block at 2D00H: the start-up "sent" stack garbage (`04`, `04 4E`) and the
/// reception table pointer was overwritten, so received text showed as wrong glyphs.)
#[test]
fn tty_communications_set_up_the_8251_and_receive() {
    let Some(img) = tty_image() else { return };
    let out = boot_with(&img, "130000000", &["--keys", "{+77}i{-77}{ms 1500}ctty{14}{ms 8000}", "--keys-at", "80000000",
        "--serial-in", "{48}{C5}{CC}{CC}{CF}{8D}{0A}", "--serial-in-at", "105000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |TTY COMMUNICATIONS ENABLED"), "{}", out);
    let serial = out.lines().find(|l| l.starts_with("serial 8251")).unwrap_or_else(|| panic!("no serial line:\n{}", out));
    // Reset, 4CH, mode, command, then 4CH = 09. The transmitter stays on.
    assert!(serial.contains("control writes: 4A:00 4A:00 4A:00 4A:40 4C:00 4A:4E 4A:37 4C:09"), "{}", serial);
    assert!(serial.contains("mode 4E, command 37, 4CH = 09"), "{}", serial);
    assert!(out.contains("  sent 0 bytes: \"\""), "{}", out);
    assert!(out.contains("  received 7 bytes (0 still waiting)"), "{}", out);
    assert!(out.lines().any(|l| l.contains(" |HELLO ") && l.split('|').nth(1).map(str::trim_end) == Some("HELLO")),
        "HELLO on the screen:\n{}", out);
    let acks = out.lines().find(|l| l.starts_with("interrupt acknowledges")).unwrap();
    assert!(acks.contains(" 02x"), "receive interrupts (RST 1) acknowledged: {}", acks);
    assert!(!out.contains("  4AH x") && !out.contains("4CH x"), "4AH-4CH are mapped now:\n{}", out);
}

/// The TTY keys enabling communications, then CODE + K for the menus. A menu item is picked
/// with SPACE (next item) and RETURN. COMMUNICATIONS starts on Select, SELECTION on
/// Transmission, RECEPTION on None.
const TTY_ON: &str = "{+77}i{-77}{ms 1500}ctty{14}{ms 8000}";
const TTY_MENU: &str = "{+77}k{-77}{ms 3000}";

/// Select, Transmission, KB: what is typed is sent as 7-bit ASCII with the even parity the
/// control page asks for (made by the software; the chip runs 8 bits, no parity), RETURN as CR.
#[test]
fn tty_transmission_from_the_keyboard_sends_what_is_typed() {
    let Some(img) = tty_image() else { return };
    let keys = format!("{TTY_ON}{TTY_MENU}{{14}}{{ms 3000}}{{14}}{{ms 3000}}{{14}}{{ms 3000}}hello{{ms 500}}{{14}}{{ms 3000}}");
    let out = boot_with(&img, "150000000", &["--keys", &keys, "--keys-at", "80000000"]);
    assert!(out.contains("  sent 6 bytes: \"{E8}ello{8D}\""), "{}", out);
    assert!(last_row(&out, "   1 |").starts_with("   1 |TTY COMMUNICATIONS ENABLED"), "{}", out);
}

/// Select, Reception, Disk asks `FILE NAME:`, then shows RECEPTION IN PROCESS and files what
/// arrives as that document: parity stripped, LF dropped, CR kept as the CPT's new line. The
/// text arrives by cycle 125.3M. About 25 s later (174M–176M), with no key pressed, the CPT
/// ends the reception itself (RECEPTION STOPPED) and the document is on the disk [HYPOTHESIS:
/// an idle-line timeout; whether the control page sets it is unknown].
#[test]
fn tty_reception_to_disk_files_a_document() {
    let Some(img) = tty_image() else { return };
    let saved = std::env::temp_dir().join(format!("cpt-rx-{}.img", std::process::id()));
    let keys = format!("{TTY_ON}{TTY_MENU}{{14}}{{ms 3000}}{{10}}{{ms 1000}}{{14}}{{ms 3000}}{{10}}{{ms 1000}}{{14}}{{ms 3000}}rxtest{{14}}{{ms 12000}}");
    let out = boot_with(&img, "180000000", &["--keys", &keys, "--keys-at", "80000000",
        "--serial-in", "{48}{C5}{CC}{CC}{CF}{8D}{0A}{F3}{E5}{E3}{6F}{EE}{E4}{8D}{0A}", "--serial-in-at", "125000000",
        "--save-disk1", saved.to_str().unwrap()]);
    let bytes = std::fs::read(&saved).expect("the saved disk");
    let _ = std::fs::remove_file(&saved);
    assert!(last_row(&out, "   1 |").starts_with("   1 |RECEPTION STOPPED"), "{}", out);
    assert!(out.contains("  received 15 bytes (0 still waiting)"), "{}", out);
    let doc = b"\x01RXTEST\x02HELLO\rsecond\r\r\x03";
    assert!(bytes.windows(doc.len()).any(|w| w == doc), "RXTEST's text on the disk");
}

/// CODE + SPACE types a coded space (90H): one column, shown as a small dot.
#[test]
fn code_space_types_a_coded_space() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "86000000", &["--keys", "ab{+77}{10}{-77}cd{ms 1500}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "  46 |").starts_with("  46 |ab·cd "), "{}", last_row(&out, "  46 |"));
}

/// JP-M24's heading is underlined: a C4 before each character of `M E M O R A N D U M`. The
/// row stays black on white (its B0 slots only follow the underlined part).
#[test]
fn underlined_text_shows_underlined() {
    let Some(img) = tty_image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "{3D}jp-m24{14}{ms 8000}", "--keys-at", "80000000"]);
    let row = out.lines().filter(|l| l.contains("M E M O R A N D U M")).last().unwrap_or_else(|| panic!("{}", out));
    assert!(row.ends_with("| underlined at 24-42"), "{}", row);
}

/// PRINT, RETURN prints the document on printer A (the daisy-wheel port at 42H/43H). The
/// emulated printer is always ready and loads the next sheet when the CPT asks for one, so the
/// two-page letter comes out whole. The characters are decoded through the print-wheel table
/// the driver uses, and laid out by carriage (1/60 inch) and paper (1/48 inch) position.
#[test]
fn print_sends_the_letter_to_the_daisy_wheel_printer() {
    let Some(img) = image() else { return };
    let text = std::env::temp_dir().join(format!("cpt-print-{}.txt", std::process::id()));
    let out = boot_with(&img, "200000000", &["--keys", "{3D}rc-l1{14}{ms 8000}{7D}{ms 3000}{14}{ms 60000}", "--keys-at", "80000000",
        "--printer-text", text.to_str().unwrap()]);
    let printed = std::fs::read_to_string(&text).expect("the printout");
    let _ = std::fs::remove_file(&text);
    let report = out.lines().find(|l| l.starts_with("printer A (42H/43H)")).unwrap_or_else(|| panic!("no printer report:\n{}", out));
    assert!(report.contains("strobe x count: 00x1 "), "one sheet loaded: {}", report);
    let pages: Vec<&str> = printed.split('\u{0C}').collect();
    assert_eq!(pages.len(), 2, "{}", printed);
    let lines: Vec<&str> = pages[0].lines().collect();
    assert_eq!(lines[0], "           August 19, 1987", "{}", pages[0]);
    assert_eq!(lines[5], "           Mr. Carl Wales");
    assert!(lines.contains(&"           Dear Carl:"), "{}", pages[0]);
    assert!(pages[1].starts_with("           has been terminated, effective July 31, 1987.\n"), "{}", pages[1]);
    assert!(pages[1].contains("           ROY CLESTER\n"), "{}", pages[1]);
    assert!(!printed.contains('?'), "every character decoded:\n{}", printed);
}

/// The TTY disk has its own copy of the driver and print-wheel table (block 1, 999AH).
#[test]
fn print_works_on_the_tty_disk_too() {
    let Some(img) = tty_image() else { return };
    let out = boot_with(&img, "160000000", &["--keys", "{3D}directions{14}{ms 8000}{7D}{ms 3000}{14}{ms 30000}", "--keys-at", "80000000"]);
    assert!(out.contains("  printout (print-wheel table in block 1, "), "{}", out);
    assert!(out.contains("    |           D. BULMER\n") || out.contains("    |           D. BULMER\r\n"), "{}", out);
    assert!(out.lines().any(|l| l.trim_end() == "    |                             THURSDAY, JULY 23, 1987"), "{}", out);
}

/// A printer that is off reads 00H: the CPT restores it, waits, and says so (message 8.51).
#[test]
fn print_with_the_printer_off_says_so() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "112000000", &["--keys", "{3D}rc-l1{14}{ms 8000}{7D}{ms 3000}{14}{ms 8000}", "--keys-at", "80000000",
        "--printer-status", "00", "--screen-at", "111000000"]);
    assert!(out.lines().any(|l| l.starts_with("   1 |PRINT      1 RC-L1") && l.contains("8.51")), "{}", out);
    let report = out.lines().find(|l| l.starts_with("printer A (42H/43H)")).unwrap();
    assert!(report.contains("status 00,") && report.contains("(strobe x count: 80x1)"), "{}", report);
}

/// The speaker (8253 counter 0): start-up plays sound 1 from the table at 1581H (2000 Hz for
/// 100 ms, then 1000 Hz for 250 ms), and IN clicks (sound 0: 1333 Hz for 50 ms).
#[test]
fn the_speaker_chimes_at_start_up_and_clicks_on_in() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "{3D}", "--keys-at", "80000000"]);
    // "speaker (6 changes): 43725401: 2000 Hz, 43932427: off, ..." -> ["2000 Hz", "off", ...]
    let spk = out.lines().find(|l| l.starts_with("speaker (")).expect("speaker line");
    let list = spk.split_once("): ").expect("speaker list").1;
    let tones: Vec<&str> = list.split(", ").map(|e| e.split_once(": ").map_or(e, |(_, t)| t)).collect();
    assert_eq!(tones, ["2000 Hz", "off", "1000 Hz", "off", "1333 Hz", "off"], "{}", spk);
}

/// The keyboard program (PROG, key 02H): CODE + PROG starts recording ("PROG" on the title
/// line), the keys typed are recorded, CODE + PROG again ends it, and PROG alone plays the
/// recording back. Here "ab" is recorded, RETURN starts a new line, and PROG types "ab" again.
#[test]
fn prog_records_and_replays_keys() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "90000000", &["--keys", "{+77}{02}{-77}{ms 600}ab", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |      PROG"), "recording: {}", last_row(&out, "   1 |"));
    let out = boot_with(&img, "110000000", &["--keys", "{+77}{02}{-77}{ms 600}ab{ms 300}{+77}{02}{-77}{ms 600}{14}{02}{ms 3000}",
        "--keys-at", "80000000"]);
    assert!(last_row(&out, "  45 |").starts_with("  45 |ab "), "the recorded line: {}", out);
    assert!(last_row(&out, "  46 |").starts_with("  46 |ab "), "PROG played it back: {}", out);
    assert!(!last_row(&out, "   1 |").contains("PROG"), "recording ended: {}", last_row(&out, "   1 |"));
}

/// CLEAR TAB SET (key 75H) toggles a tab stop at the cursor's column: one press sets it (glyph
/// 84H on the ruler), a second press at the same column clears it, a third sets it again. The
/// cursor moves on (typing "xy") so its own marker doesn't cover the tab marker.
#[test]
fn clear_tab_set_toggles_a_tab_stop() {
    let Some(img) = image() else { return };
    let ruler = |keys: &str| -> String {
        let out = boot_with(&img, "95000000", &["--keys", keys, "--keys-at", "80000000"]);
        last_row(&out, "  47 |")[6..16].to_string()
    };
    assert_eq!(ruler("Hello{75}{ms 300}xy"), "M...:>.^.|", "one press sets a tab at column 6");
    assert_eq!(ruler("Hello{75}{ms 300}{75}{ms 300}xy"), "M...:..^.|", "a second press clears it");
    assert_eq!(ruler("Hello{75}{ms 300}{75}{ms 300}{75}{ms 300}xy"), "M...:>.^.|", "a third sets it again");
}
