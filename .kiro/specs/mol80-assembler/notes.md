* instruction chaining: I think a sequence such as 

123:A
A:PORT(78)

could be written as 123:A:PORT(78)

PORT(78):A
A:(HL)

could be written as PORT(78):(HL)<A>

but not 1005 sure.