# One competition per database, a fresh database each season

The tool runs exactly one competition: the config's `[competition]` replaces any other competition row, and categories, the timeplan, finals and jury categories are seeded only into empty tables. Each season starts with a fresh database; the 2024–2026 migrations were squashed into one baseline that can't migrate an old season's database (originals kept in `migrations-archive/`).

We chose this over multi-event support (#160) because one competition a year is all the clubs need, and keeping a single competition keeps every rule (deadlines, fees, ages counted on the determining day) free of an event dimension. Supporting several events later means adding that dimension to most tables, not just a column.
