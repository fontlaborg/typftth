<!-- this_file: docs/errors.md -->
# Error codes

`InterpreterError::code()` — stable integers carried in the trace blob's
`ins_error` field (0 = no error).

| code | name | meaning |
|---|---|---|
| 1 | arithmeticError | point move overflowed 26.6 |
| 2 | callStackTooDeep | more than 64 nested CALL/LOOPCALL/IDEF |
| 3 | calledFunctionNotDefined | (reserved; undefined functions are silently skipped like the reference) |
| 4 | cvtLocationOutOfBounds | CVT index out of range (reads of non-negative bad indices push 0 instead) |
| 5 | definitionsCannotBeNested | FDEF/IDEF inside FDEF/IDEF |
| 6 | definitionsNotAllowedInGlyf | FDEF/IDEF in a glyph program |
| 7 | illegalInstruction | undefined opcode without IDEF, or 0x7B |
| 8 | internalError | (reserved) |
| 9 | invalidAccessToGlyphZone | glyph zone used outside a glyph program |
| 10 | invalidAccessToTwilightZone | twilight zone used in `fpgm` |
| 11 | invalidOperand | bad zone number, contour, range… |
| 12 | jumpOutOfBounds | (reserved; jumps report 17) |
| 13 | maxpLimitExceeded | function/instruction index or point/contour counts over `maxp` |
| 14 | noInstructionsProvided | (reserved) |
| 15 | noSuchPoint | point index ≥ zone capacity |
| 16 | noSuchContour | contour index ≥ contour count |
| 17 | ranOffEndOfInstructions | program/operands/jump past the end |
| 18 | stackDepthExceedsLimit | stack over 65535 entries |
| 19 | stackUnderflow | pop from an empty stack |
| 20 | storageLocationOutOfBounds | WS/RS index ≥ `maxStorage` |
| 21 | unbalancedIF_ELSE_EIF | (reserved; seeking reports 17) |
| 22 | unexpectedENDF | ENDF outside a definition |
| 100 | stopped | the step observer asked to stop |
