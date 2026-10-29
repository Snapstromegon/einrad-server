// The riders the pickers of the club portal offer: the club's own and, for
// admins, those of all other clubs.
import type { AnyRider, ClubData, ClubRider } from "../../../club-data";
import type { PickerOption } from "../cup-rider-picker";

/** A rider of any club, with what the pickers need. */
export type Rider = Omit<
  ClubRider,
  "resolved_partner_club" | "resolved_partner_name"
> & {
  club_name: string;
};

const byFirstname = (a: Rider, b: Rider) =>
  a.firstname.localeCompare(b.firstname, "de") ||
  a.lastname.localeCompare(b.lastname, "de");

/**
 * The club's riders and, for admins, everyone else's: first the club's, then
 * the others, each by first name.
 */
export const ridersOf = (data: ClubData, admin: boolean): Rider[] => {
  const own: Rider[] = data.riders.map((s) => ({
    ...s,
    club_name: data.club.name,
  }));
  own.sort(byFirstname);
  if (!admin || !data.allRiders) {
    return own;
  }
  const others = data.allRiders
    .filter((s: AnyRider) => s.club_id !== data.club.id)
    .map((s): Rider => ({
      id: s.id,
      club_id: s.club_id,
      club_name: s.club_name,
      firstname: s.firstname,
      lastname: s.lastname,
      // Only for who manages registrations, like this list.
      birthdate: s.birthdate ?? "",
      gender: s.gender,
      single_male: s.single_male,
      single_female: s.single_female,
      pair: s.pair,
      partner_id: s.partner_id,
      partner_name: s.partner_name,
      group_count: s.group_count,
      waived_count: s.waived_count,
      fee_waived: s.fee_waived,
    }))
    .sort(byFirstname);
  return [...own, ...others];
};

/**
 * Picker options for `riders`, in their order (the club's own first). When
 * other clubs' riders are among them, the club's are listed under its name
 * and the others under "Andere Vereine", with their club.
 */
export const riderOptions = (
  data: ClubData,
  riders: Rider[],
  detail: (rider: Rider) => string = () => "",
): PickerOption[] => {
  const mixed = riders.some((r) => r.club_id !== data.club.id);
  return riders.map((r) => {
    const own = r.club_id === data.club.id;
    const extra = detail(r);
    return {
      value: r.id,
      label: `${r.firstname} ${r.lastname}`,
      detail: [own ? "" : `(${r.club_name})`, extra].filter(Boolean).join(" "),
      group: mixed ? (own ? data.club.name : "Andere Vereine") : undefined,
    };
  });
};
