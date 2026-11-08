//! Boot the real CPT program disk (`disk/cpt.img`, extracted from `cpt.td0` by
//! `tools/td0.py`) on the real-hardware model, through `cpt-run --cpt-boot`.
//!
//! If the image has not been extracted these tests print a note and pass without running.

use std::process::Command;

fn image() -> Option<String> {
    let path = format!("{}/../../disk/cpt.img", env!("CARGO_MANIFEST_DIR"));
    if std::path::Path::new(&path).exists() {
        Some(path)
    } else {
        eprintln!("skipping: run  python tools/td0.py .kiro/specs/cpt8000-emulator/cpt.td0 --raw disk/cpt.img");
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
/// top message line, 44 text rows, the typing line (row 46), the ruler with margins at columns
/// 1 and 80 and the cursor marker at column 1, and the status row (column 1, line 0). The
/// screen ends at the B1 mark. Getting here needs the floppy interrupt (RST 3) and the drive
/// Not Ready probe; without them the system halts on error 1 at 15C1H.
#[test]
fn the_started_system_shows_its_editing_screen() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "80000000", &[]);
    assert!(out.contains("screen from the display list at 7FF8H (48 rows)"), "{}", out);
    let ruler = last_row(&out, "  47 |");
    assert!(ruler.starts_with("  47 |^...:....|....:....|"), "ruler, cursor at column 1: {}", ruler);
    assert!(ruler.ends_with("....:....M| white on black"), "right margin at column 80, reversed: {}", ruler);
    let status = last_row(&out, "  48 |");
    assert!(status.contains(" 1    0 "), "status row: {}", status);
    assert!(out.contains("ended at: end mark at"), "{}", out);
}

/// Keys typed through the real keyboard path: each key goes out as a make/break event on
/// 01H/02H, the RST 2 handler puts it in the ring at 058BH, the editor's task translates it with
/// the table at DEFFH (block 2), and the text appears on the typing line. RETURN (key 14H)
/// moves "Hello" up a line. The status row then shows column 6, line 1, and the ruler's cursor
/// marker follows the column.
#[test]
fn typing_shows_text_on_the_typing_line() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "Hello{14}World!", "--keys-at", "80000000"]);
    let kb = out.lines().find(|l| l.starts_with("keyboard:")).expect("keyboard line");
    assert!(kb.contains("0 still waiting"), "{}", kb);
    assert!(last_row(&out, "  45 |").starts_with("  45 |Hello   "), "{}", out);
    assert!(last_row(&out, "  46 |").starts_with("  46 |World!   "), "{}", out);
    assert!(last_row(&out, "  47 |").starts_with("  47 |M...:.^..|"), "cursor marker at column 7:\n{}", out);
    let status = last_row(&out, "  48 |");
    assert!(status.contains(" 7    1 "), "status row (column 7, line 1): {}", status);
    // The title, ruler and status rows are white on black; text and the typing line are not.
    assert!(out.contains("white on black (rows): 1, 47, 48"), "{}", out);
    assert!(last_row(&out, "  47 |").ends_with("| white on black"), "the whole ruler is reversed");
    assert!(last_row(&out, "  46 |").ends_with('|'), "the typing line is black on white");
}

/// The unit keys name themselves on the status row, and IN/OUT/PRINT open a command prompt on
/// the top line that takes the typed document name (upper-cased).
#[test]
fn command_keys_show_their_prompts() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "100000000", &["--keys", "Hello{2E}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "  48 |").contains("WORD"), "{}", out);
    let out = boot_with(&img, "100000000", &["--keys", "Hello{2D}world", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |OUT        1 WORLD"), "{}", out);
}

/// IN, a document name and RETURN open a document from the disk through the system's own
/// filer. LEGAL is a print-wheel table: its first line goes on the typing line (row 46) and
/// the following lines below the ruler and status rows. The filer reads its four blocks at
/// track 48, physical sectors 0, 3, 6 and 9 (logical 0-3 with the skew of 3), and the screen
/// matches those sectors byte for byte.
#[test]
fn in_opens_a_document_from_the_disk() {
    let Some(img) = image() else { return };
    let out = boot_with(&img, "110000000", &["--keys", "{3D}legal{14}", "--keys-at", "80000000"]);
    assert!(last_row(&out, "   1 |").starts_with("   1 |IN         1 LEGAL"), "{}", out);
    assert!(last_row(&out, "  46 |").starts_with("  46 |T.017-5  REV (G-1-1)    USA LEGAL           03/31/81  LEGAL"), "{}", out);
    assert!(last_row(&out, "  49 |").starts_with("  49 |1   20  22  23  24  25  26  229 8   9"), "{}", out);
    let reads = out.lines().find(|l| l.contains("after the keys started")).expect("read list");
    assert!(reads.contains("(4): t48s0(768) t48s3(771) t48s6(774) t48s9(777)"), "{}", reads);
}

/// Line-down (key 0FH) moves through an open document: the letter RC-L1 scrolls up past the
/// typing line, its own format line (margins at 1 and 60, white on black) goes above, and the
/// main ruler takes the document's margins.
#[test]
fn line_down_scrolls_through_a_letter() {
    let Some(img) = image() else { return };
    let keys = format!("{{3D}}rc-l1{{14}}{{ms 8000}}{}", "{0F}{ms 300}".repeat(14));
    let out = boot_with(&img, "150000000", &["--keys", &keys, "--keys-at", "80000000"]);
    assert!(last_row(&out, "  32 |").starts_with("  32 |Letter to Carl Wales"), "{}", out);
    assert!(last_row(&out, "  33 |").starts_with("  33 |M...:....|") && last_row(&out, "  33 |").ends_with("white on black"),
        "the letter's format line:\n{}", out);
    assert!(last_row(&out, "  45 |").starts_with("  45 |Dear Carl:"), "{}", out);
    assert!(last_row(&out, "  49 |").starts_with("  49 |Since July 13, 1987, you began calling in sick on the"), "{}", out);
    assert!(last_row(&out, "  48 |").contains(" 1   14 "), "line 14: {}", last_row(&out, "  48 |"));
    assert!(last_row(&out, "  47 |").starts_with("  47 |^...:....|....:....|....:....|....:....|....:....|....:....M"),
        "the ruler takes the letter's right margin (60): {}", last_row(&out, "  47 |"));
}
