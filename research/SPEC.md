# Nusa notch: observed spec (2026-10-31)

Sources: live screenshots of Nusa 1.0.0 on this PC (1920x1080, 100% scale) in this folder,
and the preview CSS on the official site (site/site.css). Sizes are in dp (1 dp = 1 px at 100%).

## Window
- One window, class `NotchWindow`, 640x190 at top-center (x 640..1280, y 0).
- Ex styles: TOPMOST | TOOLWINDOW | NOACTIVATE | NOREDIRECTIONBITMAP (DirectComposition, per-pixel alpha).
- Hover is detected with a low-level mouse hook (SetCursorPos alone does not open it).
- Draws only while something animates; 0% CPU idle.

## Shape
- Fill pure #000. Bottom corners rounded, top corners flare OUTWARD (concave "ears"
  of radius e, like the MacBook notch). Soft shadow below: 0 14px 34px -16px rgba(0,0,0,.95).
- States (w x h, bottom radius r, flare e):
  - idle 150x30, r14, e10   (minimal-notch mode: a thin black line, measured ~44x4)
  - pages 420x140, r32, e22 (music, pomodoro, water, shelf, todo)
  - lyrics 520x140, r32, e22
  - peek 310x60, r19, e13   (bluetooth, charging)
- Size animates with spring cubic-bezier(.34,1.32,.5,1) over .62s; radius .45s ease.
- Layer content fades in .32s with .2s delay after the shape grows.

## Chrome shared by all pages
- Header (y~19): page title left at x15, 15px/600 (two-tone, e.g. "Water" white + "Reminder" blue,
  "Todo" white + "List" green); centered: battery ring 15px (green conic, % filled) +
  "Sat 31 Oct 2026" + bold "23:57".
- Side arrows: 22px circles rgba(255,255,255,.28) with 9px chevron, vertically centered, ~12px in from edges.
- Page dots at (210,133): 3.6px dots, gap 4.5, rgba(255,255,255,.45); active one is a 15px white pill.
- Page order: Music, Pomodoro, Water, Shelf, To-do. Mouse wheel / arrow keys switch pages.

## Music
- Art 64x64 radius 10 at left; title bold white ~15px, artist dim below (Arabic shaped RTL correctly).
- Progress row: elapsed left, 4px bar rgba(255,255,255,.22) with white fill, "-remaining" right.
- Controls 17px white: prev / play-pause / next centered; lyrics bubble button bottom-left.
- Idle while playing: equalizer bars (5 bars 2.2px wide, animated) in the closed notch.

## Lyrics (notch widens to 520)
- Left half: compact player (art, title, progress, controls). Right half at x264 w234:
  lines 16px/700, dim rgba(255,255,255,.3) scale .93, current white scale 1, list scrolls with spring .7s,
  top/bottom fade mask. Background: horizontal gradient from album-art color, ~82% height, glow at bottom.
- Click a line to seek. "Lyrics not available" when missing.

## Pomodoro
- Red ring 64px (conic #ff5a6e/#ff3a3a), glow, timer icon inside, at (80,74).
- "25:00" large bold + reset icon; "Focus time" dim.
- Pills 28px high rgba(255,255,255,.12): "Short Break", red play button, "Focus Time". Chime at end.

## Water
- "0.5 / 2.0 L" (big bold + dim), "Today · 25% · Next 01:07", bar 152x5 gradient #66dbff→#2d6bff.
- Toggle switch 34x20 #2699ff with bell (reminders on/off). Buttons: "Drink" (blue), "Settings".
- Right side has 2 sub-pages (dots top-right): ring gauge with "25 %", and an hourly bar chart (0–4000 ml).
- Settings: Bottle (- 500 ml +), Goal chips 1.5/2.0/2.5/3.0 L, Hours from–to (+/-), summary card
  "4 reminders / 3 refills", Reset, back button. Reminds only inside the hours.

## Shelf
- Drop zone 336x90 radius 16, dashed border rgba(255,255,255,.3); hover: brighter + scale 1.02, arrow bobs,
  text changes "Drop files here" → "Release to add". Cards 62 wide: 44px thumb, name 10.5px/600, size 9.5px dim.
- Cards fall in with a bounce. "Clear" small pill next to title. Drag files back out. Click to open.

## To-do
- Big "0/0", "Task Done Today", buttons "Add Task" (green +), "Edit Task".
- Right: list 176 wide, rows 28px radius 9 green-tinted, round green check; empty state "No tasks." card.
- Add form: "Task title" input, time range __:__-__:__, Cancel / Save (green). Due-time reminder with snooze.

## Other
- Peeks: charger plugged (ring + %), headphones connected (name, battery).
- HUDs: volume, brightness, Caps Lock, keyboard language.
- Mascot: when idle, clock fades and a pair of glowing eyes appears; they doze, wave when you return.
- Tray menu: Start with Windows, Mascot, Minimal notch, About, Exit. Hides for full-screen apps.

## Water reminder: observed behavior (Nusa 1.0.0, tested 2026-11-01)

Data (%APPDATA%\Nusa\water.txt): bottle, goal (ml), start/end (minutes after midnight),
day, taken (ml today), inb (ml left in the bottle), fired, hours (24 per-hour ml), on.

- **Schedule:** one reminder per 15 minutes inside the hours window, at the middle of each
  slot (window 16:30-17:30 -> 16:37:30, 16:52:30, 17:07:30, 17:22:30). Slots already past
  when the window is set are skipped. Reminders stop once the goal is reached, or when off.
- **Amounts:** reminders = window minutes / 15; each reminder = goal / reminders
  (shown as "750 ml each"). Refills = goal / bottle - 1.
- **Settings:** bottle 250+ ml in 50 ml steps; goal chips 1.5/2.0/2.5/3.0 L; hours move in
  30-minute steps, the window is at least 1 h, and moving the start moves the end with it.
  Summary card: "<n> reminders", "<n> refills"; Reset.
- **The reminder is a peek in the notch** (about 310x60), not a Windows toast. It stays
  until answered (still up after 3 minutes; hovering only lights the icon ring).
  - Bottle empty (inb < one reminder): bottle icon, "Time to Refill", "Your bottle is
    probably empty", buttons **Snooze** (gray) and **Refilled** (blue).
  - Refilled -> inb = bottle, and the peek turns straight into "Time to Drink": drop icon,
    "Stay hydrated! · <amount>", buttons **Snooze** and **Drank**.
  - Drank -> taken += one reminder's amount (inb drained), the peek closes.
  - Snooze -> asks again 10 minutes later ("Next" shows the snoozed time).
- **Page:** "0.8 / 3.0 L", "Today · 25% · Next 17:20" (or "Reminders off" / "Goal reached");
  Drink adds one reminder's amount and doesn't move the next reminder; the percentage can
  pass 100%. Bell switch off: switch and gauge turn gray (Drink still works in 1.0.0).
- **Chart:** bars only for the window's hours, the current hour highlighted with a bubble
  showing its ml; axis 0 / goal / 2x goal.
