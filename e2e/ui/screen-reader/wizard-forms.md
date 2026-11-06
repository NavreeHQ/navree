# Wizard and form errors screen reader script

## Purpose

A wizard is a form in steps: a list of steps, one step's fields at a time, Back and Next, and a review before Finish. Every move is a form post. A form with errors shows them under the fields, and with JavaScript on it also moves focus to the first one and says how many fields need attention. This script settles what a reader says when a step opens, how the step list speaks its states, how fields and their help and errors are named, what is said when a post is refused by the browser and by the server, what is said on the review and on Finish, and that the whole flow works with JavaScript off.

## Page to open

The UI proof app. See [Start the proof apps](README.md#start-the-proof-apps).

- The wizard: http://127.0.0.1:8090/flow/wizard/start. It opens a new draft and goes to its first step, at an address like `/flow/wizard/<id>/basics`. The steps are Basics, Contact, Extras (optional), Terms and Review.
- The deal form: http://127.0.0.1:8090/deals/new.

## Before you start

- JavaScript on, window at least 1200 px wide, browser zoom 100%. Steps 19 and 20 turn JavaScript off; turn it on again afterwards.
- Open the speech viewer (NVDA) or caption panel (VoiceOver).
- Reader mode: browse mode to read, and let NVDA switch to focus mode in fields. VoiceOver: Tab between controls.
- Use these values: deal name `Annual plan`, email `anil.verma@example.com`, access code `monsoon-2026`.
- When a step says "the title is announced", it means the page's title, spoken once by the page's announcer within about two seconds of the page changing. The wizard changes pages in place, with no full reload, so a reader gets no page-load speech of its own.
- Open `/flow/wizard/start` fresh at step 1. Each visit makes a new draft.

## Steps

### Opening the wizard

**1. Open the first step**
- Keys: Type the address `/flow/wizard/start` in the browser. NVDA: NVDA+T after the page loads to hear the title again. VoiceOver: the title is spoken as the page loads; listen for it.
- The page: the browser loads the first step, Basics. The page's title is "New deal: Basics" and its main heading is the same text. The step's own heading is "Basics", and under it the line "What the deal is, and who it is for". Focus may already be in the "Deal name" field, because that field asks for focus when the page loads.
- The reader says: the title "New deal: Basics". Use the headings list (NVDA: NVDA+F7, Headings; VoiceOver: VO+U, Headings): a level 1 heading "New deal: Basics" and a level 2 heading "Basics". The step is also a region named "Basics". If focus is in "Deal name", the reader says that field.

**2. Read the step list**
- Keys: NVDA: H or Down arrow in browse mode to the list named "Steps". VoiceOver: VO+U, Lists, choose "Steps".
- The page: the list has five steps. On a new draft only the first can be used.
- The reader says: a list named "Steps" with 5 items. The items, in order: "Basics (current step)" followed by "What the deal is, and who it is for"; "Contact (not available yet)", a button, unavailable; "Extras (not available yet)" with "Optional"; "Terms (not available yet)"; "Review (not available yet)". The state is in words after each name. For "Basics" the reader should convey "current step" once. If it says "current step" twice in a row (once from the words and once from its own report of the current step), that is a failure. "Basics" is not a button or a link.

**3. Read the fields of Basics**
- Keys: Both readers: Tab until the reader says "Deal name", then Tab to the next control.
- The page: the step has a text field "Deal name" and a group "Who is it for" with two radio buttons. Neither radio is chosen.
- The reader says: "Deal name", edit, required. Then the group name "Who is it for", and its radios: "A new customer", radio button, not checked, 1 of 2 (the reader may phrase the position its own way); "An existing customer", radio button, not checked, 2 of 2. Press the Down arrow (or Right arrow) in the group: the next radio is chosen and the reader says "checked". Choose "A new customer" before the next step.

**4. Press Next with the name empty**
- Keys: Both readers: Tab to "Next" (past "Save and leave") and press Enter (VoiceOver: VO+Space).
- The page: the browser refuses to send an empty required field. Focus goes to "Deal name". The page does not change, and the title stays "New deal: Basics".
- The reader says: focus lands on "Deal name" and the reader says its name and role. The browser may also show a message such as "Please fill out this field"; its words are the browser's own (they differ between Chrome and Firefox and Safari), and the reader may speak them. Write them in the notes. The step fails if nothing is spoken at all, or if focus stays on "Next".

**5. Type a name and move on with Enter**
- Keys: Both readers: in "Deal name" type `Annual plan`, then press Enter.
- The page: Enter presses "Next", not a button in the step list. The wizard moves to Contact. The address changes to end in `/contact`. The title becomes "New deal: Contact". Focus goes to the page's main area.
- The reader says: the title "New deal: Contact", once, within about two seconds. The reader may also say that focus is on the main area; write down what you heard. The step list now has "Basics (completed)" as a button, and "Contact (current step)".

### Contact and Back

**6. Read the fields of Contact**
- Keys: Both readers: Tab until the reader says "Email". Type `anil.verma@example.com`. Tab to the next field and type `monsoon-2026`.
- The page: "Email" is an email field. "Access code" is a password field with a line of help under it.
- The reader says: "Email", edit, and the characters as you type. Then "Access code", the role for a protected field (NVDA "protected edit"; VoiceOver "secure text field"), and its description "Typed again if you leave and come back; a draft never keeps it." The characters of the code are not spoken back as you type (a reader may say "star" or nothing). The description is said once.

**7. Go Back and come on again**
- Keys: Both readers: Tab to "Back" and press Enter (VoiceOver: VO+Space). Then Tab to "Deal name" and press Enter.
- The page: Back saves what was typed without checking it and shows Basics. "Deal name" holds `Annual plan` and "A new customer" is still chosen. Enter moves on to Contact. "Email" still holds `anil.verma@example.com`, and "Access code" is empty, because a draft never keeps a password.
- The reader says: after Back, the title "New deal: Basics". On Basics: "Deal name", edit, `Annual plan`; "A new customer", radio button, checked. After Enter, the title "New deal: Contact". On Contact: "Email", edit, `anil.verma@example.com`; "Access code", protected edit, blank. The title is spoken once per move. Type `monsoon-2026` into "Access code" again.

### Extras and the step list

**8. An optional step**
- Keys: Both readers: Tab to "Next" (past "Back" and "Save and leave") and press Enter.
- The page: the wizard moves to Extras. The title is "New deal: Extras". The step is marked optional. It has a text area "Notes" and a check box "Send me updates".
- The reader says: the title "New deal: Extras". In the step list, "Extras (current step)" followed by "Optional". Tab to the fields: "Notes", edit, multi-line; "Send me updates", check box, not checked. Do not fill them in.

**9. Jump ahead with the step list**
- Keys: Both readers: Shift+Tab back to the step list (or reach it from the top of the page with Tab), and move through it with Tab. When the reader says "Terms", press Enter (VoiceOver: VO+Space).
- The page: finished steps and the next steps are buttons. "Terms" can be reached without filling in the optional Extras. The wizard moves to Terms. The title is "New deal: Terms".
- The reader says: in the step list, "Basics (completed)", button; "Contact (completed)", button; "Extras (current step)" is not a button; "Terms", button, with no state words. After Enter, the title "New deal: Terms". The reader does not say "not available yet" for "Terms".

### Terms and the server

**10. Press Next with the box unticked**
- Keys: Both readers: Tab until the reader says "I accept the terms" and check that it is not checked. Tab to "Next" (past "Back" and "Save and leave") and press Enter (VoiceOver: VO+Space).
- The page: the browser has nothing to refuse, so the server checks the step. It stays on Terms and shows the message "Accept the terms to finish." under the check box. The title is still "New deal: Terms". Focus stays on "Next".
- The reader says: the title "New deal: Terms" again, once. The page does not announce the message itself, and the message is not tied to the check box. Check this: Shift+Tab to "I accept the terms", check box, not checked. Then in browse mode press the Down arrow (VoiceOver: VO+Right) from the check box: the next thing read is "Accept the terms to finish." Write in the notes whether the reader said the message when focus was on the check box.

**11. Tick the box and go to the review**
- Keys: Both readers: with focus on "I accept the terms" press Space. Tab to "Next" (past "Back" and "Save and leave") and press Enter.
- The page: the wizard moves to Review. The title is "New deal: Review". The error message is gone.
- The reader says: on Space, "checked". After Enter, the title "New deal: Review".

### Review and Finish

**12. Read the review**
- Keys: NVDA: H through the headings. VoiceOver: VO+U, Headings.
- The page: under the heading "Review" the line "Check your answers. Nothing is created until you finish." follows. There is one section for each of the four steps, each with a heading, an "Edit" button and a list of answers.
- The reader says: a level 2 heading "Review", the line above, then four regions named "Basics", "Contact", "Extras" and "Terms", each with a level 3 heading of the same name. Each section has a button "Edit Basics", "Edit Contact", "Edit Extras", "Edit Terms" (the button reads "Edit" on screen; its spoken name includes the step). The answers are pairs of a term and its value: "Deal name" with `Annual plan`; "Who is it for" with `A new customer`; "Email" with `anil.verma@example.com`; "Notes" with a dash; "Send me updates" with `No`; "I accept the terms" with `Yes`. The access code is not shown. No section says "This step needs attention."

**13. Edit a step and come back**
- Keys: Both readers: Tab until the reader says "Edit Basics" and press Enter (VoiceOver: VO+Space). Then Tab through the step list to "Review" and press Enter.
- The page: the wizard opens Basics with its answers kept. Then "Review" in the step list returns to the review.
- The reader says: after the first Enter, the title "New deal: Basics" and "Deal name", edit, `Annual plan`. In the step list "Review" is a button. After the second Enter, the title "New deal: Review".

**14. Finish**
- Keys: Both readers: Tab until the reader says "Finish" (it is the last button; it replaces "Next" on the review) and press Enter (VoiceOver: VO+Space).
- The page: the deal is made once and the page changes to one titled "Deal created". It shows a success message: "Created: Annual plan".
- The reader says: the title "Deal created", once. In browse mode, the message "Created: Annual plan" can be read at the top of the page body. The message is a status; the reader may also speak it on its own.

**15. Save and leave**
- Keys: Both readers: open `/flow/wizard/start` again. Type `Annual plan`, choose "A new customer", then Tab to "Save and leave" and press Enter (VoiceOver: VO+Space).
- The page: the wizard saves the draft without checking it and goes to the Flow page. A toast with the text "Draft saved. Come back any time." shows.
- The reader says: the title "Flow", and the toast text "Draft saved. Come back any time." Each is spoken once within about two seconds.

### Form errors on the deal form

**16. Read the deal form**
- Keys: Both readers: open `/deals/new`. Tab through the form: "Deal name", "Company", "Main contact", "Value", the currency control, "Expected close", "Stage", "Owner", "Cancel", "Create deal".
- The page: it is titled "New deal". "Deal name" has an example in it ("Annual platform license") and asks for focus on load. "Company", "Main contact" and "Owner" are choice controls with a prompt.
- The reader says: each field's label as its name, with the right role. "Company" says its current text "Choose a company" and that it opens a list or is collapsed. "Cancel" is a link; "Create deal" is a button. No field is invalid.

**17. Send the form with a bad value**
- Keys: Both readers: in "Deal name" type `Ludhiana renewal`. Tab to "Company", press Enter to open the list, press the Down arrow until the reader says "Acme", press Enter. Tab to "Value" (past "Main contact") and type `lots`. Then Tab to "Create deal" and press Enter (VoiceOver: VO+Space); do not press Enter inside "Value", because focus must move from the button to the field for the reader to speak the field.
- The page: the form is sent without leaving the page. The server refuses "lots". Under "Value" the message "Enter an amount of 0 or more, like 12,500." appears. The field is marked invalid and tied to the message. Focus moves to "Value". The words "1 field needs attention" are spoken by the page.
- The reader says: the field first: "Value", edit, invalid entry (NVDA) or invalid (VoiceOver), `lots`, and its description "Enter an amount of 0 or more, like 12,500." Then, from the page's announcer, "1 field needs attention". Each of the two is spoken once, within about two seconds. The message is not spoken twice. What you typed in "Deal name" and "Company" is still there.

**18. Correct it and send**
- Keys: Both readers: in "Value" select the text and type `12,500.50`. Tab to "Create deal" and press Enter.
- The page: the old message goes away from "Value". The server accepts the deal, and the page changes to the deals list, titled "Deals". A toast with the text "Ludhiana renewal created" shows.
- The reader says: the title "Deals" and the toast text "Ludhiana renewal created", each once, within about two seconds. The reader does not say "1 field needs attention" again.

### JavaScript off

**19. The deal form without script**
- Keys: Turn JavaScript off and reload `/deals/new`. Both readers: type `Ludhiana renewal` in "Deal name". Tab to "Company": it is now an ordinary list; choose "Acme" with the arrow keys (on macOS press Space to open it, the Down arrow to "Acme", then Return). Tab to "Value", type `lots`, Tab to "Create deal" and press Enter.
- The page: the whole page loads again with the error shown. "Deal name" holds `Ludhiana renewal`. Under "Value" is "Enter an amount of 0 or more, like 12,500." Nothing says how many fields need attention, and nothing moves focus to the error ("Deal name" may take focus because it asks for focus on load).
- The reader says: the new page's title "New deal". Find the field with Tab or the form-fields list: "Value", edit, invalid entry (or invalid), `lots`, and the description "Enter an amount of 0 or more, like 12,500." The reader does not say "1 field needs attention": that line needs script.

**20. The wizard without script**
- Keys: Open `/flow/wizard/start` (JavaScript still off). Both readers: in "Deal name" type `Annual plan`, choose "A new customer", press Enter. Then Tab to "Back" and press Enter.
- The page: each move is a full page load. The wizard moves to Contact, and Back returns to Basics with `Annual plan` kept and "A new customer" chosen.
- The reader says: on each load, the reader's own page-load speech, with the titles "New deal: Contact" and then "New deal: Basics". The step list, field names and values are spoken as in steps 2, 3 and 7. Turn JavaScript on again.

## Results

| | |
|---|---|
| Script | wizard-forms.md |
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
| 1 | Title "New deal: Basics"; level 1 and level 2 headings; region "Basics" | | | |
| 2 | List "Steps", 5 items; state words once; current step not a button | | | |
| 3 | Deal name required; radio group "Who is it for", 1 of 2 and 2 of 2 | | | |
| 4 | Browser refuses empty name; focus on Deal name; something spoken | | | |
| 5 | Enter presses Next; title "New deal: Contact" once | | | |
| 6 | Email required; Access code protected with its help; code not echoed | | | |
| 7 | Back keeps values; title once per move; Access code blank | | | |
| 8 | Title "New deal: Extras"; "Optional"; Notes and check box named | | | |
| 9 | Completed steps are buttons; Terms has no state words; title announced | | | |
| 10 | Server refusal: title again; message reachable after the check box | | | |
| 11 | Check box checked; title "New deal: Review" | | | |
| 12 | Four regions with headings and "Edit <step>" buttons; answers as pairs | | | |
| 13 | Edit Basics keeps answers; Review button returns | | | |
| 14 | Title "Deal created"; "Created: Annual plan" readable | | | |
| 15 | Title "Flow" and toast "Draft saved. Come back any time." once | | | |
| 16 | Labels are names; Company says it opens a list; Create deal button | | | |
| 17 | Value invalid with description; "1 field needs attention" once; focus on Value | | | |
| 18 | Title "Deals"; toast "Ludhiana renewal created" once | | | |
| 19 | Error read on the Value field; no count spoken without script | | | |
| 20 | Full loads with titles; same names, states and values | | | |

## Overall result

- [ ] Every step passed in NVDA with Firefox.
- [ ] Every step passed in NVDA with Chrome.
- [ ] Every step passed in VoiceOver with Safari.
- [ ] Every failed step has an issue, with the module name and the step number.
- [ ] Every n/a has a reason in the notes.

Signed off by: ______________________  Date: ______________

Reviewed by: ______________________  Date: ______________
