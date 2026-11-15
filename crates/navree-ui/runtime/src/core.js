// The core runtime, loaded only on pages that need it. Everything is delegated
// from `document`, so a morph or a swap needs no re-wiring.
import { startCommands } from "./commands.js";
import { startForms } from "./forms.js";
import { applyEnvelope } from "./ops.js";
import { startNav, startRegions, whileWriting } from "./nav.js";
import { announce, startToasts } from "./ui.js";
import { on, root } from "./dom.js";

startCommands();
startForms();
startNav();
startRegions();
startToasts();
// A module asks for core's own write path, op applier and announcer, so it keeps no copy of their
// state. Any other detail (a server-drawn Cmd::dispatch("ui:core") sends null) is passed over.
on(document, "ui:core", (e) => {
  const d = e.detail;
  if (d && typeof d === "object" && !d.apply) Object.assign(d, { apply: applyEnvelope, writing: whileWriting, say: announce });
});
// What needs a script to work is shown only when this has run.
root().setAttribute("data-ui-js", "");
