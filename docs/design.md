# gh-core design direction

_Owner: Josh. Last updated: 2026-11-16._

The app should feel **modern, round and bouncy, like a native iOS app**, and carry the Geaux Hack 2027 storyline: **"Geaux Beyond the Known — Beau vs. Parkeaux."** It borrows ideas, not assets, from other hackathons (listed below). We never copy another event's artwork, copy, code or branding.

## Typography: Allo

- **Font:** [Allo](https://allo.joshpuckett.me/) ([GitHub](https://github.com/joshpuckett/allo)). Free and open source under the **SIL Open Font License (OFL)**, so it can be bundled in the app and self-hosted. The license text ships with the font files.
- **Variable axes:** weight `wght` 100–900, roundness `ROND` 0–100, optical size `opsz` 14–32 (applied automatically). Has a true oblique, tabular figures, fractions, and arrows that form from `->` / `=>`.
- **How we use it:**
  - Roundness is **fixed at 80** across the whole app, which gives the "round" feel.
  - Tabular figures (`font-variant-numeric: tabular-nums`) for countdowns, the leaderboard and point totals, so numbers don't jiggle as they change.
  - No oblique for now. Story text uses weight 600 instead.
- **Size, measured 2026-11-16** (fonttools, from `AlloVariable.ttf`). The full variable `woff2` is **432KB**.

  | Subset | Size |
  |---|---|
  | Latin, all three axes | 310KB |
  | Latin, `opsz` fixed at 24 | 139KB |
  | Latin, `opsz` 24, `ROND` 60–100 | 103KB |
  | **Latin, `opsz` 24, `ROND` fixed at 80, `wght` 400–800** | **50KB ← chosen** |
  | Basic Latin (ASCII) only, same settings | 37KB |

  Varying roundness per element would roughly double the file, so it stays fixed. The chosen subset covers Latin-1 (accented names), dashes, quotes, arrows and ★ ♥ ✓ ✕. The subset script goes into the repo in PR 1.2.
- **License:** Allo's OFL declares no Reserved Font Name, so a subset may keep the name "Allo". `OFL.txt` ships alongside the font file.
- **Loading:**
  - Self-hosted, preloaded, and precached by the service worker; bundled in the native app.
  - `font-display: swap`, with a fallback font whose metrics are adjusted to match, so the page doesn't shift when Allo arrives (this keeps Lighthouse's layout-shift score clean).
  - The font file **does not count toward the 35KB JS/CSS budget**, but it does count toward Lighthouse Performance.

## Look and feel

**Style tile:** https://claude.ai/artifact/LPhptxiLDtmHg42fyfagLa (private until shared). It shows type, both themes, the teams, the Districts, components, and three phone screens: Home, Leaderboard and Registration.

**Simplicity, borrowed from HackUTD:** lots of empty space, one accent color per screen, near-black on a plain background, pill buttons, big tightly spaced headings, and a few deliberate, repeated animations rather than many different ones.

- **Liquid glass tab bar.** The bottom nav floats over the content as a glass capsule, so content scrolls underneath it.
  - **Web and Android:** CSS approximation that should read clearly as glass: a light blur that keeps the content behind it visible (`backdrop-filter: blur(12px) saturate(230%) brightness(1.06)`), a mostly clear fill, a bright 1.5px rim, a strong top-left shine, and a purple glow along the inside bottom edge. Where `backdrop-filter` isn't supported, it falls back to a nearly solid fill.
  - **iOS 26 and later:** use Apple's real Liquid Glass material where possible. That needs a native tab bar (or a small custom Capacitor plugin using `UIGlassEffect`) over the WebView; decide in PR 1.4 whether it's worth the extra code. Older iOS versions use the CSS version.
  - The active tab gets a soft tinted lens behind its icon, not a solid fill.
- **Things we avoid** (they read as generic AI-generated UI):
  - **Tinted background shapes behind small things:** pill-shaped tags and badges ("Web & App", "ON DECK" chips), and icons sitting in a pale rounded square or circle. The problem is the soft tinted container itself, whatever its shape. A small icon placed above or beside a card's title has the same generic look. If a card needs an icon, make it part of the card's background: large, faint (about 16% opacity), in the track color, cropped off a corner. Otherwise use colored text or a heading. Buttons and the tab bar can stay capsule-shaped.
  - **Putting everything in a card.** A screen that's a stack of rounded white boxes gets repetitive. Use other structures: type set directly on the page (Home's countdown), a timeline (Up next), plain lists with hairline dividers and sticky headers (Schedule), full-bleed bands and rows (the landing page). Save contained shapes for things that really are separate objects, like the tab bar or an urgent alert.
  - **Busy heroes.** The landing page hero follows HackUTD: full screen, one short italic line, one medium-weight heading in sentence case, the date, and a thin line drawing of campus on Nicholson across the whole bottom edge. The drawing sketches itself in piece by piece, clouds drift, and then Beau's crown drops in from above and falls through the pothole, once. The pothole flashes gold and keeps a soft glow. The crown is never shown standing still before it falls. No glow, giant icon, paragraph or second button.
  - **A long page before registration opens.** Until then, geauxhack.com is one screen with nothing to scroll, like HackMIT's between events: the hero, a "notify me" email field, and a sponsor contact line. The story, tracks, FAQ and sponsor sections appear when `REGISTRATION_OPEN` is set to true. There's no dates timeline.
  - **Middle-dot separators between words** ("PFT 1240 · in 12 min"). Write a phrase ("Starts in 12 min in PFT 1240") or split it into two elements.
  - **Lists of three in copy** ("purple, gold, the crown"). Say the one or two things that matter.
- **Tabs, the same in both themes:** Home, Schedule, Scan, Map, Battle. Profile and the QR pass open from the avatar button at the top of Home.
- **The Beau vs Parkeaux battle lives only on the Battle tab.** Home shows urgent alerts, the countdown and what's next.
- **Order screens by urgency:** anything that needs action now comes first (ON DECK and other judging alerts, then the hacking countdown), then what's next on the schedule. The Beau vs Parkeaux score only matters at the very end, so it sits below the alerts and countdown.
- **No orange.** Parkeaux, alerts and "starts soon" use LSU gold.
- **Battle card:** one card with both totals in ink (black), team names in their colors (Beau purple, Parkeaux gold), and a single split bar of Beau purple against Parkeaux gold. Same design in both themes.
- **Shape:** large corner radii (cards 20–28px, sheets 32px, buttons fully round). Cards float on a soft, layered background. iOS-style bottom tab bar and sheets that slide up.
- **Motion:**
  - Spring easing written in CSS with `linear()`, not a JavaScript animation library, to protect the bundle budget.
  - Buttons press down to about `scale(0.96)` and bounce back.
  - Sheets and toasts spring in.
  - Points pop with a little overshoot.
  - Native haptics fire on confirming actions.
  - Everything respects `prefers-reduced-motion`.
- **Motion tokens** (all CSS, used in the style tile):

  | Token | Value | Use |
  |---|---|---|
  | `--out` | `cubic-bezier(.16,1,.3,1)` | Hover, card lift, label roll (0.3–0.45s). This is HackUTD's curve. |
  | `--spring` | CSS `linear()` spring | Release after a press (0.55s) |
  | overshoot | `cubic-bezier(.34,1.56,.64,1)` | Entrances: badges, bars, points (0.8–1.1s) |
  | press | `scale(.94)` over 0.12s | Every button and tab |
  | roll | duplicated label, `translateY(-50%)` | Pill buttons on hover or keyboard focus (HackUTD) |
  | twinkle | opacity and scale loop, 4–6s, staggered | Gold stars in Abyssus (HackUTD's star field) |
  | pulse | expanding ring, 1.8s | "Starts soon" and "ON DECK" only |
- **Two worlds = two themes.** The storyline's split works directly as light and dark mode:
  - **Elysiu (light):** LSU campus. Warm cream paper, purple and gold.
  - **Abyssus (dark):** the dark fantasy world ruled by the Vespers. Near-black asphalt with glowing accents.
  - The app follows the system setting by default, with a manual toggle.

## Color

Starting palette. Final hex values get checked for contrast (WCAG AA) when the Tailwind theme is built.

| Token | Use | Starting point |
|---|---|---|
| `beau` | Team Beau, primary | LSU purple `#461D7C` |
| `crown` | Highlights, the crown, wins | LSU gold `#FDD023` |
| `parkeaux` | Team Parkeaux | LSU gold `#FDD023`, the same gold as the crown. No dark mustard gold anywhere; text that would have used it is ink or muted instead. |
| `paper` | Elysiu background | Cream `#FDF7EC` |
| `abyss` | Abyssus background | `#0E0B14` |

**Districts (tracks)** take their color from each Vesper's costume:

| District | Track | Vesper | Color |
|---|---|---|---|
| The Fragmented City | Web & App | Drexa | Glowing cyber blue |
| The Neural Nexus | AI/ML | Nexus | LED purple |
| The Forgotten Grounds | Beginner | Onyx | Explorer green |
| The Oddworks | Funniest / Most Useful | Hei Feng | LSU gold (placeholder; this track may change) |
| The Hidden Territories | Sponsor | Tartereus | Merchant amber |
| (Envoy) | — | Vipera | Emerald, metallic |

Before using LSU's purple and gold, check LSU's brand guidelines for student organizations.

## Ideas borrowed from other hackathons

| From | What we take | Where it goes in gh-core |
|---|---|---|
| **HackMIT** (hackmit.org, Plume) | Registration as a **lifecycle timeline**: applications open → deadline → decisions → confirmation → event, with the current step highlighted and dates on each. Calm cream-and-indigo palette. | Registration and dashboard |
| **TreeHacks** | An **interactive track wheel** you tap to explore each track. A big stats row (hours, hackers, prizes). | Picking a District, landing page |
| **HackUTD** | Every year has a named theme, and past themes are shown as a **history timeline**. | "Previous Geaux Hacks" section; Beau as a recurring mascot |
| **Knight Hacks** | Warm, beginner-friendly copy ("Start wherever you are"). Display headings paired with a friendly sans. A dashboard sign-in on the landing page. | Copy tone; Forgotten Grounds (beginner) messaging |

## Registration (modeled on HackMIT's flow)

We copy the **structure** of HackMIT's Plume portal, not its visuals, wording or code. (Plume is built with React and Mantine, which wouldn't fit our 35KB budget anyway.)

1. **Public page:** a timeline of application phases with dates (opens, deadline, decisions, confirmation due, event). The current phase is highlighted, with a "Sign in to apply" call to action.
2. **Sign in** (auth PR): OAuth or email/SMS code.
3. **Application:** a multi-step form with a progress bar, saving each step as a draft so it can be resumed. Steps: about you, school, experience, interest in Districts (tracks), dietary needs, agreements (MLH code of conduct, media release).
4. **Status card:** Draft → Submitted → Under review → Admitted / Waitlisted / Not admitted.
5. **Confirmation (RSVP):** admitted hackers confirm by a deadline, with travel reimbursement (if offered) and shirt size. Expired confirmations release spots to the waitlist.
6. **After confirming:** the dashboard becomes the event app (pass, schedule, team).

**Choosing Team Beau or Team Parkeaux happens at the opening ceremony, not at registration**, to match the storyline (Chapter 3).

## Picking a side (Team Beau or Team Parkeaux)

Hackers choose their own team at the opening ceremony, but **neither team can go above 52% of all picks**, so one side can't run away with the game.

**The rule**
- A team is locked while its share of picks is **above 52%**.
- While a side is locked, its half is dimmed with a lock icon and the message "Full for now". The open side is expanded and shows a banner calling for help: **"Beau needs help!"** or **"Parkeaux needs help!"**
- A pick counts only once the server accepts it. The server checks the cap inside the same database transaction that records the pick, so two people tapping at once can't push a team over 52%. The client's lock state is only a preview.
- Early on, a single pick swings the percentage a lot (the very first hacker puts one side at 100%). Picks will alternate until enough people have chosen. If that feels bad at the ceremony, add a grace period: don't enforce the cap until about 20 picks.
- A pick is final once made.

**Look and motion** (inspired by Fortnite's split-screen face-off art, not copied from it)
- The screen is split down a diagonal seam that glows gold. Beau is a purple glow with gold sparks. Parkeaux is dark asphalt with faint gold road stripes.
- **Hover (web) or tap (phone)** springs the chosen side open to **60%** of the screen. Its emblem grows and the other side shrinks and dims. The join button appears only on the open side.
- **Web and big screens:** left and right halves. **Phone:** top and bottom halves, so the open side gets 60% of the height.
- The live split ("Beau 47%, 53% Parkeaux") sits at the bottom as one bar.
- After joining, a confirmation card pops in.
- The crown and toll-barrier emblems are placeholders until the Beau and Parkeaux artwork is ready.
- Designs: the "Pick your side" artboards on the style tile.

## Story features this adds (not in the architecture spec yet)

The storyline needs features that `GH_Tooling.pdf` doesn't cover. Each needs a backend owner:

- **Picking an alliance** (Team Beau or Team Parkeaux) at the opening ceremony.
- **Point QR codes** scanned at workshops, mini-games, meals and sponsor booths, plus **side quests**. These can reuse the entitlement scanner pattern: HMAC-signed codes, scanned offline, with a points ledger.
- **Live leaderboard** of the two teams, broadcast over WebSocket.
- **Ending reveal:** the final points total decides which closing video plays.

## Assets needed

- Beau (tiger with a crown and purple cape) and the six Vespers, as **SVG** illustrations, with simple versions for small sizes.
- District illustrations or icons, and the Elysiu/Abyssus portal key art (the main graphic).
- App icon and splash screen.
