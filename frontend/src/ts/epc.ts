/**
 * EPC QR code ("GiroCode") payloads for SEPA credit transfers, following
 * EPC069-12 v3.0. Banking apps fill in a transfer when they scan one.
 */

/** The EPC limit for the whole payload, in UTF-8 bytes. */
export const MAX_PAYLOAD_BYTES = 331;
const MAX_NAME = 70;
const MAX_TEXT = 140;

/**
 * NFC-normalizes `value`, turns line breaks and other control characters into
 * spaces (a line break would shift every field after it), collapses
 * whitespace and cuts it to `max` code points.
 */
const clean = (value: string, max: number) =>
  Array.from(
    value
      .normalize("NFC")
      .replace(/\p{Cc}/gu, " ")
      .replace(/\s+/g, " ")
      .trim(),
  )
    .slice(0, max)
    .join("")
    .trim();

/** An IBAN without spaces, in upper case. */
export const normalizeIban = (iban: string) =>
  iban.replace(/\s+/g, "").toUpperCase();

/** Whether `iban` has the IBAN format and passes the mod-97 check. */
export const isValidIban = (iban: string) => {
  const normalized = normalizeIban(iban);
  if (!/^[A-Z]{2}\d{2}[A-Z0-9]{11,30}$/.test(normalized)) {
    return false;
  }
  const rearranged = normalized.slice(4) + normalized.slice(0, 4);
  let remainder = 0;
  for (const char of rearranged) {
    const value = Number.parseInt(char, 36);
    remainder = (remainder * (value >= 10 ? 100 : 10) + value) % 97;
  }
  return remainder === 1;
};

/**
 * The transfer reference of a club, e.g. "Startgeld: SSV Nümbrecht
 * (EV-K7Q3)". The QR code adds "QR" to the code, so payments made with it
 * can be recognized. The club name is shortened if the reference would get
 * longer than the 140 characters a transfer allows.
 */
export const paymentReference = (
  prefix: string | null | undefined,
  clubName: string,
  code: string,
  { qr = false } = {},
) => {
  const head = prefix ? `${clean(prefix, MAX_TEXT)}: ` : "";
  const tail = ` (${code}${qr ? " QR" : ""})`;
  const room = MAX_TEXT - Array.from(head + tail).length;
  return `${head}${clean(clubName, Math.max(room, 0))}${tail}`;
};

export interface EpcTransfer {
  /** Account holder; must match the account exactly (Verification of Payee). */
  name: string;
  iban: string;
  bic?: string | null;
  /** In euros. */
  amount: number;
  /** Unstructured remittance information (Verwendungszweck). */
  text: string;
}

const utf8Length = (value: string) => new TextEncoder().encode(value).length;

/**
 * Builds the payload, or returns null if no valid transfer can be built: an
 * invalid IBAN, an empty name, or an amount outside 0.01 … 999999999.99 €.
 * The text is shortened until the payload fits into 331 bytes.
 */
export const buildEpcPayload = ({
  name,
  iban,
  bic,
  amount,
  text,
}: EpcTransfer): string | null => {
  const cents = Math.round(amount * 100);
  const beneficiary = clean(name, MAX_NAME);
  if (!isValidIban(iban) || !beneficiary || cents < 1 || cents > 99999999999) {
    return null;
  }
  // Version 002 allows leaving out the BIC; a malformed one is left out.
  const normalizedBic = (bic ?? "").replace(/\s+/g, "").toUpperCase();
  const validBic = /^[A-Z0-9]{8}([A-Z0-9]{3})?$/.test(normalizedBic)
    ? normalizedBic
    : "";
  const head = [
    "BCD",
    "002",
    "1",
    "SCT",
    validBic,
    beneficiary,
    normalizeIban(iban),
    `EUR${(cents / 100).toFixed(2)}`,
    // Purpose code and structured reference stay empty.
    "",
    "",
  ].join("\n");
  let remittance = Array.from(clean(text, MAX_TEXT));
  while (
    remittance.length > 0 &&
    utf8Length(`${head}\n${remittance.join("")}`) > MAX_PAYLOAD_BYTES
  ) {
    remittance = remittance.slice(0, -1);
  }
  return `${head}\n${remittance.join("").trim()}`;
};
