# Juries may be split into sessions by age group; registration stays per unit

This refines ADR 0016. A jury unit (the Vorlauf of a heat, or a final) was judged by one jury. At a competition with many age groups in one heat or final, the jury lead wants several juries for it, one per age group or per set of age groups (#386). The unit stays the thing clubs and judges see; the jury lead splits it into **jury sessions** (Jurydurchgänge).

**Registration stays per unit.** Judges register for a heat's Vorlauf or a final as before, and the club portal never shows or asks for a session. Only the jury plan knows sessions, and only the jury lead sets them up. Deployments (Einsätze) stay on the unit.

**One default session per unit.** Every heat and final has at least one session, created with it. The default session holds all the unit's categories, so an unsplit unit behaves exactly as before. A unit without categories keeps one empty session and cannot be split.

**The jury lead splits.** Each category of a unit is in exactly one session of that unit. A category is in a heat and in the finals the heat feeds, so the membership is stored per unit (`jury_session_categories` with the unit and the category as its key). A final's categories are the categories of the heats that feed it.

**Orphan rule.** A category that joins a unit after the split (a new category, a heat edit) has no row yet and belongs to the unit's first session, the oldest by id. A stored row of a category that has left the unit is stale and ignored. The next save of the split writes the complete membership. Because of this rule a new unit needs no category rows for its default session.

**Needed counts inherit.** A session's needed technical, performance and dismount judges are NULL (inherit the unit's, live) or an override. The unit's `jury_needed_*` stay the default.

**Assignments move to the session and get an id.** `jury_assignments` references its session and has an id of its own, which the merge flow needs. The key is unique per (judge, session): a judge may sit in several sessions of a unit, one task per session. The unit stays denormalised on the assignment for queries.

**Over-max counts a unit once per judge** (#383): a judge in two sessions of one unit uses one deployment's worth of their maximum, not two.

**A session is named by its categories**: their names joined by ", " (for example `EMU23, EMU30`). A unit with a single session shows no label, so unsplit units read as before. The order of a unit's sessions is derived, never stored: a final's by category `order`, then name (a final has no per-category timeplan position); a heat's likewise.

**Re-merging.** When sessions are merged the jury lead keeps individual assignments, not whole sessions: the merge lists every assignment of the sessions being merged, the ticked ones move to the merged session, the others are deleted. A judge may not end up twice in the target.

**Migration.** Every existing heat and final gets one session holding all its categories, and the existing assignments move into it; session and assignment ids are minted as UUIDv7 in SQL (ADR 0018), the random bits from the database's random source. The down migration merges the sessions: for a judge in several sessions of a unit the assignment of the oldest session wins and the others are dropped.
