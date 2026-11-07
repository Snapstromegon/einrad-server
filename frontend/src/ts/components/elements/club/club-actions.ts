// Changes the club portal makes. A rider's own data is saved with
// `edit_rider`; their single and pair with one start command each
// (`set_single`, `request_pair`, `link_pair`, `leave_pair`), and the server
// creates or deletes the acts.
import { ApiError, unwrap } from "../../../api";
import { client, type schemas } from "../../../apiClient";
import { type ClubGroup, type ClubRider, riderName } from "../../../club-data";
import type { Starts } from "../../../starts";

/**
 * The fields of a rider that an edit sends. Riders of other clubs come
 * with a birthdate only for who manages registrations.
 */
type RiderFields = Pick<ClubRider, "firstname" | "lastname" | "gender"> & {
  birthdate?: string | null;
};

/** A rider of the own club or (for admins) of any club, with their starts. */
export type RiderLike = RiderFields & Starts & { id: string; club_id?: string };

/** Saves a rider's own data with `changes`, keeping everything else. */
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
export const hasSingle = (rider: Starts) =>
  rider.single_female || rider.single_male;

/** Registers (or removes) a rider's single, by their gender. */
export const setSingle = async (rider: RiderLike, single: boolean) => {
  if (single && !rider.gender) {
    throw new Error(
      `Bitte gib zuerst das Geschlecht von ${riderName(rider)} an (Schritt FahrerInnen).`,
    );
  }
  await unwrap(client.setSingle({ body: { rider_id: rider.id, single } }));
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
 * Ends a rider's pair (for both riders, nobody is left registered for a pair
 * without partner) or pair request.
 */
export const leavePair = async (rider: RiderLike) => {
  await unwrap(
    rider.pair_act_id
      ? client.dissolvePair({ body: { act_id: rider.pair_act_id } })
      : client.leavePair({ body: { rider_id: rider.id } }),
  );
};

/**
 * Pairs two riders. A rider who had another partner leaves that pair; the
 * old partner waits for a new one. Clubs pair riders of their own club,
 * admins riders of any clubs.
 */
export const linkPartners = async (a: RiderLike, b: RiderLike) => {
  if (a.partner_id === b.id) {
    return;
  }
  for (const rider of [a, b]) {
    if (rider.partner_id) {
      await unwrap(client.leavePair({ body: { rider_id: rider.id } }));
    }
  }
  await unwrap(client.linkPair({ body: { rider_id: a.id, partner_id: b.id } }));
};

/**
 * Registers a pair with a partner of another club (or one not registered
 * yet). The organizer links the partner later; until then the pair request
 * is pending.
 */
export const pairWithOtherClub = async (
  rider: RiderLike,
  partnerName: string,
) => {
  if (rider.partner_id) {
    // The old partner waits for a new one.
    await unwrap(client.leavePair({ body: { rider_id: rider.id } }));
  }
  await requestPair(rider, partnerName);
};

/** Asks for a pair partner by name, or changes the name asked for. */
export const requestPair = async (rider: RiderLike, partnerName: string) => {
  await unwrap(
    client.requestPair({
      body: { rider_id: rider.id, partner_name: partnerName.trim() || null },
    }),
  );
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
