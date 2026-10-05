# hackdiet

A local Hacker's Diet weight log. Enter a weight, get a summary and a chart in
Ghostty, and return to your shell. Data lives in a small Markdown table.

## Install

```sh
cargo install --path . --locked
```

The installed binary renders charts itself; Python, Typst, Kitty's `kitten`
command, and a browser are not required. Charts use an installed sans-serif font.
macOS supplies one. Linux builds need Fontconfig and FreeType development packages
and an installed font (for example, DejaVu Sans).

## Daily use

```sh
hackdiet 83.5
hackdiet 83.4 after a long walk
hackdiet
```

The first two commands save today's weight in kilograms. Entering a weight again
replaces that day's weight. A supplied note replaces the previous note; omitting
the note preserves it. Use `hackdiet 83.4 ""` to clear a note. Quote shell special
characters in notes as usual. Everything after the weight is the note, so put
options **before** the weight.

Each command shows the seven calendar days ending today, including the new entry.
`hackdiet` alone displays the same view without changing the file. Dates use your
computer's local timezone.

Set your height once, in centimeters, to include BMI:

```sh
hackdiet height 171
```

Example summary (values depend on your history):

```text
Weekly loss         0.14 kg/week
Estimated deficit   154 kcal/day
BMI                 28.7 (mean 28.6)
Latest trend        83.92 kg (2026-09-29)
```

In Ghostty and Kitty, the chart includes the selected dates, weekly change,
estimated daily calorie balance, latest BMI (with height set), and latest trend.
The statistics appear as large, muted numbers above the plot. Weekly gain and
estimated calorie excess use muted red. `--no-graph` and redirected output retain
the aligned text summary, including mean BMI.

Weight change and calorie estimates require at least two weigh-ins in the selected
period. A first entry still produces a chart and, with height set, a BMI.

## Longer views

Like `mdl`, `this` and `last` count **calendar months**:

```sh
hackdiet this       # current month, through today
hackdiet this 3     # current month and the preceding two
hackdiet last       # previous complete month
hackdiet last 3     # three complete months before this month
```

The summary and graph cover the selected period. The weekly change is the fitted
rate over that period expressed in kg/week, not a comparison of its endpoints.

## Files and corrections

By default the file is `hackdiet.md` in the current directory. To use the same log
from anywhere, set an absolute path in your shell configuration:

```sh
export HACKDIET_FILE="$HOME/Documents/hackdiet.md"
```

The parent directory must exist. `--file PATH` overrides this setting:

```sh
hackdiet --file personal.md 83.5
hackdiet --date 2026-09-28 83.7 missed yesterday
hackdiet --no-graph 83.5
hackdiet --png month.png this
```

`--date` records or replaces a past entry and shows the week ending on that date.
Future entries are rejected. The Markdown file looks like this:

```markdown
# Weight log

Height: 171 cm

| Date | Weight (kg) | Note |
|------------|-------------|------|
| 2026-09-28 | 83.7 | missed yesterday |
| 2026-09-29 | 83.5 | after a walk |
```

Height is optional. Edit the table directly if you prefer; the calculations always
use the complete history, before selecting a display period. Dates must be unique,
weights positive, and the table must keep these three column headings. Notes may
contain escaped pipes (`\|`). Prose outside the table is preserved. Writes replace
the file atomically; a hidden lock file serializes writes by concurrent `hackdiet`
processes. Invalid data is rejected before saving. The default personal log is
git-ignored.

## Charts

Ghostty and Kitty are detected automatically. Each invocation prints a fresh chart
below its summary using the Kitty graphics protocol, then exits. There is no TUI,
background process, or live resize handler; run the command again after resizing.

The red line is the smoothed trend. Green stems connect daily weight diamonds to
the trend. Views longer than 92 days use a subdued line for daily weights.
Thin dashed vertical lines mark Sundays, including days without a weigh-in.
When height is set, the right axis shows the BMI corresponding to the weight
scale on the left; both axes describe the same plotted lines and points.
PNG export uses the same renderer. On other terminals, inside tmux/screen, or when
output is redirected, a plain weight/trend table is shown. `--kitty` forces graphics
on a terminal known to support them; redirected output always stays plain text.
`--no-graph` suppresses both the chart and fallback table.

## Calculations

Based on [The Hacker's Diet](https://www.fourmilab.ch/hackdiet/e4/pencilpaper.html)
and the [original application source](https://github.com/Fourmilab/hackers_diet_online):

- Initialize the trend to the first measured weight.
- On a weigh-in, `trend += (weight - trend) / 10`, keeping full precision.
- Carry the trend unchanged across missing days, including month boundaries.
- Fit a least-squares line to daily trends in the selected period.
- Weekly change is `slope × 7`; estimated daily calorie balance is `slope × 7716`.
- BMI is trend weight divided by height in meters squared. Mean BMI uses the daily
  trends in the selected period; most recent BMI uses its final trend.

The series starts on the first actual observation and ends on the latest one:
unobserved days before or after the history do not dilute the estimated rate.
This intentionally avoids the original monthly application's initial backfill
before the first weigh-in. Missing days inside the history retain the prior trend.
The calorie figure is the Hacker's Diet model's estimate of net deficit or excess,
not a measurement of food intake or calories burned.

## Development

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
```
