# MOL80 multitasking monitor (throwaway study)

`monitor.mol` is a small preemptive multitasking monitor written in MOL80, after
`.kiro/specs/cpt8000-emulator/8080_multitasking_monitor_notes.md`. It runs on the emulated CPT
machine with the 100 Hz task-switch timer:

```
cargo run -q -p mol80asm -- --bin monitor/monitor.bin monitor/monitor.mol
cargo run -p cpt-run -- --timer --serial-listen 127.0.0.1:2323 monitor/monitor.bin
```

The full task API (`SPAWN`, `SETQUANT`, `YIELD`, `WAITSEM`, `SIGNALSEM`), the stack-pool
design, and the interrupt-latency rules are described in the header comment of `monitor.mol`.
`cargo test -p cpt-run --test monitor` exercises all of it.

## The PL/M-80 version

`monitor.plm` is the same monitor written in Intel PL/M-80: the same TCB layout and data
addresses, stack pool, per-task quanta, fair semaphore, and demo tasks. Build and run:

```
powershell -File monitor/build-plm.ps1
cargo run -p cpt-run -- --timer --serial-listen 127.0.0.1:2323 monitor/plm-build/monitor.bin
```

`build-plm.ps1` compiles, links and locates with Mark Ogden's C ports of the Intel ISIS-II
tools (`plm80`, `link`, `locate`, `aomf2bin`), which `get-plm80-tools.ps1` downloads once into
`monitor/plm80tools/` from [ogdenpm/intel80tools](https://github.com/ogdenpm/intel80tools).
They are third-party binaries for hobbyist and academic use, so they are fetched rather than
committed. Build output goes to `monitor/plm-build/` (both gitignored). The code is located at
2000H, above the fixed data areas, with a jump at 0000H and the timer handler at 0020H.
`cargo test -p cpt-run --test monitor_plm` runs the same checks as for the MOL80 version (they
skip themselves if the binary has not been built).

What had to change, because PL/M-80 has no inline assembly:

- **The context switch lives in the timer interrupt procedure.** Only an `INTERRUPT`
  procedure saves and restores every register, so `TICK` switches tasks by swapping the
  built-in `STACKPTR`. A task therefore cannot switch away by calling the monitor:
  `YIELD`, `WAITSEM` and `SYSEXIT` mark the task, set the quantum to expire, and `HALT`, and
  the next tick does the switch. A voluntary switch costs up to one tick (10 ms) of latency,
  where the MOL80 version switches at once.
- **SPAWN primes the frame the compiler's interrupt epilogue pops**: flags, A, C, B, E, D, L,
  H, then the entry point and `SYSEXIT`. This was read off the compiler's CODE listing; recheck
  it if the compiler changes.
- **The print-lock test flag is inverted**: `LOCKOFF` at 0C00H, nonzero = lock off. PL/M
  cannot give a variable at a fixed address an initial value, so the power-on 0 has to mean
  "lock on".
- Shared procedures are `REENTRANT`, so their locals live on each task's own stack.
- Each stack block is 256 bytes, not 128: compiled code with reentrant locals needs more.

A compiler pitfall worth knowing: `TCB(CURRENT).SP = STACKPTR` in a single statement saves an
SP that is **2 bytes low**, because the compiler pushes HL while it computes the element
address and then reads SP with that push still on the stack. Reading `STACKPTR` into a plain
variable first avoids it.

Measured: interrupts are off 2.3% of the time and the longest disabled stretch is about
1,700 cycles (the compiled `TICK`), against 653 for the hand-written MOL80. CPU share still
follows the quantum (A:B = 2.02 for quanta 10:5).

## Using semaphores to build critical sections

`WAITSEM` and `SIGNALSEM` are general: any routine can make a critical section by bracketing
code with them. `PRTLINE` does exactly this to keep two tasks' output lines from interleaving.

```
MYROUTINE PRTSEM:A          * the number of the semaphore that guards the shared thing
          CALL(WAITSEM)     * blocks until this task owns it
          ...               * critical section
          PRTSEM:A
          CALL(SIGNALSEM)   * hand it to the next waiter, or mark it free
          RTN
```

What it gives you:

- Only one task at a time is between the two calls. Interrupts stay enabled and the other
  tasks keep being scheduled; only tasks wanting the same lock wait.
- `WAITSEM`/`SIGNALSEM` preserve `BC` and `DE` and clobber `A`, `HL` and the flags, so a
  routine can keep working values in `BC`/`DE` or on its stack across the calls.
- Use one semaphore number per shared resource, so unrelated critical sections do not block
  each other. Semaphore 0 (`PRTSEM`) guards the serial port.
- Waiters are served in ring order starting just after the task that signals, so none is
  starved. Ownership passes straight to the woken task (handoff).

### Limits

1. **Not re-entrant.** A task that already holds a semaphore and calls `WAITSEM` on it again
   (for example a locked routine calling another routine that takes the same lock) blocks on
   itself forever. Nested use needs a different semaphore, or a recursive lock.
2. **No ownership check.** Any task can call `SIGNALSEM`, even one that never waited, which
   would let a second task into the section. It is a convention, not enforced.
3. **A task that exits while holding a lock never releases it.** Everyone waiting on that
   semaphore hangs. The exit path (`SYSEXIT`) does not release held locks.
4. **A fixed table.** Semaphore numbers index a table at `SEMBASE` (0C10H), two bytes each
   (value 1 = free / 0 = held, waiter count). Only semaphore 0 is initialised to "free"; more
   semaphores need table rows with initial value 1 and room for them in memory.
5. **Not for interrupt handlers.** `WAITSEM` can block, and both calls re-enable interrupts on
   return, so neither is safe inside an interrupt handler.
6. **No timeout and no priority.** A waiter waits as long as it takes, and waiters are served
   in ring order, not by importance.

### Possible additions (not built)

- `LOCK` / `UNLOCK` as friendlier names for the same two calls.
- A `CRITICAL` helper taking a semaphore number and a subroutine address: lock, call it,
  unlock, so a caller cannot forget the unlock.
- A recursive lock with an owner and depth count (removes limit 1; also enables an ownership
  check for limit 2).
- Release of a task's held locks when it exits (removes limit 3).
- Semaphore initialisation at start-up from a list, instead of hand-written table rows.
