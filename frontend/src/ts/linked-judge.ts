/** Pure helpers for judges that are linked to a rider (#384). */

interface RiderLike {
  id: string;
  firstname: string;
  lastname: string;
}

/** Same comparison as `person_key` in `src/people.rs`. */
const personKey = (firstname: string, lastname: string) =>
  `${firstname.trim()} ${lastname.trim()}`.toLowerCase();

/** Riders no judge is linked to yet, sorted by last name, first name. */
export const availableRiders = <R extends RiderLike>(
  riders: R[],
  judges: { rider_id?: string | null }[],
): R[] => {
  const taken = new Set(judges.map((j) => j.rider_id).filter(Boolean));
  return riders
    .filter((r) => !taken.has(r.id))
    .sort(
      (a, b) =>
        a.lastname.localeCompare(b.lastname, "de") ||
        a.firstname.localeCompare(b.firstname, "de"),
    );
};

/** The available rider with the typed name, if any. */
export const matchingRider = <R extends RiderLike>(
  firstname: string,
  lastname: string,
  riders: R[],
  judges: { rider_id?: string | null }[],
): R | null => {
  if (!firstname.trim() || !lastname.trim()) return null;
  const key = personKey(firstname, lastname);
  return (
    availableRiders(riders, judges).find(
      (r) => personKey(r.firstname, r.lastname) === key,
    ) ?? null
  );
};

export const isLinked = (judge: { rider_id?: string | null }): boolean =>
  !!judge.rider_id;

/** What deleting the judge together with the rider throws away. */
export const deleteImpactText = (
  registrations: number,
  assignments: number,
): string => {
  const parts: string[] = [];
  if (registrations > 0) {
    parts.push(
      `${registrations} ${registrations === 1 ? "Anmeldung" : "Anmeldungen"}`,
    );
  }
  if (assignments > 0) {
    parts.push(
      `${assignments} ${assignments === 1 ? "Einteilung" : "Einteilungen"}`,
    );
  }
  if (parts.length === 0) return "";
  const verb = registrations + assignments === 1 ? "wird" : "werden";
  return `${parts.join(" und ")} ${verb} ebenfalls gelöscht`;
};
