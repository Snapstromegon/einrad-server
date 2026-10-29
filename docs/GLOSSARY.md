# Unicycle freestyle competition registration

The registration and event-day tool for one unicycle freestyle competition (here the Landesmeisterschaft NRW / NI / HB): clubs register their riders, acts and judges; the organizer plans the timeplan and the jury and runs the event day. Terms are English; the German word clubs and the Ausschreibung use is in brackets.

## Language

### The event

**Competition** (Wettkampf):
The one event this tool runs, with its dates, deadlines, fees and rules.
_Avoid_: Event, Cup, Meisterschaft (as a term in the tool)

**Competition file**:
The file that imports a competition with its categories, finals, timeplan and jury categories; applied only when the database has no competition yet, or on purpose as a replace. The database stays the source of the competition (ADR 0005).
_Avoid_: Config (for the competition)

**Remap file**:
The list of judge registrations a replace dropped because their jury category has no namesake in the new competition file, with an empty jury category to fill in.

**Determining day** (Stichtag):
The day ages are counted on for categories: 1 June of the season, for every act type.
_Avoid_: Age day, reference date

**Registration** (Anmeldung):
The period from Meldestart to Meldeschluss in which clubs register riders, acts and judges themselves.
_Avoid_: Sign-up, Meldung

**Meldeschluss**:
The end of the registration; afterwards only the organizer changes registrations.
_Avoid_: Deadline (without saying which)

**Judge registration deadline** (Ende der Jury-Anmeldung):
The end of the judge registration; the Meldeschluss unless the competition sets its own.

**Music upload deadline** (Musik-Upload bis):
The last day clubs can upload their acts' music.

**Organizer** (Veranstalter, Ausrichter):
Whoever runs the competition, here the host club; acts in the tool through its organizer team.
_Avoid_: Admin (for the organizer itself), host

**Jury lead** (Juryleitung):
The team that plans the jury, leads it at the event and decides objections.
_Avoid_: Chief judge, Oberschiedsrichter

### Access

**Role** (Rolle):
A fixed set of things a user may do on the organizer side, granted to a user; a user can hold several.
_Avoid_: Permission (for the set), Recht

**Admin**:
The role that may do everything, including granting roles.
_Avoid_: Superuser, Orga-Leitung

**Organizer team** (Orga-Team):
Every user holding at least one role.
_Avoid_: Staff, Helfer (as a term in the tool)

### Clubs and riders

**Club** (Verein):
A club that registers riders, acts and judges and pays for them.
_Avoid_: Team, Verband

**Club contact** (VereinsansprechpartnerIn, Kontaktperson):
The person whose account registered the club and manages its registration.
_Avoid_: Owner, Trainer

**Club representative** (Vereinsverantwortliche/r):
The person who files objections for the club; the club contact unless the club names someone else.
_Avoid_: Contact, Verantwortlicher (without "Vereins-")

**Club code** (Vereinscode):
A short code identifying the club in bank transfer references, unchanged when the club is renamed.

**Rider** (FahrerIn):
A person of a club who rides in acts.
_Avoid_: Starter, StarterIn, TeilnehmerIn, athlete

### Acts

**Act** (Kür):
One performance with music: a single, a pair, a small group or a large group.
_Avoid_: Routine, performance, Start

**Single** (Einzelkür):
An act of one rider, ranked separately for women and men (Einzel weiblich / Einzel männlich).

**Pair** (Paarkür):
An act of two riders; its age is the older rider's. Pairs across clubs are linked by the organizer.

**Pair request** (Paaranfrage):
A rider's wish to ride a pair with a named partner who isn't linked yet; within a club it's linked once the partner is registered, across clubs the organizer confirms it. It pays a start but doesn't count towards the jury obligation until it's a pair.
_Avoid_: Unconfirmed pair, Partnerwunsch

**Small group** (Kleingruppe):
A group act of up to 8 regular riders; the club chooses the type, the size only warns.

**Large group** (Großgruppe):
A group act of 9 or more regular riders.

**Regular rider** (FahrerIn einer Gruppe):
A rider who performs in a group; groups need at least 3.

**Replacement rider** (Ersatzfahrer/in):
A rider who stands in for a group if needed; counts for the group's age and pays a start, but gets no medal and doesn't count towards its size.
_Avoid_: Substitute, Reserve

**Owner club** (Verein der Gruppe):
The club that created a group act; riders of other clubs can join it, and it counts for the owner club's jury obligation.

**Incomplete group** (unvollständige Gruppe):
A group with fewer than 3 regular riders; it stays registered but has no category until it's complete.

**Start**:
One rider's place in one act; start fees are counted per start, at most a cap per rider.
_Avoid_: Act, Kür (for this meaning)

**Withdrawal** (Abmeldung):
Taking an act back after the Meldeschluss; it leaves the timeplan, but its start fee stays unless the fee is waived.
_Avoid_: Cancellation, deletion

### Categories and finals

**Category** (Kategorie):
A discipline and an age class in which acts are ranked against each other, e.g. "Einzel weiblich U13" or "Kleingruppe Expert".
_Avoid_: Altersklasse, Wertungsklasse, class

**Age class** (Altersklasse):
The age part of a category, e.g. U13 or 70+; groups only have Junior Expert (under 15) and Expert.
_Avoid_: Category (for the age part only)

**Heat** (Vorlauf):
A group of age classes whose acts are ranked together to find the finalists: Junior Expert (U11–U15), Expert 1 (U17–U21) and Expert 2 (U23 and older).
_Avoid_: Block, Vorlaufblock, round

**Final** (Finale):
The final round of singles or pairs, with reserved finalist slots for the best of its heats; group categories have no separate final, they are the final.
_Avoid_: Finalrunde

**Finalist slot** (Finalplatz):
A reserved start in a final, e.g. "1. Finalist (U11–U15)", filled with an act once the heats are ranked.

### Timeplan and event day

**Timeplan** (Zeitplan):
The order of everything on the competition days, from which the expected times are predicted.
_Avoid_: Schedule, Ablaufplan

**Block** (Startblock):
A timeplan entry in which one category or one final starts.
_Avoid_: Heat, Vorlauf, Session

**Custom entry** (eigener Eintrag):
A timeplan entry without acts, e.g. the opening, a lunch break or an award ceremony.

**Internal timeplan entry** (Interner Zeitplaneintrag):
A custom entry only the organizer team sees, e.g. a jury meeting; it takes its time in the timeplan, but public screens show a break.
_Avoid_: Hidden entry, private entry

**Break** (Pause):
A pause inside a block, with a planned length.

**Warm-up** (Einfahrzeit):
Time before a block for its riders to warm up; small consecutive categories share one.

**Act time** (Kürzeit):
The maximum length of an act's performance in a category.

**Judging time** (Wertungszeit):
The time the jury gets after each act before the next one starts.

**Delay** (Verzug):
How far the live event runs behind or ahead of the timeplan.

**Check-in**:
A rider confirming their presence at the event desk; advisory, it doesn't remove anyone.

**Startlist** (Startliste):
The published order of acts with their expected start times.

**Info board** (Info-Board):
The public screen in the hall showing what's running now and next.

**Moderation screen** (Moderation):
The screen for the announcer with the current and next act and the delay.

### Money

**Start fee** (Startgeld):
The fee per start, at most a cap per rider.
_Avoid_: Meldegebühr, entry fee

**Jury fee** (Jurygebühr):
The fee per missing jury deployment, paid with the start fees.
_Avoid_: Strafgebühr, penalty

**Amount due** (Betrag, Soll):
What a club owes: its start fees plus its jury fee, less its fee waivers.
_Avoid_: Invoice, Rechnung

**Booking** (Buchung):
A movement of money on the event account as recorded by the organizer, with an amount (negative for money going out), a date and a note; it is a payment, unassigned or ignored.
_Avoid_: Transaction, Überweisung (for the record)

**Payment** (Zahlung):
A booking assigned to a club: money the club paid, or a refund to it when negative.
_Avoid_: Booking (for one that isn't assigned)

**Refund** (Rückzahlung):
A negative payment: money the organizer paid back to a club.

**Unassigned booking** (nicht zugeordnete Buchung):
A booking not assigned to a club yet, e.g. a transfer whose reference names no club.
_Avoid_: Unknown payment, Fehlbuchung

**Ignored booking** (ignorierte Buchung):
A booking that has nothing to do with the event, e.g. account fees; it counts for no club.

**Paid** (Gezahlt):
The sum of a club's payments.

**Fee waiver** (Gebühr erlassen):
The organizer dropping fees, always in full and with a waiver reason: for a club all its fees (start fees and jury fee), for a rider all their start fees, for an act its start for every rider. It lowers the amount due; it is never a booking.
_Avoid_: Rabatt, discount, Gutschrift (for a waiver)

**Waiver reason** (Grund):
Why a fee was waived, e.g. "Veranstalter", "befreundeter Verein", "Attest"; only the organizer team sees it.

### Jury

**Judge** (JurorIn):
A person a club registers to judge, or to count dismounts.
_Avoid_: Juror, Kampfrichter, Wertungsrichter

**Trained judge** (geschulte/r JurorIn):
A judge trained per the IUF Rulebook; only trained judges judge Technik or Performance, and they must be at least 15 on the first competition day.
_Avoid_: Licensed judge (there's no judge register yet)

**Dismount counter** (AbstiegszählerIn):
An untrained judge who only counts dismounts and doesn't count towards the jury obligation.
_Avoid_: Hospitation, helper

**Task** (Aufgabe):
What a judge does in a jury category: Technik (T), Performance (P) or Abstiege (A).
_Avoid_: Role, Rolle (for this meaning)

**Jury category** (Jury-Kategorie):
A unit judges register for and are assigned to, e.g. one heat or one final, worth a number of jury deployments.
_Avoid_: Category (without "Jury-"), Wertungsklasse

**Jury deployment** (Juryeinsatz):
One unit of jury work; a heat counts 2, a final 1.
_Avoid_: Einsatz (alone), shift

**Jury obligation** (Pflicht-Juryeinsätze):
The jury deployments a club must provide: 4 for every started two of its acts.
_Avoid_: Quota

**Credited deployments** (angerechnete Einsätze):
The deployments a club's trained judges registered for, each at most their maximum, unless the organizer credits a different number.

**Jury plan** (Juryplan):
Which judge judges which jury category in which task; published to the clubs after the judge registration deadline.
_Avoid_: Jury schedule, Einteilung

**Objection** (Einspruch):
A written complaint about a result, filed by the club representative with the jury lead within 30 minutes.
_Avoid_: Protest

### Awards

**Award ceremony** (Siegerehrung):
The ceremony honouring a category's or final's riders, with their acts' music.

**Medal** (Medaille):
An award with a sticker for each regular rider of the first three acts of a category.

**Trophy** (Pokal):
An award for each rider of the first three places of a final, and for each group act on the podium.

## Relationships

- A member of the **Organizer team** holds one or more **Roles**; the same user can also be a **Club contact**, and neither widens the other.
- A **Club** has **Riders**, registers **Acts** and **Judges**, and owes an **Amount due**.
- An **Act** has one or more **Starts**, one per **Rider**; a group's **Replacement riders** have starts too.
- A **Rider** is in at most one **Pair**, or has at most one **Pair request** instead; a pair request is one **Start**.
- An **Act** belongs to one **Category** once it is complete and not withdrawn; a **Category** belongs to one **Heat**.
- The best acts of a **Heat** fill the **Finalist slots** of its **Final**.
- A **Block** runs one **Category** or one **Final**; **Custom entries** and **Breaks** fill the rest of the **Timeplan**.
- A **Judge** registers for **Jury categories** for **Tasks**; the **Jury plan** assigns each judge at most one task per jury category.
- A **Booking** is a **Payment** of one **Club**, an **Unassigned booking** or an **Ignored booking**; a club's **Paid** amount is the sum of its payments, set against its **Amount due**.
- A club's **Credited deployments** below its **Jury obligation** cost a **Jury fee** per missing **Jury deployment**.
- A **Fee waiver** applies to a **Club**, a **Rider** or an **Act**; it lowers the **Amount due** but never the **Jury obligation**, except for a withdrawn act.

## Example dialogue

> **Club contact**: "Lea rides a single and is a replacement rider in our Großgruppe — is that two starts?"
> **Organizer**: "Yes, two starts, so two start fees, but at most the cap per rider. The Großgruppe is one act, and since your club created it, it counts for your jury obligation."
> **Club contact**: "Our Kleingruppe only has two regular riders yet."
> **Organizer**: "Then it's an incomplete group: no category, but it already pays its starts and counts as an act for your obligation."
