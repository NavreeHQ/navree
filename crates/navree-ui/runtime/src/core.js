// The core runtime, loaded only on pages that need it. Everything is delegated
// from `document`, so a morph or a swap needs no re-wiring.
import { startCommands } from "./commands.js";
import { startForms } from "./forms.js";
import { startNav, startRegions } from "./nav.js";
import { startToasts } from "./ui.js";
import { root } from "./dom.js";

startCommands();
startForms();
startNav();
startRegions();
startToasts();
// What needs a script to work is shown only when this has run.
root().setAttribute("data-ui-js", "");
