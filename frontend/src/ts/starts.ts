// A rider's single and pair starts, read from the acts and the pair requests:
// the API's riders don't carry them. Only type imports, so the unit tests can
// import it without a DOM.
import type { schemas } from "./apiClient";

export type PairRequest = schemas.PairRequest;

/** An act's rider; club acts name the fields `rider_firstname` etc. */
type Participant = { id: string; club_name: string } & (
  | { firstname: string; lastname: string }
  | { rider_firstname: string; rider_lastname: string }
);

const participantName = (p: Participant) =>
  "firstname" in p
    ? `${p.firstname} ${p.lastname}`
    : `${p.rider_firstname} ${p.rider_lastname}`;

/** What the starts need of an act; club acts and all acts both have it. */
export interface StartAct {
  id: string;
  act_type: schemas.ActType;
  participants: Participant[];
}

export interface Starts {
  /** Rides a single, Einzel weiblich or männlich by the gender. */
  single_male: boolean;
  single_female: boolean;
  /** Rides a pair act or waits for a partner (a pair request). */
  pair: boolean;
  /** The pair act and its partner. */
  pair_act_id: string | null;
  partner_id: string | null;
  /** The partner the club asked for, or the pair act's partner. */
  partner_name: string | null;
  /** The pair act's partner, who may ride for another club. */
  resolved_partner_name: string | null;
  resolved_partner_club: string | null;
}

/** The riders with their starts. */
export const withStarts = <
  R extends { id: string; gender?: schemas.Gender | null },
>(
  riders: R[],
  acts: StartAct[],
  requests: PairRequest[],
): (R & Starts)[] => {
  const singles = new Set<string>();
  const partners = new Map<string, Participant | null>();
  const pairActs = new Map<string, string>();
  for (const act of acts) {
    if (act.act_type === "single") {
      for (const p of act.participants) {
        singles.add(p.id);
      }
    } else if (act.act_type === "pair") {
      for (const p of act.participants) {
        pairActs.set(p.id, act.id);
        partners.set(
          p.id,
          act.participants.find((other) => other.id !== p.id) ?? null,
        );
      }
    }
  }
  const requested = new Map(requests.map((r) => [r.rider_id, r]));
  return riders.map((rider) => {
    const single = singles.has(rider.id);
    const partner = partners.get(rider.id) ?? null;
    const request = requested.get(rider.id);
    const partnerName = partner ? participantName(partner) : null;
    return {
      ...rider,
      single_male: single && rider.gender === "male",
      single_female: single && rider.gender === "female",
      pair: partners.has(rider.id) || !!request,
      pair_act_id: pairActs.get(rider.id) ?? null,
      partner_id: partner?.id ?? null,
      partner_name: request ? (request.partner_name ?? null) : partnerName,
      resolved_partner_name: partnerName,
      resolved_partner_club: partner?.club_name ?? null,
    };
  });
};
