# Charts screen reader script

## Purpose

Charts are drawn on the server. The drawing is hidden from a screen reader, so what a reader gets is the chart's name, a written summary, a legend, a data table, and, with script on, a line spoken from a live region each time a key moves between values. This script settles those spoken parts for each chart type: reading a chart in order, the table and its switch, the keys and the words they produce, the legend switches, the empty, loading and failed states, the report page where a link changes the grain and the page title is announced, a large series, and the page with script off.

## Page to open

The UI proof app, http://127.0.0.1:8090 (see [Start the proof apps](README.md#start-the-proof-apps)). Each chart is opened alone with `?case=<name>`, and the address ends with `#<id>` so that the chart is where the reader starts and one press of Tab reaches it:

| Step | Address |
|---|---|
| 1 to 8 | `/charts?case=bar-stacked#bar-stacked` |
| 9 | `/charts?case=line#line` |
| 10 | `/charts?case=line-gaps#line-gaps` |
| 11 | `/charts?case=bar#bar` |
| 12 | `/charts?case=donut#donut` |
| 13 | `/charts?case=funnel#funnel` |
| 14 | `/charts?case=sparkline#spark-1` |
| 15 | `/charts?case=locale#locale-in` |
| 16 | `/charts?case=rtl#rtl` |
| 17 | `/charts?case=empty#empty` |
| 18 | `/charts?case=loading#loading` |
| 19 | `/charts?case=failed#failed` |
| 20, 21, 28 | `/charts?case=bar-grouped#bar-grouped` |
| 22 to 24, 29 | `/charts/report?from=2026-10-28&to=2026-11-03` |
| 25 | `/charts/report?from=2026-10-28&to=2026-11-03&grain=hour#report-area` |
| 26 | `/charts/report?from=2024-01-01&to=2026-11-03` |
| 27 | `/charts/big?n=5000#big` |

Today is 3 November 2026 on this app, and the data is fixed, so the numbers below are the same on every run.

## Before you start

- JavaScript is on, except in steps 28 and 29. Browser window at least 1000 px wide, zoom 100%.
- NVDA: speech viewer open. You read in browse mode and use the chart's keys in focus mode (NVDA+Space switches). VoiceOver: caption panel on, and "Press Tab to highlight each item" on in Safari.
- The chart's live words follow one pattern: the row's name, then each series' name and its value, separated by commas. For example `Week 41, Reported 42, Ignored 120, Clicked 30, Total 192`. The words are the same as the matching row of the chart's table (step 3 shows how to read a row). Where a step gives a row's words, the numbers are from the proof page's fixed data.
- A dash for a missing value is the one place the live words and the table differ: the live words say the dash, and the table cell says "No value" (step 10).
- Open each address in a new load of the page (type it in the address bar, press Enter). If Tab does not land on the chart on the first press, press it until the reader says the chart's name, and write the number of presses in the Notes.

## Steps

### Reading a chart

**1. Read a chart from its title to its table switch**
- Keys: NVDA: open `/charts?case=bar-stacked#bar-stacked`, browse mode, NVDA+T for the page title, then Down arrow from the chart's title until the "Show table" check box. VoiceOver: VO+Right arrow the same way.
- The page: a chart titled "Outcomes by week" with a legend of three entries, a drawing of stacked bars, and a "Show table" switch under it.
- The reader says, in this order: the chart title "Outcomes by week"; a list of 3 items, "Reported", "Ignored", "Clicked" (no colour names, no "graphic"); a group named "Outcomes by week, chart"; then the summary, which reads: "Reported: highest 81 for Week 45, lowest 42 for Week 41, total 307. Ignored: highest 120 for Week 41, lowest 90 for Week 45, total 522. Clicked: highest 30 for Week 41, lowest 12 for Week 45, total 104."; then "Show table", check box, not checked. Nothing is said for axis numbers, bars or the tooltip. The summary is read once, not twice in a row.

**2. Show the table with the switch**
- Keys: NVDA: Tab or the F key to "Show table", then Space. VoiceOver: VO+Right arrow to the check box, then VO+Space.
- The page: the table appears under the chart. Before this it was not visible, but it was in the page for readers.
- The reader says: before, "Show table", check box, not checked; after, checked. The module says nothing else. Reading down, the table is announced with 5 columns and 6 rows and the caption "Outcomes by week".

**3. Read the table by cell**
- Keys: NVDA: browse mode, move into the table, then Ctrl+Alt+Right arrow, Left arrow, Down arrow and Up arrow. VoiceOver: VO+Shift+Down arrow to interact with the table, then VO+arrows; VO+Shift+Up arrow to stop.
- The page: nothing changes.
- The reader says: the column headers are Category, Reported, Ignored, Clicked, Total. Moving to a cell says its column header and its value ("Reported", "42"). The first cell of each row is the row's name ("Week 41"). The first row is Week 41, 42, 120, 30, 192. The last row is Week 45, 81, 90, 12, 183.

**4. Switch the table off and find it again**
- Keys: NVDA: Space on "Show table" to clear it, then the T key (next table) from the top of the page. VoiceOver: VO+Space on the check box, then VO+U, choose Tables.
- The page: the table is out of sight again.
- The reader says: "not checked" for the switch. The table is still found: the T key (or the rotor's Tables list) lands on it, there is exactly one table for this chart, and its cells read as in step 3.

### The keyboard

**5. Tab to the chart**
- Keys: both readers: reload `/charts?case=bar-stacked#bar-stacked`, then press Tab once.
- The page: a focus ring around the chart's drawing, and a tooltip showing the first week.
- The reader says: the name "Outcomes by week, chart" and that it is a group; its description (the summary from step 1 and "Use the arrow keys to move between values."); and, from the live region, "Week 41, Reported 42, Ignored 120, Clicked 30, Total 192". Each is said once. The live words are heard after the name, not cut off by it.

**6. Move with the arrow keys**
- Keys: NVDA: NVDA+Space to switch to focus mode, then Right arrow, Right arrow, Left arrow. If NVDA stays in browse mode the arrows move NVDA's own cursor and the chart hears nothing: write in Notes whether NVDA switched by itself. VoiceOver: Right arrow, Right arrow, Left arrow, without VO keys.
- The page: the tooltip and the highlighted bars move to the next week, then the next, then back.
- The reader says: after the first Right arrow, "Week 42, Reported 51, Ignored 110, Clicked 26, Total 187". After the second, "Week 43, Reported 63, Ignored 104, Clicked 21, Total 188". After Left arrow, the Week 42 words again. Each key press gives its row once, within about a second, and nothing else.

**7. Home, End, the page keys, Up and Down**
- Keys: both readers, in this order: Home, End, PageUp, Home, PageDown, ArrowDown, ArrowUp, End, ArrowRight.
- The page: this chart has 5 rows, so PageDown and PageUp move by one row, the same as ArrowDown and ArrowUp.
- The reader says, key by key: Home, the Week 41 row; End, "Week 45, Reported 81, Ignored 90, Clicked 12, Total 183"; PageUp, Week 44; Home, Week 41; PageDown, Week 42; ArrowDown, Week 43; ArrowUp, Week 42; End, Week 45; ArrowRight on Week 45 stays on Week 45: the same words may be said again, once for the press, and focus does not leave the chart.

**8. Escape, then leave and come back**
- Keys: both readers: Escape; then NVDA+Tab to say what has focus (VoiceOver: look for the focus ring on the chart); Tab; Shift+Tab.
- The page: Escape closes the tooltip and the highlight, and focus stays on the chart. Tab moves to the "Show table" check box. Shift+Tab returns to the chart.
- The reader says: nothing after Escape, and focus is still "Outcomes by week, chart". After Tab: "Show table", check box, not checked, with nothing about the chart. After Shift+Tab: the chart's name and description, then the words of the row you left (Week 45), so the chart keeps its place.

### The chart types

**9. A line with two series**
- Keys: NVDA: open `/charts?case=line#line`; browse mode and Down arrow from the title, then Tab and PageDown in focus mode. VoiceOver: VO+Right arrow, then Tab and PageDown.
- The page: a line chart "Opens and clicks by hour" with the description "3 November, Asia/Kolkata time", a legend of two entries, 24 hourly rows.
- The reader says: the title, the description "3 November, Asia/Kolkata time", a list of 2 items ("Opens", "Clicks"), the group "Opens and clicks by hour, chart", and a summary with one sentence for each series in the form "Opens: N at <hour>, N at <hour>. Highest N at <hour>, lowest N at <hour>." After Tab, the live words are the first hour, then "Opens N, Clicks N". PageDown moves by 2 rows (24 rows, a step of 2): the hour in the live words is two hours later. Show the table (step 2) and check that one row of the table has the same words.

**10. A missing value**
- Keys: both readers: open `/charts?case=line-gaps#line-gaps`, read the summary, Tab, Home, then PageDown three times (30 rows, a step of 3, which lands on a day with no value). Then show the table and find that day's row.
- The page: a line chart "Response time" with a gap in the line, the description "A gap is a day with nothing measured; one value between gaps is a dot", and a target line.
- The reader says: the summary ends with "Marked: Target 450 ms (450 ms)." On the day with no value the live words are the day, then "p95" and a dash. In the table, that row's cell reads "No value". Write in Notes how the dash was spoken (said as "dash", or silent).

**11. A bar chart with a goal**
- Keys: both readers: open `/charts?case=bar#bar`; Down arrow from the title; Tab; ArrowRight.
- The page: bars "Sent per day" for seven days, with a goal line of 1,500. One series, so there is no legend.
- The reader says: the group "Sent per day, chart" and the summary "Sent: highest 1,820 for <date>, lowest 302 for <date>, total 7,816. Marked: Goal (1,500)." (the dates as the table writes them). After Tab: the first day, then "Sent 1,204". After ArrowRight: the second day, then "Sent 1,480". The goal is in the summary only, not in the live words.

**12. A donut**
- Keys: both readers: open `/charts?case=donut#donut`; Down arrow from the title; Tab; ArrowRight; End.
- The page: a ring "Delivery" with four parts. The centre shows a total, and a list of the parts stands beside the ring.
- The reader says: the group "Delivery, chart" contains a list of 4 items: Delivered 9,412 and 97.5%, Bounced 188 and 1.9%, Deferred 40 and 0.4%, Rejected 12 and 0.1%. The centre total is not read. The summary: "4 parts, total 9,652. Largest: Delivered, 97.5%." After Tab: "Delivered, Value 9,412, Share 97.5%". After ArrowRight: "Bounced, Value 188, Share 1.9%". After End: "Rejected, Value 12, Share 0.1%". The icon next to Bounced is not spoken.

**13. A funnel**
- Keys: both readers: open `/charts?case=funnel#funnel`; Down arrow from the title; Tab; ArrowRight twice.
- The page: four stages, Delivered, Opened, Clicked, Converted, each a bar with its number and share. The description is "Each step as a share of everyone it reached".
- The reader says: the title "Campaign funnel", the description, then a list of 4 stages, each with its name, its number and a share. The summary: "From Delivered, 9,412, to Converted, 96: 1% of the first stage." After Tab: "Delivered, Value 9,412, Of first 100%, Of previous" and a dash. After the first ArrowRight: "Opened, Value 4,120, Of first 43.8%, Of previous 43.8%". After the second: "Clicked, Value 812, Of first 8.6%, Of previous 19.7%".

**14. Small charts in a figure**
- Keys: NVDA: open `/charts?case=sparkline#spark-1`, browse mode, Down arrow through the three cards, then press Tab. VoiceOver: VO+Right arrow through the cards, then Tab.
- The page: three figures, "Opened", "Bounced" and "Unsubscribed", each with a small line chart.
- The reader says: for each figure its label and number ("Opened", "4,120", "+12% on last week" for the first), then a picture named "Opens, last 24 hours, chart" (the second is "Bounces, last 24 hours, chart", the third "Unsubscribes, last 24 hours, chart") with the summary as its description, for example "Opens, last 24 hours: N at <hour>, N at <hour>. Highest N at <hour>, lowest N at <hour>." Tab does not stop on any of the three pictures. There is no "Show table" switch. Each picture has a table found with the T key (3 in all).

**15. Amounts in rupees**
- Keys: both readers: open `/charts?case=locale#locale-in`; Down arrow from the title; Tab; ArrowRight.
- The page: the chart "Revenue by month" for July to October, in rupees with lakh grouping.
- The reader says: the summary "Revenue: highest ₹16,10,000 for Oct, lowest ₹9,80,500 for Aug, total ₹52,42,800." The rupee sign is spoken as the currency, and each amount is heard as one number, not as separate groups of digits. After Tab: "Jul, Revenue ₹12,50,000". After ArrowRight: "Aug, Revenue ₹9,80,500".

**16. Right to left**
- Keys: both readers: open `/charts?case=rtl#rtl`; Tab; ArrowLeft; ArrowRight; ArrowRight.
- The page: a bar chart with Arabic labels, drawn right to left, 7 rows.
- The reader says: the name, summary and row words are read in Arabic if the reader has an Arabic voice (without one, mark the words n/a and test the keys only). ArrowLeft moves forward to the second row and ArrowRight moves back to the first, which is the reverse of the other charts. A second ArrowRight at the first row stays on it.

### The states

**17. No data**
- Keys: both readers: open `/charts?case=empty#empty`; Down arrow from the title; press Tab.
- The page: a chart "Bounces by domain" with a picture and a message instead of a drawing.
- The reader says: the title "Bounces by domain", then "No data" and "Nothing to show for this period." There is no group, no summary, no "Show table" and no table. Tab does not stop on the chart.

**18. Loading**
- Keys: both readers: open `/charts?case=loading#loading`; Down arrow from the title.
- The page: a chart "Opens by hour" with a placeholder bar. The figure is marked busy.
- The reader says: the title "Opens by hour", then "Loading Opens by hour". Write in Notes if the reader skips the busy figure and says nothing.

**19. Could not load**
- Keys: both readers: open `/charts?case=failed#failed`; Down arrow from the title; Tab; Enter.
- The page: a chart "Opens by hour" with a failure message and a "Try again" link. Enter loads the same page again, and the failure shows again.
- The reader says: the title, then "This chart could not be loaded" and "The analytics service did not answer in time.", then "Try again", link. Tab lands on the link, with a visible focus ring. After Enter, the reader says the page title "Charts" once, and the failure shows again.

### The legend switches

**20. Switch a series off**
- Keys: both readers: open `/charts?case=bar-grouped#bar-grouped`; Tab to "Sent"; Tab to "Opened" and press Space; Tab to "Clicked"; Tab to the chart.
- The page: a chart "Sent and opened" with three series. The legend entries are check boxes. Switching "Opened" off removes its bars from the drawing.
- The reader says: Tab says "Sent", check box, checked; then "Opened", check box, checked; Space says "not checked"; then "Clicked", check box, checked. The module says nothing more when a series is switched off. Tab to the chart gives its name and description and the first day's words with only two series: "<day>, Sent 1,204, Clicked 120". The summary (read in browse mode) still describes all three series, and the table still has the Opened column.

**21. Switch it back on**
- Keys: both readers: from the chart, Shift+Tab twice to "Opened", Space; then Tab twice to the chart.
- The page: the Opened bars return.
- The reader says: "Opened", check box, checked. On the chart the live words list three series again: "<day>, Sent 1,204, Opened 802, Clicked 120".

### The report

**22. Read the report page**
- Keys: NVDA: open `/charts/report?from=2026-10-28&to=2026-11-03`, browse mode, NVDA+T, then H for headings and D for landmarks. VoiceOver: VO+Right arrow through the page, and the rotor (VO+U) for Headings and Landmarks.
- The page: the heading "Engagement report"; a range picker named "Window" and an "Apply" button; a "Grain" switch of links; three counts, "Events", "Opened" and "Reported"; the chart "Engagement over the window" (description "Per day in Asia/Kolkata time", legend "Opened" and "Clicked" as check boxes, a "Show table" switch); and the chart "Reported per period" with its table always shown.
- The reader says: the title "Engagement report" and a heading of the same name. The range picker conveys its title "Window" and the chosen range, 28 October to 3 November (its keys are in the dates script). A navigation landmark named "Grain" holds the links to the grains this window can take (Hour and Day among them); the link for Day is said to be current, and the others are not. The first chart has "Show table"; the second has no such switch, and its table is read straight after its summary with the caption "Reported per period".

**23. Change the grain with a link**
- Keys: both readers: put the reader on the link "Hour" in the Grain navigation and press Enter.
- The page: the address gains `grain=hour`. The page updates in place with no full load: the sidebar stays. The first chart now has one row for each hour (168), and the link "Hour" is the current one. Focus moves to the page's main area.
- The reader says: the page title "Engagement report", once, from the page's live region, within about a second. As focus enters the main area the reader may also say "main" (a landmark): that is not a repeat of the title. Reading the Grain navigation again: "Hour" is current, and "Day" is not.

**24. Back and forward**
- Keys: NVDA: Alt+Left arrow, then Alt+Right arrow. VoiceOver: Command+[ , then Command+] (Safari).
- The page: Back restores the day grain for 28 October to 3 November; Forward restores the hour grain. Neither is a full load, and focus moves to the page's main area each time.
- The reader says: after Back, the page title "Engagement report" once, and in the Grain navigation "Day" is current. After Forward, the title once, and "Hour" is current.

**25. The report's charts by key**
- Keys: both readers: open `/charts/report?from=2026-10-28&to=2026-11-03&grain=hour#report-area`; Tab; End; PageUp; then Tab until the second chart's table.
- The page: the area chart has 168 hourly rows, so PageUp moves 17 rows.
- The reader says: the chart is the group "Engagement over the window, chart", described by its summary and the keys sentence. After Tab: the first hour, then "Opened N, Clicked N". After End: the last hour, then "Opened N, Clicked N". After PageUp: the row 17 hours earlier, with the same pattern. Each key gives its row once.

**26. A window the report refuses**
- Keys: both readers: open `/charts/report?from=2024-01-01&to=2026-11-03`, then Down arrow from the heading.
- The page: a warning message appears above the counts, and the charts show the last 30 days.
- The reader says: a warning with the title "That window can't be shown" and a message that ends "Showing the last 30 days." Then the "Grain" navigation lists the grains for 30 days and "Day" is current.

### A large series

**27. A series cut to fit**
- Keys: both readers: open `/charts/big?n=5000#big`; Down arrow from the title; Tab; End; then PageUp.
- The page: one line chart, "5000 points", drawn from 640 of the 5,000 points. A note under the table says so.
- The reader says: the group "5000 points, chart" and a summary that begins "Random walk:". After Tab: the first point's number, then "Random walk" and a value. After End: the last point's number, 4,999, and its value. PageUp moves 64 rows (640 rows, a step of 64). With the table shown, the note after the table reads: "The drawing shows 640 of 5,000 points; the table lists the points drawn." The table has 640 rows plus its header.

### With script off

**28. No script: a chart**
- Keys: turn script off in the browser (see the README), open `/charts?case=bar-grouped#bar-grouped`, read down the chart, then go back to the top of the chart and press: Tab (to "Sent"), Tab (to "Opened"), Space, Tab (to "Clicked"), Tab (to "Show table"), Space.
- The page: the chart is drawn, with its legend, summary and table. The legend entries and "Show table" work with no script. There is no tooltip.
- The reader says: reading in order gives the same title, legend, group name "Sent and opened, chart", summary and "Show table" as before. Tab says "Sent", check box; then "Opened", and Space switches its series off; the stop after "Clicked" is "Show table" (the chart's drawing is not a Tab stop). The group's description is the summary alone, with no sentence about arrow keys. Space on "Show table" shows the table. Nothing is spoken that the page did not write: no live words.

**29. No script: the report**
- Keys: with script off, open `/charts/report?from=2026-10-28&to=2026-11-03`, put the reader on the link "Hour" and press Enter; then go Back.
- The page: a full page load for each: the address gains `grain=hour`, then returns.
- The reader says: on each load, the reader speaks the page title as it does for any new page (this is the reader's own, not the page's live region), and the "Grain" navigation says "Hour" is current, then "Day" after Back. The two charts and their tables are read as in step 22.

## Results

| | |
|---|---|
| Script | charts.md |
| Tester | |
| Date | |
| Operating system and version | |
| Browser and version | |
| Screen reader and version | |
| Speech settings changed from the defaults | none |
| Build under test (commit) | |
| Proof app and port | UI proof app, 8090 |
| Steps run | |
| Steps failed | |
| Issues opened | |

| Step | Expected | NVDA heard / pass | VoiceOver heard / pass | Notes |
|---|---|---|---|---|
| 1 | Title, legend list, group "Outcomes by week, chart", full summary, "Show table" not checked, once | | | |
| 2 | "Show table" checked; table with 5 columns, 6 rows, caption; nothing else said | | | |
| 3 | Column headers, row names and values read by cell; first row Week 41 | | | |
| 4 | Switch cleared; table still found by table key, one table | | | |
| 5 | Tab: name, group, description, then "Week 41, Reported 42, ..." once | | | |
| 6 | Right arrow: Week 42 then Week 43 words; Left arrow back; once each | | | |
| 7 | Home, End, page keys, Up, Down give the right rows; end of range stays | | | |
| 8 | Escape silent, focus stays; Tab to switch; Shift+Tab keeps place | | | |
| 9 | Line: description, 2-item legend, summary per series; PageDown moves 2 rows | | | |
| 10 | Missing value: live words show a dash; table cell says "No value" | | | |
| 11 | Bars: summary with total 7,816 and "Marked: Goal (1,500)."; rows "Sent N" | | | |
| 12 | Donut: 4-item list with value and share; live "Delivered, Value 9,412, Share 97.5%" | | | |
| 13 | Funnel: stage list; summary 9,412 to 96; rows with "Of first" and "Of previous" | | | |
| 14 | Small charts: named pictures with summary; no Tab stop; three tables | | | |
| 15 | Rupee amounts spoken as one number each; "Jul, Revenue ₹12,50,000" | | | |
| 16 | Right to left: ArrowLeft goes forward, ArrowRight back | | | |
| 17 | "No data" and "Nothing to show for this period."; no group, no table | | | |
| 18 | "Loading Opens by hour" conveyed | | | |
| 19 | Failure words and "Try again" link; Enter reloads | | | |
| 20 | Legend check boxes; "Opened" not checked; live words list two series | | | |
| 21 | "Opened" checked; live words list three series again | | | |
| 22 | Heading, "Window", "Grain" navigation with Day current; second chart has no switch | | | |
| 23 | Hour link: title "Engagement report" once; Hour current; no full load | | | |
| 24 | Back and Forward: title once each time; right grain current | | | |
| 25 | Report area chart: first hour, End, PageUp 17 rows; each once | | | |
| 26 | Warning "That window can't be shown", then "Showing the last 30 days." | | | |
| 27 | Large series: End gives 4,999; PageUp 64 rows; note "640 of 5,000" | | | |
| 28 | No script: chart not a Tab stop; legend and switch work; no keys sentence | | | |
| 29 | No script: grain link loads page; title by the reader; right grain current | | | |
