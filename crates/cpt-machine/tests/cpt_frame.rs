//! The display-memory latch on the real program disk: once the main system runs, its retrace
//! handler (0D5CH) returns normally and the frame is latched every retrace; and the screen does
//! not flicker or jump while CODE + IN builds the table of contents.
//!
//! Needs `disk/cpt.img` (run `setup-disks.bat`); passes with a note if it is missing.

use cpt_machine::cpt_display::CptScreen;
use cpt_machine::cpt_system::{CptSystem, READ_DELAY_CYCLES, RETRACE_PERIOD, SETTLED_CYCLES};

fn boot() -> Option<CptSystem> {
    let path = format!("{}/../../disk/cpt.img", env!("CARGO_MANIFEST_DIR"));
    let Ok(image) = std::fs::read(&path) else {
        eprintln!("skipping: disk/cpt.img not found; run setup-disks.bat (python tools/setup_disks.py)");
        return None;
    };
    Some(CptSystem::boot(&image, READ_DELAY_CYCLES).expect("boots"))
}

#[test]
fn the_real_retrace_handler_latches_every_frame() {
    let Some(mut sys) = boot() else { return };
    sys.run_cycles(40_000_000);
    // Sample through a stretch of the running system: the latch is never more than about a
    // frame old (a retrace period plus the handler's run).
    for _ in 0..50 {
        sys.run_cycles(7_919);
        let age = sys.frame_age().expect("latched");
        assert!(age < 2 * RETRACE_PERIOD as u64, "latch is {age} cycles old");
    }
    let rows = sys.frame_screen(64).rows.len();
    assert!(rows > 10, "the latched frame has the editing screen ({rows} rows)");
}

fn row_text(s: &CptScreen, r: usize) -> String {
    s.rows.get(r).map(|row| row.iter().map(|c| c.code as char).collect()).unwrap_or_default()
}

fn tap(sys: &mut CptSystem, key: u8) {
    sys.keyboard().press(key);
    sys.run_cycles(60_000);
    sys.keyboard().release(key);
    sys.run_cycles(60_000);
}

/// While the TOC is built, every frame keeps CONTENTS on the title row and the same number of
/// rows. Two things used to break that for a frame at a time: decoding mid-way through the
/// retrace handler's queue (a 2-byte patch half applied), and the builder's brief C2 -> C3
/// while it links in a new line, which the decoder took for the status row and merged into
/// the title (the new line flashed over CONTENTS and the rows below moved up).
#[test]
fn building_the_toc_shows_no_torn_frames() {
    let Some(mut sys) = boot() else { return };
    sys.run_cycles(SETTLED_CYCLES);
    // CODE + IN, then RETURN.
    sys.keyboard().press(0x77);
    sys.run_cycles(60_000);
    tap(&mut sys, 0x3D);
    sys.keyboard().release(0x77);
    sys.run_cycles(200_000);
    tap(&mut sys, 0x14);
    sys.run_cycles(4 * RETRACE_PERIOD as u64);

    let mut last_age = u64::MAX;
    let (mut frames, mut rows) = (0, None);
    let mut entries = std::collections::BTreeSet::new();
    for _ in 0..(6 * 60 * RETRACE_PERIOD / 200) {
        sys.run_cycles(200);
        let age = sys.frame_age().expect("latched");
        if age < last_age {
            // A new frame.
            frames += 1;
            let s = sys.frame_screen(64);
            let title = row_text(&s, 0);
            assert!(title.starts_with("CONTENTS"), "frame {frames}: title row {title:?}");
            let n = *rows.get_or_insert(s.rows.len());
            assert_eq!(s.rows.len(), n, "frame {frames}: the rows moved");
            entries.insert(title[40..].trim().to_string());
        }
        last_age = age;
    }
    assert!(frames > 300, "{frames} frames");
    assert!(entries.len() > 10, "the TOC was being built (the count in the title changed): {entries:?}");
}
