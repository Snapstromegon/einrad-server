import { type schemas } from "./apiClient";
import { type Starts } from "./starts";
import { type ActType } from "./utils";

type Rider = schemas.Rider & Starts;
type Act = schemas.Act;

export interface RiderAdminEntry {
  rider: Rider;
  acts: Act[];
}

export interface RiderAdminClub {
  id: string;
  name: string;
  riders: RiderAdminEntry[];
}

export interface RiderAdminFilter {
  filter: string;
  search: string;
  onlyMissingCheckIn: boolean;
}

export interface RiderAdminMarker {
  key: string;
  act: Act | null;
  actType: ActType | null;
  status:
    | "categorized"
    | "uncategorized"
    | "withdrawn"
    | "pair-request"
    | "no-starts";
  isReplacement: boolean;
}

const compareText = (a: string, b: string) => a.localeCompare(b, "de");

export const groupRidersByClub = (
  riders: Rider[],
  acts: Act[],
): RiderAdminClub[] => {
  const actsByRider = new Map<string, Act[]>();
  for (const act of acts) {
    for (const participant of act.participants) {
      actsByRider.set(participant.id, [
        ...(actsByRider.get(participant.id) ?? []),
        act,
      ]);
    }
  }

  const clubs = new Map<string, RiderAdminClub>();
  for (const rider of riders) {
    let club = clubs.get(rider.club_id);
    if (!club) {
      club = { id: rider.club_id, name: rider.club_name, riders: [] };
      clubs.set(club.id, club);
    }
    club.riders.push({ rider, acts: actsByRider.get(rider.id) ?? [] });
  }

  return [...clubs.values()]
    .map((club) => ({
      ...club,
      riders: club.riders.sort(
        (a, b) =>
          compareText(a.rider.lastname, b.rider.lastname) ||
          compareText(a.rider.firstname, b.rider.firstname),
      ),
    }))
    .sort((a, b) => compareText(a.name, b.name));
};

export const riderAdminMarkers = (
  entry: RiderAdminEntry,
): RiderAdminMarker[] => {
  const markers = entry.acts.map((act): RiderAdminMarker => ({
    key: act.id,
    act,
    actType: act.act_type,
    status: act.withdrawn
      ? "withdrawn"
      : act.category
        ? "categorized"
        : "uncategorized",
    isReplacement:
      act.participants.find((participant) => participant.id === entry.rider.id)
        ?.is_replacement ?? false,
  }));
  const hasPairAct = entry.acts.some((act) => act.act_type === "pair");

  if (entry.rider.pair && !hasPairAct) {
    markers.push({
      key: "pair-request",
      act: null,
      actType: "pair",
      status: "pair-request",
      isReplacement: false,
    });
  } else if (markers.length === 0) {
    markers.push({
      key: "no-starts",
      act: null,
      actType: null,
      status: "no-starts",
      isReplacement: false,
    });
  }

  return markers;
};

export const riderAdminRiderMatches = (
  entry: RiderAdminEntry,
  filter: RiderAdminFilter,
  isCheckedIn: boolean,
) => {
  if (filter.onlyMissingCheckIn && isCheckedIn) {
    return false;
  }

  const markers = riderAdminMarkers(entry);
  if (
    filter.filter === "none" &&
    !markers.some((marker) => marker.status !== "categorized")
  ) {
    return false;
  }
  if (filter.filter.startsWith("cat:")) {
    const category = filter.filter.slice(4);
    if (!markers.some((marker) => marker.act?.category === category)) {
      return false;
    }
  }
  if (filter.filter.startsWith("type:")) {
    const actType = filter.filter.slice(5);
    if (!markers.some((marker) => marker.actType === actType)) {
      return false;
    }
  }

  const needle = filter.search.trim().toLocaleLowerCase("de");
  if (!needle) {
    return true;
  }
  const searchable = [
    `${entry.rider.firstname} ${entry.rider.lastname}`,
    entry.rider.club_name,
    ...markers.flatMap(({ act, actType }) => [
      act?.category ?? "",
      act?.group_name ?? "",
      act?.name ?? "",
      actType ?? "",
    ]),
  ];
  return searchable.some((text) =>
    text.toLocaleLowerCase("de").includes(needle),
  );
};
