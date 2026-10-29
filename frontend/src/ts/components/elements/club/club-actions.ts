// Changes the club portal makes. Singles and pairs aren't acts of their own in
// the API: they are flags of a rider (`single_male`, `single_female`, `pair`
// with `partner_name`), and the server creates or deletes the act. These
// helpers turn "add a single", "pair two riders" etc. into those edits.
import { ApiError, unwrap } from "../../../api";
import { client, type schemas } from "../../../apiClient";
import { type ClubGroup, type ClubRider, riderName } from "../../../club-data";

/**
 * The fields of a rider that an edit sends. Riders of other clubs come
 * with a birthdate only for who manages registrations.
 */
type RiderFields = Pick<
  ClubRider,
  | "firstname"
  | "lastname"
  | "gender"
  | "single_male"
  | "single_female"
  | "pair"
  | "partner_name"
  | "partner_id"
> & { birthdate?: string | null };

/** A rider of the own club or (for admins) of any club. */
export type RiderLike = RiderFields & { id: string };

/** Saves a rider with `changes`, keeping everything else as it is. */
export const editRider = async (
  rider: RiderLike,
  changes: Partial<RiderFields>,
) => {
  const next = { ...rider, ...changes };
  if (!next.birthdate) {
    throw new Error(
      `Das Geburtsdatum von ${next.firstname} ist nicht bekannt.`,
    );
  }
  await unwrap(
    client.editRider({
      body: {
        rider_id: rider.id,
        firstname: next.firstname.trim(),
        lastname: next.lastname.trim(),
        birthdate: new Date(next.birthdate).toISOString(),
        gender: next.gender ?? null,
        single_male: next.single_male,
        single_female: next.single_female,
        pair: next.pair,
        partner_name: next.pair ? next.partner_name || null : null,
        partner_id: next.pair ? next.partner_id || null : null,
      },
    }),
  );
};

export const addRider = async (
  clubId: string,
  firstname: string,
  lastname: string,
  birthdate: Date,
  gender: Gender,
) => {
  await unwrap(
    client.addRider({
      body: {
        club_id: clubId,
        firstname: firstname.trim(),
        lastname: lastname.trim(),
        birthdate: birthdate.toISOString(),
        gender,
        single_male: false,
        single_female: false,
        pair: false,
        partner_name: null,
        partner_id: null,
      },
    }),
  );
};

export const deleteRider = async (rider: ClubRider) => {
  await unwrap(
    client.deleteRider({
      body: { rider_id: rider.id },
    }),
  );
};

export type Gender = schemas.Gender;

export const genderLabels: Record<Gender, string> = {
  female: "weiblich",
  male: "männlich",
};

/** Whether the rider rides a single. */
export const hasSingle = (rider: RiderLike) =>
  rider.single_female || rider.single_male;

/**
 * The single flags for a rider of `gender`: a single counts as Einzel
 * weiblich or männlich by the rider's gender.
 */
export const singleFlags = (
  single: boolean,
  gender: Gender | null | undefined,
) => ({
  single_female: single && gender === "female",
  single_male: single && gender === "male",
});

/** Registers (or removes) a rider's single, by their gender. */
export const setSingle = (rider: RiderLike, single: boolean) => {
  if (single && !rider.gender) {
    throw new Error(
      `Bitte gib zuerst das Geschlecht von ${riderName(rider)} an (Schritt FahrerInnen).`,
    );
  }
  return editRider(rider, singleFlags(single, rider.gender));
};

/**
 * Gives a single or pair act to another rider of the club; the act keeps its
 * title, description and music.
 */
export const replaceRider = async (
  actId: string,
  riderId: string,
  newRiderId: string,
) => {
  await unwrap(
    client.replaceActRider({
      body: {
        act_id: actId,
        rider_id: riderId,
        new_rider_id: newRiderId,
      },
    }),
  );
};

/**
 * Ends a rider's pair. A partner of the own club leaves the pair too, so
 * nobody is left registered for a pair without partner.
 */
export const leavePair = async (rider: RiderLike, riders: RiderLike[]) => {
  const partner = riders.find((s) => s.id === rider.partner_id);
  await editRider(rider, {
    pair: false,
    partner_name: null,
    partner_id: null,
  });
  if (partner?.pair) {
    // The server already unlinked the partner; only the flag is left.
    await editRider(partner, {
      pair: false,
      partner_name: null,
      partner_id: null,
    });
  }
};

/**
 * Pairs two riders of the own club. The server links partners by name: the
 * second rider is marked for a pair with the first one's name, then the
 * first one names the second, which links both and creates the act.
 */
export const pairRiders = async (
  a: RiderLike,
  b: RiderLike,
  riders: RiderLike[],
) => {
  if (a.partner_id === b.id) {
    return;
  }
  for (const rider of [a, b]) {
    if (rider.pair) {
      await leavePair(rider, riders);
    }
  }
  // Fresh copies: leaving a pair changed the flags.
  const plain = (s: RiderLike): RiderLike => ({
    ...s,
    pair: false,
    partner_id: null,
    partner_name: null,
  });
  await editRider(plain(b), { pair: true, partner_name: riderName(a) });
  await editRider(plain(a), { pair: true, partner_name: riderName(b) });
};

/**
 * Registers a pair with a partner of another club. The organizer links the
 * partner later; until then the pair is pending.
 */
export const pairWithOtherClub = async (
  rider: RiderLike,
  partnerName: string,
  riders: RiderLike[],
) => {
  if (rider.pair && rider.partner_id) {
    await leavePair(rider, riders);
  }
  await editRider(
    { ...rider, pair: false, partner_id: null, partner_name: null },
    { pair: true, partner_name: partnerName.trim() },
  );
};

/**
 * Pairs two riders of any clubs (admins only for different clubs). Riders of
 * one club are linked by name; across clubs the explicit partner is set,
 * which the server only accepts from admins.
 */
export const linkPartners = async (
  a: RiderLike & { club_id: string },
  b: RiderLike & { club_id: string },
  riders: RiderLike[],
) => {
  if (a.club_id === b.club_id) {
    return pairRiders(a, b, riders);
  }
  if (a.partner_id === b.id) {
    return;
  }
  for (const rider of [a, b]) {
    if (rider.partner_id) {
      await leavePair(rider, riders);
    }
  }
  const named = (s: RiderLike, partner: RiderLike): RiderLike => ({
    ...s,
    pair: true,
    partner_id: null,
    partner_name: riderName(partner),
  });
  await editRider(named(b, a), {});
  await editRider(named(a, b), {});
  // The partner name is unchanged now, so the server takes the partner.
  await editRider(named(a, b), { partner_id: b.id });
};

export interface GroupMember {
  rider_id: string;
  is_replacement: boolean;
}

/** The club's own riders of a group. */
export const ownGroupMembers = (
  group: Pick<ClubGroup, "members">,
  clubId: string,
): GroupMember[] =>
  group.members
    .filter((m) => m.club_id === clubId)
    .map((m) => ({
      rider_id: m.rider_id,
      is_replacement: m.is_replacement,
    }));

/**
 * Sets the club's own riders of a group. Returns the server's warning, e.g.
 * when a rider is now in two groups of the same type.
 */
export const setGroupMembers = async (
  clubId: string,
  groupId: string,
  members: GroupMember[],
) => {
  const { warning } = await unwrap(
    client.setClubGroupMembers({
      body: { club_id: clubId, act_id: groupId, members },
    }),
  );
  return warning ?? null;
};

/** The name as the backend stores it: trimmed, inner whitespace collapsed. */
export const normalizeGroupName = (name: string) =>
  name.trim().replace(/\s+/g, " ");

export const sameGroupName = (a: string, b: string) =>
  normalizeGroupName(a).toLocaleLowerCase("de") ===
  normalizeGroupName(b).toLocaleLowerCase("de");

/** The server accepts song files up to 10 MiB. */
export const MAX_SONG_SIZE = 10 * 1024 * 1024;

/**
 * Throws with a German message if the server won't take `file` as music, so
 * it can be checked before an act is created.
 */
export const checkSong = (file: File) => {
  if (!file.name.toLowerCase().endsWith(".mp3")) {
    throw new Error(
      `„${file.name}“ ist keine mp3-Datei. Bitte lade die Musik als mp3 hoch.`,
    );
  }
  if (file.size > MAX_SONG_SIZE) {
    throw new Error(tooLarge(file));
  }
};

const tooLarge = (file: File) =>
  `„${file.name}“ ist zu groß. Die Musik darf höchstens 10 MB groß sein.`;

/**
 * Uploads an act's music. Throws with a German message for files the
 * server won't take.
 */
export const uploadSong = async (actId: string, file: File) => {
  checkSong(file);
  const data = new FormData();
  data.append("file", file);
  // The generated types don't describe the multipart body and its query.
  try {
    await unwrap(
      client.saveActSong({
        query: { act_id: actId } as never,
        body: data as never,
      }),
    );
  } catch (error) {
    switch (error instanceof ApiError ? error.status : undefined) {
      case 413:
        throw new Error(tooLarge(file), { cause: error });
      case 400:
        throw new Error(
          `„${file.name}“ konnte nicht hochgeladen werden. Bitte lade die Musik als mp3 hoch.`,
          { cause: error },
        );
      case 403:
        throw new Error(
          "Der Musik-Meldeschluss ist vorbei. Musikänderungen sind nicht mehr möglich.",
          { cause: error },
        );
    }
    throw error;
  }
};

export const editAct = async (
  actId: string,
  name: string,
  description: string | null,
) => {
  await unwrap(
    client.editClubAct({
      body: { id: actId, name: name.trim(), description },
    }),
  );
};

/**
 * The act of a rider's single or pair. The server creates it when the
 * rider's flags change, without returning it.
 */
export const findActOf = async (
  clubId: string,
  type: "single" | "pair",
  riderId: string,
) => {
  const acts = await unwrap(
    client.listClubActs({
      query: { club_id: clubId },
    }),
  );
  return acts.find(
    (act) =>
      act.act_type === type && act.participants.some((p) => p.id === riderId),
  )?.id;
};
