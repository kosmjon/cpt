# 8080 Multitasking Monitor Notes

## 1. Task Control Block (TCB) & Quanta Units

In an **Intel 8080 multitasking monitor** (a primitive real-time operating system or kernel), the **Task Control Block (TCB)** and **Quanta Units** are core mechanisms used to manage and schedule concurrent processes. 

### Task Control Block (TCB)
The TCB is a dedicated structure in memory (RAM) that holds the execution state of a specific task when it is not actively running. Because the 8080 has a limited register set, the TCB must efficiently capture the entire state of the CPU.

A typical 8080 TCB occupies a few consecutive bytes of RAM and contains:
* **Task Pointer / Link:** A 2-byte address pointing to the next TCB, forming a linked list of tasks (Ready Queue).
* **Saved Program Counter (PC):** The 2-byte address where the task will resume execution.
* **Saved Stack Pointer (SP):** The 2-byte address pointing to the task's private stack.
* **Task Status:** A 1-byte flag indicating if the task is *Ready*, *Running*, *Blocked* (waiting for I/O), or *Suspended*.
* **Saved Registers (Optional in TCB itself):** The 8080 registers (`A`, `Flags/PSW`, `B`, `C`, `D`, `E`, `H`, `L`) are usually **pushed onto the task's individual stack** during a context switch rather than stored in the TCB block directly. The TCB then just saves the Stack Pointer.

### Quanta Units (Time Slicing)
A **quantum** (plural *quanta*) is the fixed amount of CPU time allocated to a task before the monitor preempts it to let another task run. 

* **The Hardware Clock:** An external hardware timer (like an Intel 8253/8254 PIT) is configured to trigger an interrupt (usually `RST` instructions or a vector interrupt) at a regular interval (e.g., every 10 milliseconds). Each interrupt is a "tick."
* **Quanta Management:** Each task is assigned a specific number of "quanta units" (ticks). A dedicated down-counter in the active task's TCB or a global monitor variable is decremented on every clock tick. 
* **Preemption:** When the countdown reaches zero, the task's quantum has expired. The monitor forces a context switch.

### The Multitasking Execution Loop
When a quantum expires or a task voluntarily yields, the 8080 monitor performs the following steps:

1. **Save Context:** The current task's registers (`PUSH PSW`, `PUSH B`, `PUSH D`, `PUSH H`) are pushed onto its stack.
2. **Save SP:** The current Stack Pointer is saved into the active task's TCB.
3. **Schedule:** The scheduler traverses the TCB linked list to find the next *Ready* task.
4. **Reload Quantum:** The monitor resets the quantum unit counter for the new task.
5. **Restore SP:** The Stack Pointer register (`SP`) is loaded with the address from the new task's TCB.
6. **Restore Context:** The new task's registers are popped off its stack (`POP H`, `POP D`, `POP B`, `POP PSW`).
7. **Resume:** A `RET` instruction pops the saved Program Counter (PC) off the new stack, and the new task resumes execution.

---

## 2. Semaphore Implementation for Critical Sections

Implementing a **semaphore** for critical sections on an Intel 8080 requires a primitive that ensures mutual exclusion. In a multitasking environment, testing and modifying a semaphore variable must be an **atomic operation**—it cannot be interrupted halfway through by a clock tick (quantum expiration).

Because the 8080 lacks a hardware "Test-and-Set" instruction, atomicity is achieved by **temporarily disabling interrupts (`DI`)** during the check and enabling them (`EI`) immediately after.

### The Binary Semaphore Structure
For a critical section, a binary semaphore (values `0` or `1`) is used:
* `1` = Available (Resource is free)
* `0` = Locked (A task is currently in the critical section)

A task block list (or wait queue) is often attached to the semaphore to keep track of tasks that are blocked waiting for it.

### Assembly Implementation: `WAIT` and `SIGNAL`

```assembly
; =====================================================================
; SEMAPHORE OPERATIONS FOR INTEL 8080
; =====================================================================

; Assume H,L points to the Semaphore RAM byte before calling.
; Example: LXI H, MY_SEMAPHORE

WAIT_SEM:
    DI                  ; CRITICAL: Disable interrupts to make this atomic
    MOV A, M            ; Load semaphore value into Accumulator
    CPI 1               ; Check if available (value == 1)
    JZ  ACQUIRE         ; If 1, we can grab it immediately

    ; --- BLOCKING STATE ---
    EI                  ; Re-enable interrupts before entering scheduler
    CALL SCHEDULER      ; Yield the CPU
    JMP WAIT_SEM        ; When unblocked and re-scheduled, try again

ACQUIRE:
    MVI M, 0            ; Lock the semaphore (set to 0)
    EI                  ; Safe to re-enable interrupts now
    RET                 ; Enter critical section


SIGNAL_SEM:
    DI                  ; Disable interrupts for atomic update
    MOV A, M            ; Get current state (usually 0)
    
    CALL CHECK_WAIT_QUEUE
    JZ  NO_WAITERS      ; If zero tasks are waiting, just unlock it

    ; --- UNBLOCKING STATE ---
    EI
    RET

NO_WAITERS:
    MVI M, 1            ; No one waiting, simply mark resource as free
    EI                  ; Re-enable interrupts
    RET
```

### TCB Tracking & Workflow

When a task blocks on a semaphore, the monitor updates the **Task Control Blocks** to prevent the CPU from wasting quanta units on a stalled process:

1. **State Transition:** The TCB status field changes from `RUNNING` to `BLOCKED`.
2. **Queue Migration:** The monitor unlinks the TCB from the main scheduling loop (Ready Queue) and links it into the Semaphore's specific linked list. This ensures the scheduler entirely skips this task on future clock ticks.
3. **Waking Up:** When the task currently inside the critical section calls `SIGNAL_SEM`, the monitor intercepts the call, grabs the blocked task's TCB, changes its status back to `READY`, and re-inserts it into the scheduler's loop.

---

## 3. Dynamic Task Creation & Stack Priming

Adding a new task to the system happens during initialization or dynamically at runtime via a system call (often called `SPAWN`, `FORK`, or `CREATE_TASK`). 

To add a task, the monitor must **allocate and format a new TCB**, **initialize a private stack space**, and **link the TCB into the active scheduler loop**.

### Preparing the Task's Private Stack
Before a task can run for the first time, its private stack must be artificially "primed." The monitor fills the stack with dummy data so that the very first time the context switcher runs a `POP` and `RET` sequence on this stack, the CPU cleanly jumps to the task's starting address.

| Stack Offset | Values Loaded by Monitor | Purpose |
| :--- | :--- | :--- |
| **Top of Stack (SP)** | `0x0000` | Dummy `H,L` registers (restored via `POP H`) |
| **SP + 2** | `0x0000` | Dummy `D,E` registers (restored via `POP D`) |
| **SP + 4** | `0x0000` | Dummy `B,C` registers (restored via `POP B`) |
| **SP + 6** | `0x0046` (Flags) + `0x00` (A) | Dummy `PSW` (Accumulator + Flags with interrupts enabled) |
| **SP + 8** | **Task Entry Point (2 bytes)** | The actual memory address where the task's code begins |

### Filling out the TCB
The monitor fills the newly created TCB fields with initial parameters:
* **Next TCB Pointer:** `0x0000` (or pointing to the current head of the queue).
* **Saved SP:** Pointing exactly to the **Top of Stack** defined above.
* **Status Byte:** Set to `0x01` (Ready to run).
* **Quanta Count:** Set to the default value (e.g., `10` ticks).

### Assembly Implementation: Linking to the Ready Queue

```assembly
; =====================================================================
; SYS_CREATE_TASK - Adds a new task to the multitasking loop
; Input:  BC = Address of the new, pre-formatted TCB
; =====================================================================

SYS_CREATE_TASK:
    DI                  ; CRITICAL: Disable interrupts during list mutation
    
    LXI H, TCB_HEAD     ; HL points to the global head pointer variable
    MOV E, M            ; Load low byte of current head into E
    INX H
    MOV D, M            ; Load high byte of current head into D

    MOV A, C            ; Move new TCB low byte to A
    STAX B              ; Store it into new TCB's next-pointer (low)
    INX B
    MOV A, B            ; Move new TCB high byte to A
    STAX B              ; Store it into new TCB's next-pointer (high)
    DCX B               

    LXI H, TCB_HEAD     ; Point back to global head variable
    MOV M, C            ; Store new TCB low byte
    INX H
    MOV M, B            ; Store new TCB high byte

    EI                  ; Safe to re-enable interrupts
    RET                 ; Task is now live
```

---

## 4. Automatic Task Destruction on `RET`

To make a task automatically remove itself upon executing its final `RET`, the monitor can **prime the stack with a "Cleanup Return Address"** during the task's creation phase. 

### The Stack Priming Trick
Instead of leaving the stack blank beneath the task's starting address, you push the address of a specialized monitor function (`SYS_EXIT`) onto the stack *first*.

| Stack Offset | Values Loaded by Monitor | Purpose |
| :--- | :--- | :--- |
| **Initial SP** | Dummy Registers (`H,L`, `D,E`, `B,C`, `PSW`) | Restored on the task's very first context switch. |
| **SP + 8** | **Task Entry Point (2 bytes)** | Popped into PC on the first switch to start the task. |
| **SP + 10** | **`SYS_EXIT` Address (2 bytes)** | **The Magic Hook.** Left sitting on the stack while the task runs. |

### Assembly Implementation: `SYS_EXIT`

```assembly
; =====================================================================
; SYS_EXIT - Automatically triggered when a task executes its final RET
; =====================================================================

SYS_EXIT:
    DI                  ; CRITICAL: Disable interrupts during list mutation
    
    LXI H, CURRENT_TCB  ; Pointer to the active task's TCB address
    MOV E, M
    INX H
    MOV D, M            ; DE now holds the address of the exiting TCB

    CALL SYS_UNLINK_TCB ; Unlink the TCB from the Ready Queue
    
    JMP SCHEDULER       ; Jump straight to scheduler to pick the next task
```