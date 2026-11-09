import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import CupClubStepActs from "../src/ts/components/elements/club/cup-club-step-acts.ts";
import CupClubPortal from "../src/ts/components/elements/club/cup-club-portal.ts";
import CupCompetitionForm from "../src/ts/components/elements/cup-competition-form.ts";

function setFields(element, fields) {
  for (const [name, value] of Object.entries(fields)) {
    Object.defineProperty(element, name, {
      configurable: true,
      writable: true,
      value,
    });
  }
}

function templateText(value) {
  if (Array.isArray(value)) {
    return value.map(templateText).join("");
  }
  if (!value || typeof value !== "object" || !Array.isArray(value.strings)) {
    return "";
  }
  return value.strings.reduce(
    (text, part, index) => text + part + templateText(value.values[index]),
    "",
  );
}

function templateValues(value) {
  if (Array.isArray(value)) {
    return value.flatMap(templateValues);
  }
  if (!value || typeof value !== "object" || !Array.isArray(value.strings)) {
    return [value];
  }
  return value.values.flatMap(templateValues);
}

describe("moderation input", () => {
  test("club act details omit the moderation field when disabled", () => {
    const element = Object.create(CupClubStepActs.prototype);
    setFields(element, {
      moderationInputEnabled: false,
      competition: { features: { moderation_input: true } },
      drafts: new Map(),
    });
    const disabled = templateText(
      element.renderDetailsForm({
        id: "act-1",
        name: "Sternenstaub",
        description: "Mit Seil",
      }),
    );
    assert.doesNotMatch(disabled, /<textarea/);

    setFields(element, { moderationInputEnabled: true });
    const enabled = templateText(
      element.renderDetailsForm({
        id: "act-1",
        name: "Sternenstaub",
        description: "Mit Seil",
      }),
    );
    assert.match(enabled, /<textarea/);
  });

  test("club portal passes the resolved feature setting to act details", () => {
    const portal = Object.create(CupClubPortal.prototype);
    setFields(portal, {
      systemStatus: {
        moderation_input: false,
        jury_registration: true,
        competition: { features: { moderation_input: true } },
      },
      step: "kueren",
      admin: false,
      target: undefined,
    });

    const template = portal.renderStep({});
    const binding = template.strings.findIndex((part) =>
      part.endsWith(".moderationInputEnabled="),
    );
    assert.notEqual(binding, -1);
    assert.equal(template.values[binding], false);
  });

  test("Wettkampf settings expose the moderation feature override", () => {
    const element = Object.create(CupCompetitionForm.prototype);
    setFields(element, {
      colors: { primary: "#123456", secondary: "#654321" },
      defaults: { primary: "#123456", secondary: "#654321" },
      colorErrors: {},
      jury: false,
      referencePrefix: "",
      warnings: [],
      error: "",
      saved: false,
      busy: false,
    });
    const settings = {
      file_id_mismatch: null,
      photos: null,
      feature_defaults: { jury_registration: false, moderation_input: true },
      competition: {
        id: "test",
        name: "Test",
        location: "Teststadt",
        venue: null,
        contact_email: null,
        announcement_url: null,
        start_date: "2027-05-01",
        end_date: "2027-05-01",
        determining_day: "2027-06-01",
        registration_start: "2027-01-01T00:00:00Z",
        registration_end: "2027-02-01T00:00:00Z",
        music_upload_end: "2027-02-01T00:00:00Z",
        fee_per_start: 10,
        max_fee_per_rider: null,
        payment_deadline: null,
        payment_account_holder: null,
        payment_iban: null,
        payment_bic: null,
        payment_reference: null,
        photos_url: null,
        jury: null,
        features: { jury_registration: null, moderation_input: false },
        primary_color: null,
        secondary_color: null,
      },
    };

    const template = element.renderForm(settings);
    const markup = templateText(template);
    const values = templateValues(template);
    assert.match(markup, /name="feature_moderation_input"/);
    assert.match(markup, /<option value="on">An<\/option>/);
    assert.match(markup, /<option value="off">Aus<\/option>/);
    assert.ok(
      values.includes("off"),
      "the competition's disabled setting should be selected",
    );
  });
});
