import assert from "node:assert/strict";
import { describe, test } from "vite-plus/test";
import {
  completePayload,
  conflictView,
  isInsecureBaseUrl,
  serverErrors,
  stepViews,
  submitLabel,
  tokenFromInput,
  validateForm,
} from "../src/ts/setup-wizard.ts";

const missing = { state: "missing" };
/** A status like a bare server: everything is missing. */
const bare = () => ({
  writes_config: true,
  config: "will_write",
  config_path: "/srv/cup/config.toml",
  steps: {
    base_url: missing,
    smtp: {
      state: "missing",
      fields: { server: "missing", user: "missing", password: "missing" },
    },
    secrets: {
      state: "missing",
      fields: {
        jwt_secret: "missing",
        reload_db_token: "missing",
        metrics_token: "missing",
      },
    },
    admin: missing,
  },
});

const form = (over = {}) => ({
  base_url: "https://cup.example.org",
  smtp: { server: "mail.example.org", user: "cup@example.org", password: "pw" },
  metrics: false,
  admin: {
    name: "Max Müller",
    email: "max@example.org",
    password: "Abcdef1!",
    password_repeat: "Abcdef1!",
  },
  ...over,
});

describe("token input", () => {
  test("takes a bare token, trimmed", () => {
    assert.equal(tokenFromInput("  abc_123-X \n"), "abc_123-X");
  });
  test("takes the token out of a pasted URL", () => {
    assert.equal(
      tokenFromInput("https://cup.example.org/setup?token=abc123"),
      "abc123",
    );
    assert.equal(tokenFromInput(" /setup?a=1&token=abc123 "), "abc123");
  });
  test("keeps text without a token parameter as it is", () => {
    assert.equal(
      tokenFromInput("https://cup.example.org/setup"),
      "https://cup.example.org/setup",
    );
  });
});

describe("http warning", () => {
  test("warns for http outside localhost", () => {
    assert.equal(isInsecureBaseUrl("http://cup.example.org"), true);
    assert.equal(isInsecureBaseUrl("http://192.168.1.5:3000"), true);
  });
  test("not for https or localhost", () => {
    assert.equal(isInsecureBaseUrl("https://cup.example.org"), false);
    assert.equal(isInsecureBaseUrl("http://localhost:3000"), false);
    assert.equal(isInsecureBaseUrl("http://127.0.0.1:3000"), false);
    assert.equal(isInsecureBaseUrl("http://[::1]:3000"), false);
    assert.equal(isInsecureBaseUrl("http://cup.localhost"), false);
  });
  test("not for something that isn't a URL", () => {
    assert.equal(isInsecureBaseUrl("cup"), false);
    assert.equal(isInsecureBaseUrl(""), false);
  });
});

describe("step views", () => {
  test("everything missing is actionable, secrets are generated", () => {
    const v = stepViews(bare());
    assert.deepEqual(
      v.steps.map((s) => [s.id, s.state]),
      [
        ["base_url", "missing"],
        ["smtp", "missing"],
        ["secrets", "missing"],
        ["admin", "missing"],
      ],
    );
    assert.equal(v.smtp.server.editable, true);
    assert.equal(v.secrets.jwt_secret.text, "wird zufällig erzeugt ✓");
    assert.equal(v.secrets.reload_db_token.text, "wird zufällig erzeugt ✓");
    assert.equal(v.metrics.editable, true);
    assert.deepEqual(v.notices, []);
  });
  test("environment steps are done and not editable, without values", () => {
    const s = bare();
    s.steps.smtp = {
      state: "env",
      fields: { server: "env", user: "env", password: "env" },
    };
    const v = stepViews(s);
    const smtp = v.steps.find((x) => x.id === "smtp");
    assert.equal(smtp.label, "SMTP: durch Umgebung gesetzt ✓");
    assert.equal(v.smtp.server.editable, false);
    assert.equal(v.smtp.password.text, "durch Umgebung gesetzt ✓");
    assert.equal(v.testMail.available, true);
  });
  test("a mixed smtp step edits only the missing fields", () => {
    const s = bare();
    s.steps.smtp = {
      state: "missing",
      fields: { server: "env", user: "missing", password: "done" },
    };
    const v = stepViews(s);
    assert.equal(v.smtp.server.editable, false);
    assert.equal(v.smtp.user.editable, true);
    assert.equal(v.smtp.password.editable, false);
    assert.equal(v.smtp.password.text, "bereits gesetzt ✓");
  });
  test("a stored user without a stored server can't be tested", () => {
    const s = bare();
    s.steps.smtp = {
      state: "missing",
      fields: { server: "missing", user: "done", password: "missing" },
    };
    const v = stepViews(s);
    assert.equal(v.testMail.available, false);
    assert.match(v.testMail.reason, /Server/);
  });
  test("not needed steps are listed as such", () => {
    const s = bare();
    s.writes_config = false;
    s.config = "not_needed";
    s.steps.base_url = { state: "not_needed" };
    s.steps.smtp = {
      state: "done",
      fields: { server: "done", user: "done", password: "done" },
    };
    s.steps.secrets = {
      state: "done",
      fields: {
        jwt_secret: "done",
        reload_db_token: "not_needed",
        metrics_token: "not_needed",
      },
    };
    const v = stepViews(s);
    assert.equal(
      v.steps.find((x) => x.id === "base_url").label,
      "Basis-URL: nicht nötig",
    );
    assert.equal(v.metrics.editable, false);
    assert.equal(v.metrics.text, "nicht nötig");
    assert.match(v.notices.join(" "), /nur der Admin/);
  });
  test("an existing incomplete config file is announced", () => {
    const s = bare();
    s.config = "exists_incomplete";
    const v = stepViews(s);
    assert.match(v.notices.join(" "), /\/srv\/cup\/config\.toml/);
  });
  test("an optional metrics token already set is not editable", () => {
    const s = bare();
    s.steps.secrets.fields.metrics_token = "env";
    const v = stepViews(s);
    assert.equal(v.metrics.editable, false);
    assert.equal(v.metrics.text, "durch Umgebung gesetzt ✓");
  });
});

describe("form validation", () => {
  test("a good form has no errors", () => {
    assert.deepEqual(validateForm(bare(), form()), {});
  });
  test("mirrors the server's admin rules", () => {
    const e = validateForm(
      bare(),
      form({
        admin: {
          name: " ",
          email: "nope",
          password: "abc",
          password_repeat: "abd",
        },
      }),
    );
    assert.ok(e["admin.name"]);
    assert.ok(e["admin.email"]);
    assert.ok(e["admin.password_repeat"]);
    assert.equal(e["admin.password"], undefined, "mismatch is reported once");
  });
  test("password needs 8 bytes and three categories", () => {
    const admin = (password) => ({
      name: "Max Müller",
      email: "max@example.org",
      password,
      password_repeat: password,
    });
    assert.match(
      validateForm(bare(), form({ admin: admin("Ab1!xyz") }))["admin.password"],
      /8 Zeichen/,
    );
    assert.match(
      validateForm(bare(), form({ admin: admin("abcdefgh") }))[
        "admin.password"
      ],
      /3 der folgenden/,
    );
    assert.equal(
      validateForm(bare(), form({ admin: admin("abcdefg1!") }))[
        "admin.password"
      ],
      undefined,
    );
    // Two umlauts are four bytes: seven characters are enough, like the server.
    assert.equal(
      validateForm(bare(), form({ admin: admin("äöabc1X") }))["admin.password"],
      undefined,
    );
  });
  test("base_url needs an http(s) URL", () => {
    assert.ok(
      validateForm(bare(), form({ base_url: "cup.example.org" }))["base_url"],
    );
    assert.ok(
      validateForm(bare(), form({ base_url: "ftp://cup.example.org" }))[
        "base_url"
      ],
    );
  });
  test("smtp needs server, an email as user, and a password", () => {
    const e = validateForm(
      bare(),
      form({ smtp: { server: "", user: "cup", password: "" } }),
    );
    assert.ok(e["smtp.server"]);
    assert.ok(e["smtp.user"]);
    assert.ok(e["smtp.password"]);
  });
  test("only what the status misses is validated", () => {
    const s = bare();
    s.steps.admin = { state: "done" };
    s.steps.base_url = { state: "env" };
    assert.deepEqual(
      validateForm(
        s,
        form({
          base_url: "",
          admin: { name: "", email: "", password: "", password_repeat: "" },
        }),
      ),
      {},
    );
  });
});

describe("completion payload", () => {
  test("sends everything the status misses", () => {
    const p = completePayload(bare(), form({ metrics: true }));
    assert.equal(p.base_url, "https://cup.example.org");
    assert.deepEqual(p.smtp, {
      server: "mail.example.org",
      user: "cup@example.org",
      password: "pw",
    });
    assert.equal(p.metrics, true);
    assert.equal(p.admin.email, "max@example.org");
  });
  test("leaves out what isn't needed, so the server doesn't reject it", () => {
    const s = bare();
    s.writes_config = false;
    s.config = "not_needed";
    s.steps.base_url = { state: "not_needed" };
    s.steps.smtp = {
      state: "done",
      fields: { server: "done", user: "done", password: "done" },
    };
    s.steps.secrets.fields.metrics_token = "not_needed";
    const p = completePayload(s, form({ metrics: true }));
    assert.equal(p.base_url, "");
    assert.deepEqual(p.smtp, { server: "", user: "", password: "" });
    assert.equal(p.metrics, false);
  });
  test("keeps the smtp password as typed, trims the rest", () => {
    const p = completePayload(
      bare(),
      form({
        base_url: " https://cup.example.org ",
        smtp: {
          server: " mail.example.org ",
          user: " cup@example.org ",
          password: " pw ",
        },
      }),
    );
    assert.equal(p.base_url, "https://cup.example.org");
    assert.equal(p.smtp.server, "mail.example.org");
    assert.equal(p.smtp.password, " pw ");
  });
});

describe("server errors", () => {
  test("go to their field, with the server text after a German lead", () => {
    const r = serverErrors([
      "admin.email: is already used by another user",
      "smtp.server: is required",
      "base_url: needs an http(s) URL",
    ]);
    assert.ok(r.fields["admin.email"].includes("bereits"));
    assert.ok(r.fields["smtp.server"]);
    assert.ok(r.fields["base_url"]);
    assert.deepEqual(r.general, []);
  });
  test("a password rule failure keeps the server's German text", () => {
    const r = serverErrors([
      "admin.password: Passwort muss mindestens 8 Zeichen haben.",
    ]);
    assert.equal(
      r.fields["admin.password"],
      "Passwort muss mindestens 8 Zeichen haben.",
    );
  });
  test("step level and unknown entries are general", () => {
    const r = serverErrors([
      "smtp: The SMTP server setting is invalid: bad: thing",
      "competition: file missing",
      "something: with a colon unknown",
      "no prefix at all",
    ]);
    assert.deepEqual(r.fields, {});
    assert.equal(r.general.length, 4);
    assert.ok(
      r.general[0].includes("The SMTP server setting is invalid: bad: thing"),
    );
    assert.ok(r.general[3].includes("no prefix at all"));
  });
});

describe("409 content", () => {
  const body = (over) => ({
    reason: "not_writable",
    config_path: "/srv/cup/config.toml",
    config_content: '[server]\nbase_url = "https://x"\n',
    partial: false,
    ...over,
  });
  test("a new file is saved at the path, then the server restarts", () => {
    const v = conflictView(body());
    assert.equal(v.path, "/srv/cup/config.toml");
    assert.equal(v.content, body().config_content);
    assert.equal(v.partial, false);
    assert.match(v.explanation, /config\.toml/);
    assert.match(v.explanation, /neu gestartet|neu starten/);
    assert.equal(v.filename, "config.toml");
  });
  test("an existing file only gets the missing lines", () => {
    const v = conflictView(body({ reason: "exists", partial: true }));
    assert.equal(v.partial, true);
    assert.match(v.explanation, /nur die fehlenden Zeilen/);
    assert.equal(v.filename, "config-fehlende-zeilen.toml");
  });
  test("a file that appeared meanwhile is not partial but exists", () => {
    const v = conflictView(body({ reason: "exists" }));
    assert.match(v.explanation, /existiert bereits/);
  });
});

describe("submit button", () => {
  test("says what saving without a good test means", () => {
    assert.equal(
      submitLabel({ smtpMissing: true, test: "ok" }),
      "Einrichtung abschließen",
    );
    assert.equal(
      submitLabel({ smtpMissing: false, test: "none" }),
      "Einrichtung abschließen",
    );
    assert.equal(
      submitLabel({ smtpMissing: true, test: "none" }),
      "Ohne Testmail speichern",
    );
    assert.equal(
      submitLabel({ smtpMissing: true, test: "failed" }),
      "Trotzdem speichern",
    );
    assert.equal(
      submitLabel({ smtpMissing: true, test: "sending" }),
      "Ohne Testmail speichern",
    );
  });
});
