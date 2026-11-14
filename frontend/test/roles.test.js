import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import { adminPages, canOpen, roleLock, withRole } from "../src/ts/roles.ts";

describe("withRole", () => {
  test("adds a role in the order of the role table", () => {
    assert.deepEqual(withRole(["music_check", "admin"], "treasurer", true), [
      "admin",
      "treasurer",
      "music_check",
    ]);
  });

  test("removes a role and ignores roles already set", () => {
    assert.deepEqual(withRole(["admin", "treasurer"], "treasurer", false), [
      "admin",
    ]);
    assert.deepEqual(withRole(["treasurer"], "treasurer", true), ["treasurer"]);
  });
});

describe("roleLock", () => {
  const me = { id: "me", roles: ["admin"] };
  const other = { id: "other", roles: ["admin"] };

  test("admins can't drop their own Admin role", () => {
    assert.match(roleLock(me, "admin", "me"), /eigene/);
    assert.equal(roleLock(me, "treasurer", "me"), null);
  });

  test("other admins' Admin role and granting Admin are never locked", () => {
    assert.equal(roleLock(other, "admin", "me"), null);
    assert.equal(roleLock({ id: "x", roles: [] }, "admin", "me"), null);
  });
});

describe("canOpen", () => {
  const can = (path, permissions) => canOpen(path, permissions);

  test("pages without a requirement are open to everyone", () => {
    assert.equal(can("/info-board", []), true);
  });

  test("the music check reaches the acts overview but not the planning", () => {
    const musicCheck = ["check_songs"];
    assert.equal(can("/admin-acts-overview", musicCheck), true);
    assert.equal(can("/admin-categories", musicCheck), false);
    assert.equal(can("/admin-structure", musicCheck), false);
    assert.equal(can("/admin-structure", ["plan_competition"]), true);
    assert.equal(can("/admin", musicCheck), false);
  });

  test("the check-in desk needs riders too, the music control can't", () => {
    const desk = [
      "view_clubs",
      "view_club_balances",
      "view_riders",
      "check_in",
      "view_medals",
    ];
    const music = ["check_in", "control_music", "view_moderation"];
    assert.equal(can("/admin-check-in", desk), true);
    assert.equal(can("/admin-medals", desk), true);
    assert.equal(can("/admin-check-in", music), false);
    assert.equal(can("/admin-music-control", music), true);
    assert.equal(can("/player", music), true);
    assert.equal(can("/admin-moderation", music), true);
    assert.equal(can("/admin-moderation", desk), false);
  });
});

describe("jury lead", () => {
  test("reaches the categories for the finals' placements", () => {
    const juryLead = [
      "view_clubs",
      "view_club_contacts",
      "manage_jury",
      "manage_results",
      "view_moderation",
      "view_internal_entries",
    ];
    assert.equal(canOpen("/admin-categories", juryLead), true);
    assert.equal(canOpen("/admin-timeplan", juryLead), false);
  });

  test("follows the event on the moderation screen and sees the medals", () => {
    const juryLead = [
      "manage_jury",
      "manage_results",
      "view_medals",
      "view_moderation",
    ];
    assert.equal(canOpen("/admin-moderation", juryLead), true);
    assert.equal(canOpen("/admin-medals", juryLead), true);
  });
});

describe("the backups page", () => {
  test("needs manage_backups, not manage_users", () => {
    assert.equal(canOpen("/admin-backups", ["manage_backups"]), true);
    assert.equal(canOpen("/admin-backups", ["manage_users"]), false);
    assert.equal(canOpen("/admin", ["manage_backups"]), false);
    assert.ok(adminPages(["manage_backups"]).includes("/admin-backups"));
  });
});

describe("adminPages", () => {
  test("lists the pages a treasurer may open", () => {
    const treasurer = [
      "manage_payments",
      "view_clubs",
      "view_club_balances",
      "view_club_contacts",
      "view_riders",
    ];
    assert.deepEqual(adminPages(treasurer), ["/admin-payments"]);
  });

  test("starts with the overview for admins", () => {
    const admin = [
      "manage_users",
      "plan_competition",
      "manage_registrations",
      "manage_payments",
      "view_clubs",
      "view_club_balances",
      "view_club_contacts",
      "view_riders",
      "manage_jury",
      "manage_results",
      "check_in",
      "view_medals",
      "control_music",
      "view_moderation",
      "check_songs",
    ];
    assert.equal(adminPages(admin)[0], "/admin");
    assert.ok(adminPages(admin).includes("/admin-judges"));
  });
});

describe("club mails", () => {
  test("whoever may mail clubs reaches the Rundmail page", () => {
    assert.equal(canOpen("/admin-mails", ["mail_clubs"]), true);
    assert.equal(canOpen("/admin-mails", ["view_clubs", "check_in"]), false);
  });
});
