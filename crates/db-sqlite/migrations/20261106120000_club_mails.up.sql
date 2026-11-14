-- Club mails (Rundmail, #257): every mail the organizer team sent to clubs,
-- as written, and per recipient as rendered and whether it went out.
CREATE TABLE "club_mails" (
  "id" BLOB PRIMARY KEY,
  "sent_at" DATETIME NOT NULL,
  -- No foreign key: the log outlives its sender.
  "user_id" BLOB,
  "user_name" TEXT NOT NULL,
  -- The templates as written.
  "subject" TEXT NOT NULL,
  "body" TEXT NOT NULL,
  "reply_to" TEXT
);

-- One mail as it went out: to a club's contact and representative, or to a
-- typed-in address.
CREATE TABLE "club_mail_messages" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "mail_id" BLOB NOT NULL REFERENCES "club_mails" ("id"),
  -- The club whose data it was rendered with, and its name at the time; NULL
  -- for a typed-in address without a club. No foreign key, like the mail.
  "club_id" BLOB,
  "club_name" TEXT,
  -- The addresses it went to, as a JSON array.
  "addresses" TEXT NOT NULL,
  "subject" TEXT NOT NULL,
  "body" TEXT NOT NULL,
  -- 'pending' until the mail server took it or refused it.
  "status" TEXT NOT NULL DEFAULT 'pending' CHECK ("status" IN ('pending', 'sent', 'failed')),
  "error" TEXT
);

CREATE INDEX "club_mail_messages_mail_id" ON "club_mail_messages" ("mail_id");
