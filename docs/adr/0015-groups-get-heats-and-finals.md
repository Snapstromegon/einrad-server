# Groups get heats and finals

Group categories are no longer their own final (#345, amends ADR 0013). Every category belongs to a heat, and every discipline has finals. Konzept §1.5: the split into Junior Expert and Expert applies to group acts too, and groups start in the final.

Each group category gets **its own heat and its own final**, named after the category ("KGU15"), e.g. a Kleingruppe Junior Expert heat and final. The heat's Vorlauf is **skipped by default** (ADR 0014), whatever the number of groups: the tool never suggests anything for a group heat (no threshold suggestion), and the organizer may un-skip it. The stored threshold follows the usual default only so the column stays set; it drives nothing for groups. Heats still share one discipline, so Kleingruppe and Großgruppe categories never share a heat.

A final is tied to its heats only through its slot groups, and a slot group needs at least one slot. A group final therefore has **one slot group of its heat with one slot**. The slot stays unused while the Vorlauf is skipped; un-skipping it means setting the number of finalists. The alternatives (a slot group of 0 slots, or a separate link between finals and heats) would have changed the model of ADR 0013 and 0014 for a case that is not needed.

A category without a heat gives a **warning** (competition file import, admin UI), not an error, so a category can be created before its heat. In the competition file a group category is put into a `[[heats]]` entry like any other and its final is a `[[finals]]` entry; a group heat without `skip` is skipped.

The **migration** gives every existing group category without a heat a heat and a final of its name; if a heat or final of that name exists, " 2", " 3", … is added. The final takes the category's times and one slot. The category's timeplan block stays, so existing timeplans keep their shape, and is marked "Vorlauf entfällt" through the skip; the final's block follows it and takes its start time, settings and stored slot order. The Vorlauf is not skipped if an act of the category has already started.
