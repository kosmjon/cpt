# Possible future tasks

Ideas that have been assessed but not started. Bugs and small fixes are in `bugs.md`.

## "Try it now" on GitHub Pages

Offer the browser emulator on GitHub Pages, so people can try the CPT without installing
anything. Assessed 2026-11, no code changed yet. **Feasible**: the page is already a static
site.

### What already fits

- The page is static: `index.html`, `cpt.js`, `real-cpt.js`, `speaker-audio.js` and
  `cpt_wasm.wasm` (about 260 KB). It needs no server code and no special headers.
- Paths are relative (`fetch("cpt_wasm.wasm")`, `src="cpt.js"`), so it works under a project
  URL like `https://<user>.github.io/cpt/`. GitHub Pages serves `.wasm` correctly.
- Everything runs in the browser: written disks are offered as downloads, and the printer pane
  and SAVE work offline. Sound starts on the first click, as browsers require.
- Start-up runs faster than real time (about 15 s of wall time to settle in the page check).

### What's needed

1. **A build step.** `cpt_wasm.wasm` isn't in git (`run-web.bat` builds it). A GitHub Actions
   workflow would:
   - install Rust and the `wasm32-unknown-unknown` target;
   - run `cargo build -p cpt-wasm --target wasm32-unknown-unknown --release`;
   - copy the `.wasm` next to `index.html`;
   - publish `crates/cpt-wasm/web/` to Pages.

   About 30–40 lines. (Committing the `.wasm` would also work, but it goes stale with every code
   change.)
2. **A program disk for visitors.** They would otherwise meet "PLEASE INSERT PROGRAM DISK" with
   nothing to insert, because the `.img` files are kept out of git on purpose. Options:
   - Have the workflow run `tools/setup_disks.py` on the `.td0` files already in the repo and
     publish `cpt.img` with the page. Add a small "Load sample program disk" button to the page
     that fetches it into drive 1 (about 20 lines in `cpt.js`). No `.img` is committed.
   - Or decode `.td0` in the browser by porting the TD0 decoder. More work, and visitors would
     still need a disk file, so it's not recommended.
3. **A copyright decision (the owner's call).** The `.td0` files are already public in the repo,
   but a one-click site makes the CPT program software much easier to use and pass around.
   Options:
   - publish the emulator only, and let visitors bring their own disks;
   - publish one program disk (e.g. `cpt.img`) with a short notice about its origin.
4. **README:** a "Try it now" link, and the notice. Then turn Pages on in the repo settings
   (Source: GitHub Actions).

### Points to check before publishing

- **Alt is CODE.** In some Windows browsers, a lone Alt press and release moves focus to the
  browser's menu. Test Edge, Chrome and Firefox.
- **Phones and tablets** have only the on-screen special keys, so there's no text typing. Say so
  on the page.
- **TTY** can't connect in a browser (it needs `cpt-run`). Printing works.
- **Caching:** Pages lets browsers keep files for about 10 minutes, so visitors may see the old
  version briefly after an update.
- **Cost:** free for public repositories. The site is under 1 MB with one disk image.

### Effort

A couple of hours including testing, plus the copyright decision.
