## Key chords and on-screen latching

The real keyboard was N-key rollover, so the operator could hold one key with a finger and
strike another to form a **chord** — e.g. `CODE+IN` (table of contents) vs. plain `IN` (read a
file). On the physical keyboard a chord is simply two overlapping make events (CODE-down, then
IN-down, with no CODE-up in between); the CPT software interprets the overlap.

A mouse can only click one on-screen key at a time, so the UI reconstructs chords with a
**latching** affordance:
- Clicking a **latching modifier** (CODE, and any other keys the hardware treats as held
  modifiers) latches it down (sends its make event; does NOT send the break yet) and shows it
  held.
- Clicking it again **unlatches** it (sends the break).
- Clicking a **non-latching key** while modifiers are latched sends that key's make event, then
  sends the break events for all latched modifiers, and clears the latches — reproducing
  exactly the make/break sequence a two-finger chord would have generated.

The keyboard *device* needs no special chord logic: it remains a plain make/break event pipe
(faithful to the real N-key-rollover hardware). Latching lives entirely in the UI. [CONFIRMED
behavior; exact key codes still VERIFY]

## CODE-O

CODE-O and giving it an operation number of 1-7. 1 is word processing, 2 is word processing with less RAM but more printer options, 3&4 are reserved. 5 is Work Disk Duplication, 6 is Disk Erase and 7 is Work Disk Repair. 

The drives are called STATION ONE and STATION TWO.
You don't Load and Save and there's no DIR command. 

Instead you hit CODE-IN, tell it which station number and it will give you a Table Of Contents. 

Then you can hit IN and give it the station number and label you want to read. 

Saving is done with the OUT key. Again you give it a station number and a label. CPT will automatically increment the end number of a label so you can have multiple versions of things or a long document made of shorter ones - screen space is limited to one full page of A4 and the machine will complain if screen memory is getting too full.
