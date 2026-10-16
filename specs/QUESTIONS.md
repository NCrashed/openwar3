# Open questions

Behaviour the implementation needs but no spec covers yet. Analysts answer
a question with a spec (see [TEMPLATE.md](TEMPLATE.md)) and remove it here.
Each entry says where the question came from and what the code does until
it is answered.

## Trigger strings (`war3map.wts`)

- **Header without a body.** `(1)TheDeathSheep.w3m` has `STRING 681`
  followed by a bare `#VALUE!` line instead of `{ ... }`. What does the game
  show for `TRIGSTR_681`, and does the bare line affect the entries after it?
  Until answered: the entry is undefined and parsing resumes at the next
  `STRING` header.
- **Repeated numbers.** If a number appears twice, which text does the game
  use? Until answered: the later one.
- **Unresolved references.** What does the game show for a `TRIGSTR_<n>`
  whose number is not defined? Until answered: the caller gets no text.

## Map info (`war3map.w3i`)

The layout is confirmed by parsing every shipped map to its last byte; the
meaning of these fields comes from community notes only:

- order of the four camera bound corners;
- channel order of the fog and water colours;
- meaning of the flag bits and of the player ally priority masks.
