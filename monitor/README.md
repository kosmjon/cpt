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
