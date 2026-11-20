// Feedback on the registration system itself, not on an event: it goes to the
// system's developer, while event questions go to the competition's contact.
// Built in, since it belongs to the software, not to an event.

/** Where ideas and bug reports for the registration system go. */
export const FEEDBACK_EMAIL = "raphael@hoeser.info";

/** The system's issue tracker, for feedback from people with GitHub. */
export const FEEDBACK_ISSUES_URL =
  "https://github.com/Snapstromegon/einrad-server/issues";

/**
 * A mail link to the developer with a fixed subject (so the mails can be
 * filtered) and the page it was sent from.
 */
export const feedbackMailto = (page: string): string =>
  `mailto:${FEEDBACK_EMAIL}` +
  `?subject=${encodeURIComponent("[Anmeldesystem] Feedback")}` +
  `&body=${encodeURIComponent(`Seite: ${page}\n\n`)}`;
