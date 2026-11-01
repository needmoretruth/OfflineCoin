# OLC Core: how the window looks and why

Written before the code, as the design method asks. Tried and abandoned lines go at the end.

## Subject

A person who owns a coin and a few small boards. What they hold is money, and what they do is
read it and sign for it. The nearest old thing in the world is a bank book and a paying-in slip,
so the window is a ledger and a slip. It is not a mining dashboard.

Tradition borrowed: the information design of Massimo Vignelli (one grid, few sizes, weight
carries rank) and the grammar of a bank slip (what, amount, to whom, signature at the end).

## Three rules that settle arguments

1. The eye goes to one place. Each screen has one large thing: the balance, or the slip's amount.
2. A value sits next to its name. Name, value, control. Never a tower of boxes.
3. Weight follows result. Only the action that moves the screen forward is filled. Anything that
   spends or cannot be undone is read on a slip first, and dangerous ones show red.

## Colour (black and white family, colour only for state)

| Role | Dark | Light |
| --- | --- | --- |
| Background | radial at top `#1A1E27`, `#0E1014`, `#0A0C10` | radial at top `#FFFFFF`, `#F4F5F7`, `#ECEEF2` |
| Surface | `#181C23` to `#13161C` | `#FFFFFF` to `#FAFBFC` |
| Edge | white at 7 percent | `#0E1014` at 8 percent |
| Ink, strong to quiet | `#E6E8EC`, `#B3B9C4`, `#8B919C` | `#1A1D22`, `#4A5059`, `#5A6270` |
| Main button | `#FFFFFF` to `#D9DDE4`, black text | `#2A2F38` to `#0E1014`, white text |
| Good | `#30A46C` | `#167A3B` |
| Waiting | `#F5C242` | `#9A6700` |
| Danger | `#FF5D5D` | `#C2352B` |
| News | `#5AA9FF` | `#2563C9` |

State colour shows as a dot or a faint glow, never as a filled panel.

## Faces and sizes

Inter for Latin and numbers, with its tabular digits baked in so that amounts line up. Noto Sans
KR for Korean. Both OFL, embedded, weights 400, 500, 600. IBM Plex Mono only for addresses.

Window sizes are the first-class ones: 1280x800 and 1920x1080. No phone layout.

| Use | Size |
| --- | --- |
| Balance | 44/600, gradient ink, tabular |
| Slip amount | 32/600 |
| Screen title | 20/600 |
| Section title | 15/600 |
| Body, rows | 14/400 |
| Meta | 13/400, the floor |

List rows 38. Buttons 34. Text inputs 34. Side rail 200. Corners by role: tag 6, control 8, button
and input 10, list and card 12, sheet 16. Pills only on what is truly round: a dot, a switch.

## Layers

Three at most: background, surface, floating. A shadow belongs only to what floats (slip, sheet).
Surfaces get a gradient, a hairline edge and a 1px inner highlight on top. Hover is one wash layer.

## Wireframe, basic mode, Home at 1280x800

```
+---------+---------------------------------------------------------+
| (logo)  |  Balance                                                |
|  OLC    |  1,234.50 OLC                                           |
|         |  12.5 OLC arrives soon                                  |
|  Home   |                                                         |
|  Boards |  [ Start mining ] ( Receive ) ( Send )                  |
|  Coins  |  * Mining, 18,400 tries a second, a block in about 12 s |
|  Records|                                                         |
|         |  What happened                       | This coin        |
|         |  Today 14:02  Mined        +50.00    | Blocks   1,204   |
|         |  Today 13:40  Received     +12.50    | Boards   2       |
|         |  Mon   09:11  Sent          -4.00    | Nearby   3       |
|  ...    |                                                         |
| Settings|                                                         |
+---------+---------------------------------------------------------+
```

Boards is a list with a register beside it. Settings is a register. A slip floats over all of it,
with the rest dimmed.

## Motion

- A press shrinks the control to 98 percent in 120 ms.
- One state change moves softly: when the balance changes it rolls to the new value over 320 ms,
  and the new row in the ledger lights for the same time. That is the stamp of a result.
- With reduced motion on, nothing moves.

## Words

Basic mode says speed, tries a second, a block in about N seconds, by cable, nearby. Expert mode
overrides only where the term differs. Basic tables are checked against a banned list.

## Next to a modern finance app

Compared at 1280 wide with Trezor Suite, from the Buy screen on trezor.io. That is the maker's own
marketing picture of the real app, not a live capture, so motion cannot be judged from it and sizes
are measured after scaling its window to 1280. OLC Core is the real window on this PC, light theme,
with real nodes running.

| Value | Trezor Suite | OLC Core | Judgment |
| --- | --- | --- | --- |
| Light | flat page `#F3F5F8` | page from `#FFFFFF` at top to `#ECEEF2` | same family, ours has light from above |
| Material | white cards, no gradient | white to `#FAFBFC`, hairline, inner highlight | same family, ours a little richer |
| Depth | card on page, no visible shadow | card on page, shadow only on what floats | same |
| Radius | cards about 16, inputs about 8 | cards 12, inputs 10, sheets 16 | same family |
| Density | inputs about 48, card padding about 24 | inputs 34, rows 38 to 46, card padding 16 | ours is denser on purpose: a ledger is read, a buy form is filled in |
| Largest figure | about 36 | 44 | ours gives money more weight |
| Main action | filled green | filled black or white | ours stays in the black and white family, as the brief asks |
| Motion | not visible in a still | press 98 percent in 120 ms, balance rolls in 320 ms | ours exists and is short |

What still reads older than Trezor: long registers in Settings are text only, and Trezor's form has
more air. Air was left out on purpose for the register, so a person sees more rows at once.

Picture: `.scratch/compare/side-by-side.png` in the working copy that made this judgment.

## Tried and abandoned

- IBM Plex Sans KR for Korean. Replaced by Noto Sans KR, as the brief names it. Plex Mono stays for
  addresses.
- A native file dialog. It needs a helper on some systems and looks foreign, so the picker is a
  sheet of this app.
- The notice as a bottom panel. A panel drawn before the page renames every widget in the page when
  it comes and goes, and a press on the old name is lost. The notice now floats after the page. Any
  block above a control that comes and goes with data sits in `widgets::slot`.
- Found coins above the fixed choices, on the first run and on Coins. A coin can turn up at any
  moment, and a row arriving above a button moves the button away from a press. What turns up on its
  own now sits under the buttons, and a screen test fails if a control moves when the data changes.
- A top strip with the language button. It held nothing else, so language moved to Settings.
- A fair lock on the runner (`Turns`), tried against slow orders. Full speed on an idle PC: worst wait
  527 ms to 168 ms. Full speed with 16 hogs on 2 CPUs: worst wait 748 ms to 2.1 s. No clear gain, and
  it did not cause the failure it was tried for, so the plain lock stays.
- A Hangul offset of 0.04 em. It pushed Hangul below digits and periods. At 0 they share a line.
