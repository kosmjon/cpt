* instruction chaining: I think a sequence such as 

123:A
A:PORT(78)

could be written as 123:A:PORT(78)

PORT(78):A
A:(HL)

could be written as PORT(78):(HL)<A>

but not 100% sure.

FIlename convention:
{0..2 segment}{A-Z variant}{module 000-999 by 20}.mol
e.g. 012A000.MOL   1st module of segment 12 (filing system)

Variants:
A - std 8xxx
B - 6xxx (smaller screen, less memory)
P - Phoenix (bitmapped graphics display)

Secondary Source files (includes)

Conditional Compilation 

if .PHOENIX and not(CPT6000)
endif  * not 100% sure about this...