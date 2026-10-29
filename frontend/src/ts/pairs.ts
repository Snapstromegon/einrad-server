// Unconfirmed pairs across clubs: a pair rider whose partner rides for
// another club isn't linked until an admin confirms it. These helpers find
// the likely partner by the names the clubs entered. Only type imports, so
// the unit tests can import it without a DOM.
import type { schemas } from "./apiClient";
import type { Starts } from "./starts";

type Rider = schemas.Rider & Starts;

/** A name for comparing: lower case, without accents, ß → ss, ä → a. */
export const normalizeName = (name: string) =>
  name
    .toLocaleLowerCase("de")
    .replaceAll("ß", "ss")
    .replace(/(?<=[aou])e/g, "") // "ae" → "a", as "ä" becomes "a" below
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim();

/** Edit distance of two strings (insertions, deletions, substitutions). */
export const editDistance = (a: string, b: string) => {
  let previous = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) {
    const current = [i];
    for (let j = 1; j <= b.length; j++) {
      current[j] = Math.min(
        previous[j] + 1,
        current[j - 1] + 1,
        previous[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1),
      );
    }
    previous = current;
  }
  return previous[b.length];
};

const fullName = (s: Pick<Rider, "firstname" | "lastname">) =>
  `${s.firstname} ${s.lastname}`;

/** How well a name matches: 0 exact, 1 similar, `null` not at all. */
const nameMatch = (wanted: string | null | undefined, name: string) => {
  if (!wanted) {
    return null;
  }
  const a = normalizeName(wanted);
  const b = normalizeName(name);
  if (a === b) {
    return 0;
  }
  // Up to two typos, but not for very short names.
  return Math.min(a.length, b.length) >= 6 && editDistance(a, b) <= 2
    ? 1
    : null;
};

/** Whether a rider waits for a partner of another club. */
export const isUnconfirmedPair = (s: Rider) => s.pair && !s.partner_id;

export interface PairCandidate {
  rider: Rider;
  /**
   * "mutual": both name each other exactly. "exact": the name matches one
   * way. "similar": the names are spelled a bit differently.
   */
  match: "mutual" | "exact" | "similar";
}

/** The unconfirmed riders of other clubs, best match first. */
export const pairCandidates = (
  rider: Rider,
  riders: Rider[],
): PairCandidate[] => {
  const ranked: (PairCandidate & { score: number })[] = [];
  for (const other of riders) {
    if (other.club_id === rider.club_id || !isUnconfirmedPair(other)) {
      continue;
    }
    const there = nameMatch(rider.partner_name, fullName(other));
    const back = nameMatch(other.partner_name, fullName(rider));
    if (there === null && back === null) {
      continue;
    }
    const match =
      there === 0 && back === 0
        ? "mutual"
        : there === 0 || back === 0
          ? "exact"
          : "similar";
    ranked.push({
      rider: other,
      match,
      score: (there ?? 2) + (back ?? 2),
    });
  }
  return ranked
    .sort((a, b) => a.score - b.score)
    .map(({ rider, match }) => ({ rider, match }));
};

export interface UnconfirmedPair {
  rider: Rider;
  /** The likely partner, if any; the other options in `candidates`. */
  partner: PairCandidate | null;
  candidates: PairCandidate[];
}

/**
 * Every unconfirmed pair rider, sorted by name. Riders who are each other's
 * best match are listed once, as one pair.
 */
export const unconfirmedPairs = (riders: Rider[]): UnconfirmedPair[] => {
  const waiting = riders
    .filter(isUnconfirmedPair)
    .sort((a, b) =>
      `${a.lastname} ${a.firstname}`.localeCompare(
        `${b.lastname} ${b.firstname}`,
        "de",
      ),
    );
  const listed = new Set<string>();
  const pairs: UnconfirmedPair[] = [];
  for (const rider of waiting) {
    if (listed.has(rider.id)) {
      continue;
    }
    const candidates = pairCandidates(rider, riders);
    const partner = candidates[0] ?? null;
    listed.add(rider.id);
    if (
      partner &&
      pairCandidates(partner.rider, riders)[0]?.rider.id === rider.id
    ) {
      listed.add(partner.rider.id);
    }
    pairs.push({ rider, partner, candidates });
  }
  return pairs;
};
