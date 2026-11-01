// The core runtime, loaded only on pages that need it. Everything is delegated
// from `document`, so a morph or a swap needs no re-wiring.
import { startCommands } from "./commands.js";
import { startForms } from "./forms.js";
import { startNav, startRegions } from "./nav.js";

startCommands();
startForms();
startNav();
startRegions();
