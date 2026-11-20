# Jury seats: every assignment occupies a numbered seat of its Durchgang

This refines ADR 0019. The jury table of a Durchgang has numbered places: Technik T1, T2, ..., Performance P1, P2, ..., Abstiege A1, A2, ... The jury lead assigns judges to those places in a seat table (#387), and the published plan tells each judge where to sit.

**A seat is the task's letter and a number.** `T`, `P` or `A`, then `n` from 1 up to the Durchgang's effective needed count of that task (its override, else the unit's). Six performance judges needed means P1 to P6.

**Every assignment has exactly one seat**, unique per (Durchgang, task, seat). "Assigned without a seat" does not exist. A seat can be empty, and an empty seat is a missing judge: it replaces the "Technik: 1 von 5" counts in the plan warnings ("1 Platz unbesetzt (A3)").

**Assigning takes the chosen seat or the lowest free one**, and is refused when there is none: a seat outside 1..needed does not exist, a taken seat is a conflict, and a full task refuses another judge. No phantom seats can be created through the API. A judge who is already seated in the Durchgang may be moved to another seat (or swapped with its occupant) by one command in one transaction (`seat_judge`); a judge sits once per Durchgang, as before.

**A gap stays.** Unassigning a judge leaves an empty seat; the other seats are not renumbered, so nobody has to look for their seat again.

**Lowering a needed count never strands a judge.** Lowering the unit's count or a Durchgang's override (or clearing an override) is refused while a seat above the new count is occupied; the jury lead clears the seat first. Only a real lowering counts: saving other fields of a unit whose migrated seats already exceed its count still works.

**Re-merging** gives every kept assignment of a removed Durchgang its old seat in the target if that is free, else the lowest free one; if that is beyond the target's count the save is refused.

**Migration.** The migration numbers the existing assignments per Durchgang and task by assignment id (UUIDv7, so in the order they were assigned). The old rules allowed more judges than needed; those get seats beyond the count, which the seat table shows as extra seats and which new assignments cannot create. The down migration drops the seat column and keeps the assignments.

**Published plan.** One entry per assignment (so per seat) with seat, Durchgang label, day, block and time, which #388 groups per judge.
